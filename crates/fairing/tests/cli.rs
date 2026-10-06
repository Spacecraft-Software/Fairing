// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Black-box tests of the CLI skeleton (R-014) against the built binary.
//!
//! Each test names the requirement it is evidence for; `cargo xtask trace`
//! collects those markers. The requirements stay `draft` until G1, so the
//! markers document intent today and become binding when the set is baselined.

use assert_cmd::Command;
use fairing_theme::{Role, Theme};
use predicates::prelude::*;

const ANSI: &str = "\u{1b}[";

/// A command with every cascade-relevant variable cleared, so tests are deterministic.
fn fairing() -> Command {
    let mut cmd = Command::cargo_bin("fairing").unwrap_or_else(|e| panic!("{e}"));
    for var in [
        "AI_AGENT",
        "AGENT",
        "CI",
        "CLAUDECODE",
        "CURSOR_AGENT",
        "GEMINI_CLI",
        "NO_COLOR",
        "FORCE_COLOR",
        "CLICOLOR",
        "TERM",
        "SPACECRAFT_THEME",
    ] {
        cmd.env_remove(var);
    }
    cmd
}

fn json(bytes: &[u8]) -> serde_json::Value {
    serde_json::from_slice(bytes)
        .unwrap_or_else(|e| panic!("not JSON: {e}: {}", String::from_utf8_lossy(bytes)))
}

#[test]
fn version_human_carries_the_attribution_block() {
    // Verifies: FRN-SRS-080
    fairing()
        .args(["--format", "human", "--version"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("fairing 0.1.0\n"))
        .stdout(predicate::str::contains(
            "Maintained by Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>",
        ))
        .stdout(predicate::str::contains(
            "https://Fairing.SpacecraftSoftware.org/",
        ))
        .stdout(predicate::str::contains("Copyright (C) 2026"));
}

#[test]
fn version_json_has_maintainer_and_website_in_metadata() {
    // Verifies: FRN-SRS-080, FRN-SRS-081
    let out = fairing()
        .args(["--version", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value = json(&out);
    assert_eq!(
        value["metadata"]["maintainer"],
        "Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>"
    );
    assert_eq!(
        value["metadata"]["website"],
        "https://Fairing.SpacecraftSoftware.org/"
    );
    assert_eq!(value["data"]["version"], env!("CARGO_PKG_VERSION"));
}

#[test]
fn describe_json_envelope_has_iso8601_utc_timestamp() {
    // Verifies: FRN-SRS-081
    let out = fairing()
        .args(["describe", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(!out.starts_with(&[0xEF, 0xBB, 0xBF]), "no UTF-8 BOM");
    let value = json(&out);
    let stamp = value["metadata"]["timestamp"].as_str().unwrap_or_default();
    assert_eq!(stamp.len(), 20, "{stamp}");
    assert!(stamp.ends_with('Z') && &stamp[10..11] == "T", "{stamp}");
    assert_eq!(value["metadata"]["tool"], "fairing");
    assert_eq!(value["metadata"]["command"], "fairing describe --json");
    assert!(
        value["data"]["commands"]
            .as_array()
            .is_some_and(|c| !c.is_empty())
    );
}

#[test]
fn piped_stdout_defaults_to_json() {
    // Verifies: FRN-SRS-081
    let out = fairing()
        .arg("describe")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(json(&out)["data"].is_object());
}

#[test]
fn fields_narrow_the_payload() {
    // Verifies: FRN-SRS-081
    let out = fairing()
        .args(["describe", "--json", "--fields", "tool,version"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let data = json(&out)["data"].as_object().cloned().unwrap_or_default();
    let mut keys: Vec<&String> = data.keys().collect();
    keys.sort();
    assert_eq!(keys, vec!["tool", "version"]);
}

#[test]
fn agent_env_with_a_descriptive_value_forces_json_without_ansi() {
    // Verifies: FRN-SRS-084
    for value in ["claude-code_2-1-218_agent", "1", "true"] {
        let out = fairing()
            .env("AI_AGENT", value)
            .env("FORCE_COLOR", "1")
            .arg("describe")
            .assert()
            .success()
            .stdout(predicate::str::contains(ANSI).not())
            .get_output()
            .stdout
            .clone();
        assert_eq!(
            json(&out)["data"]["profile"]["reader"],
            "agent",
            "AI_AGENT={value}"
        );
    }
    let out = fairing()
        .env("CI", "true")
        .arg("describe")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(json(&out)["data"]["profile"]["reader"], "ci");
    fairing()
        .env("CI", "false")
        .args(["describe", "--format", "human"])
        .assert()
        .success()
        .stdout(predicate::str::contains("profile: human"));
}

#[test]
fn claudecode_is_informational_but_disables_colour_and_prompts() {
    // Verifies: FRN-SRS-084
    fairing()
        .env("CLAUDECODE", "1")
        .env("FORCE_COLOR", "1")
        .args(["describe", "--format", "human"])
        .assert()
        .success()
        .stdout(predicate::str::contains("mode: human"))
        .stdout(predicate::str::contains("color: off"))
        .stdout(predicate::str::contains("interactive: no"));
    let out = fairing()
        .env("CLAUDECODE", "1")
        .args(["describe", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(json(&out)["metadata"]["tool_agent"], "claude-code");
}

#[test]
fn no_color_and_force_color_precedence() {
    // Verifies: FRN-SRS-085
    fairing()
        .env("NO_COLOR", "1")
        .args(["describe", "--format", "human"])
        .assert()
        .success()
        .stdout(predicate::str::contains("color: off"));
    fairing()
        .env("FORCE_COLOR", "1")
        .args(["describe", "--format", "human"])
        .assert()
        .success()
        .stdout(predicate::str::contains("color: on"));
    fairing()
        .env("FORCE_COLOR", "1")
        .env("NO_COLOR", "1")
        .args(["describe", "--format", "human"])
        .assert()
        .success()
        .stdout(predicate::str::contains("color: on"));
    fairing()
        .env("FORCE_COLOR", "1")
        .args(["describe", "--format", "human", "--no-color"])
        .assert()
        .success()
        .stdout(predicate::str::contains("color: off"));
    let out = fairing()
        .env("FORCE_COLOR", "1")
        .args(["describe", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(
        json(&out)["data"]["profile"]["color"],
        false,
        "machine mode never carries colour"
    );
}

#[test]
fn conflicting_quiet_and_verbose_exit_2_with_structured_error() {
    // Verifies: FRN-SRS-082
    let output = fairing()
        .args(["describe", "--json", "--quiet", "--verbose"])
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty())
        .get_output()
        .clone();
    let error = json(&output.stderr);
    assert_eq!(error["error"]["code"], "INVALID_ARGUMENT");
    assert_eq!(error["error"]["exit_code"], 2);
    assert!(
        error["error"]["hint"]
            .as_str()
            .is_some_and(|h| !h.is_empty())
    );
    let lines = output
        .stderr
        .split(|b| *b == b'\n')
        .filter(|l| !l.is_empty())
        .count();
    assert_eq!(lines, 1, "single-line JSON error");
}

#[test]
fn unknown_schema_target_exits_3_not_found() {
    // Verifies: FRN-SRS-082
    let output = fairing()
        .args(["schema", "nosuch", "--json"])
        .assert()
        .code(3)
        .stdout(predicate::str::is_empty())
        .get_output()
        .clone();
    let error = json(&output.stderr);
    assert_eq!(error["error"]["code"], "NOT_FOUND");
    assert_eq!(error["error"]["hint"], "fairing describe --json");
}

#[test]
fn no_subcommand_is_a_missing_argument() {
    // Verifies: FRN-SRS-082
    let output = fairing()
        .arg("--json")
        .assert()
        .code(2)
        .get_output()
        .clone();
    assert_eq!(json(&output.stderr)["error"]["code"], "MISSING_ARGUMENT");
}

#[test]
fn human_mode_error_carries_tag_hint_and_no_ansi() {
    // Verifies: FRN-SRS-082, FRN-SRS-085
    fairing()
        .env("NO_COLOR", "1")
        .args(["--format", "human", "schema", "nosuch"])
        .assert()
        .code(3)
        .stderr(predicate::str::starts_with("[ERROR] "))
        .stderr(predicate::str::contains("  hint: fairing describe --json"))
        .stderr(predicate::str::contains(ANSI).not());
}

#[test]
fn unavailable_format_is_a_structured_exit_1() {
    // Verifies: FRN-SRS-082
    let output = fairing()
        .args(["describe", "--format", "yaml"])
        .assert()
        .code(1)
        .get_output()
        .clone();
    assert_eq!(json(&output.stderr)["error"]["code"], "FEATURE_UNAVAILABLE");
}

#[test]
fn explore_falls_back_to_json_with_a_warn_diagnostic() {
    // Verifies: FRN-SRS-084
    let output = fairing()
        .args(["describe", "--format", "explore"])
        .assert()
        .success()
        .get_output()
        .clone();
    assert!(json(&output.stdout)["data"].is_object());
    let line = String::from_utf8_lossy(&output.stderr);
    let diag = json(
        line.lines()
            .find(|l| l.contains("TUI_FALLBACK"))
            .unwrap_or_default()
            .as_bytes(),
    );
    assert_eq!(diag["diagnostic"]["severity"], "warn");
    assert_eq!(diag["diagnostic"]["code"], "TUI_FALLBACK");
}

#[test]
fn severity_floor_hides_info_by_default_and_shows_it_under_verbose() {
    // Verifies: FRN-SRS-082
    fairing()
        .args(["describe", "--json"])
        .assert()
        .success()
        .stderr(predicate::str::contains("OUTPUT_MODE").not());
    fairing()
        .args(["describe", "--json", "--verbose"])
        .assert()
        .success()
        .stderr(predicate::str::contains("\"severity\":\"info\""));
    fairing()
        .args(["describe", "--format", "human", "--verbose"])
        .assert()
        .success()
        .stderr(predicate::str::starts_with("[INFO] "));
}

#[test]
fn schema_documents_declare_the_dialect_and_examples() {
    // Verifies: FRN-SRS-081
    let out = fairing()
        .arg("schema")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let tool = json(&out);
    assert_eq!(
        tool["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    for command in tool["commands"].as_array().cloned().unwrap_or_default() {
        for key in [
            "command",
            "description",
            "parameters",
            "output_schema",
            "exit_codes",
            "examples",
            "supports_json",
            "idempotent",
            "destructive",
        ] {
            assert!(
                command.get(key).is_some(),
                "{key} missing from {}",
                command["command"]
            );
        }
        assert!(command["examples"].as_array().is_some_and(|e| {
            e.iter()
                .any(|x| x["command"].as_str().is_some_and(|c| c.contains("--json")))
        }));
    }
    let out = fairing()
        .args(["schema", "describe"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(json(&out)["command"], "fairing describe");
}

#[test]
fn help_exits_zero_with_footer_attribution() {
    fairing()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "https://Fairing.SpacecraftSoftware.org/",
        ))
        .stdout(predicate::str::contains("Maintained by Mohamed Hammad"));
}

/// Preview arguments that never touch a device and finish in one frame.
const MEMORY_FRAME: [&str; 8] = [
    "preview",
    "--backend",
    "memory",
    "--seconds",
    "0",
    "--size",
    "64x36",
    "--json",
];

#[test]
fn preview_memory_snapshot_reports_and_writes_a_ppm_with_the_percentage() {
    // Verifies: FRN-SRS-081, FRN-SRS-053
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let snapshot = dir.path().join("frame.ppm");
    let out = fairing()
        .args([
            "preview",
            "--backend",
            "memory",
            "--seconds",
            "0",
            "--size",
            "640x360",
            "--snapshot",
        ])
        .arg(&snapshot)
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value = json(&out);
    assert_eq!(value["data"]["backend"], "memory");
    assert_eq!(value["data"]["width"], 640);
    assert_eq!(value["data"]["height"], 360);
    assert_eq!(value["data"]["frames"], 1);
    assert_eq!(value["data"]["planned"], false);
    assert_eq!(value["data"]["palette"]["slug"], "steelbore");
    assert_eq!(value["data"]["palette"]["source"], "family-default");
    assert_eq!(
        value["data"]["snapshot"].as_str(),
        Some(snapshot.display().to_string().as_str())
    );
    let bytes = std::fs::read(&snapshot).unwrap_or_else(|e| panic!("{e}"));
    let header = b"P6\n640 360\n255\n";
    assert!(bytes.starts_with(header));
    assert_eq!(bytes.len(), header.len() + 640 * 360 * 3);
    let theme = Theme::family_default();
    let pixels = bytes[header.len()..].chunks_exact(3);
    let paints = |role: Role| {
        let c = theme.color(role);
        pixels.clone().filter(|p| *p == [c.r, c.g, c.b]).count()
    };
    assert!(
        paints(Role::Background) > 640 * 360 / 2,
        "canvas is the background role"
    );
    assert!(paints(Role::Accent) > 100, "the bar is full at 100%");
    assert!(
        paints(Role::Foreground) > 20,
        "percentage and status text are drawn"
    );
    let background = theme.color(Role::Background);
    let inked = |bytes: &[u8]| {
        bytes[header.len()..]
            .chunks_exact(3)
            .filter(|p| *p != [background.r, background.g, background.b])
            .count()
    };
    let quiet = dir.path().join("quiet.ppm");
    fairing()
        .args([
            "preview",
            "--backend",
            "memory",
            "--seconds",
            "0",
            "--size",
            "640x360",
            "--status",
            "",
            "--snapshot",
        ])
        .arg(&quiet)
        .arg("--json")
        .assert()
        .success();
    let quiet_bytes = std::fs::read(&quiet).unwrap_or_else(|e| panic!("{e}"));
    assert!(quiet_bytes.starts_with(header));
    assert!(
        inked(&quiet_bytes) < inked(&bytes),
        "an empty status draws no status line"
    );
}

#[test]
fn preview_honours_the_duration() {
    // Verifies: FRN-SRS-081
    let out = fairing()
        .args([
            "preview",
            "--backend",
            "memory",
            "--seconds",
            "0.3",
            "--size",
            "160x90",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value = json(&out);
    assert_eq!(value["data"]["seconds"], 0.3);
    assert_eq!(value["data"]["fps"], 30);
    assert!(
        value["data"]["frames"].as_u64().is_some_and(|f| f >= 2),
        "{value}"
    );
    assert!(
        value["data"]["measured_fps"]
            .as_f64()
            .is_some_and(|f| f > 0.0)
    );
    assert!(
        value["data"]["first_frame_ms"]
            .as_f64()
            .is_some_and(|ms| ms > 0.0)
    );
}

#[test]
fn agent_environments_preview_into_memory_and_never_open_a_vt() {
    // Verifies: FRN-SRS-084
    for (var, value) in [
        ("AI_AGENT", "claude-code_2-1-218_agent"),
        ("CI", "true"),
        ("CLAUDECODE", "1"),
    ] {
        let output = fairing()
            .env(var, value)
            .args(["preview", "--seconds", "0", "--size", "64x36", "--json"])
            .assert()
            .success()
            .get_output()
            .clone();
        assert_eq!(json(&output.stdout)["data"]["backend"], "memory", "{var}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("AGENT_MEMORY_BACKEND"), "{var}: {stderr}");
    }
    let output = fairing()
        .env("AI_AGENT", "1")
        .args(["preview", "--backend", "drm", "--seconds", "0"])
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty())
        .get_output()
        .clone();
    assert_eq!(json(&output.stderr)["error"]["code"], "INVALID_ARGUMENT");
    fairing()
        .env("AI_AGENT", "1")
        .args(MEMORY_FRAME)
        .assert()
        .success();
}

#[test]
fn preview_theme_flag_is_unavailable_until_m2() {
    // Verifies: FRN-SRS-082
    let output = fairing()
        .args(MEMORY_FRAME)
        .args(["--theme", "steelbore.ncl"])
        .assert()
        .code(1)
        .get_output()
        .clone();
    assert_eq!(json(&output.stderr)["error"]["code"], "FEATURE_UNAVAILABLE");
}

#[test]
fn preview_rejects_unknown_palettes_and_out_of_range_values() {
    // Verifies: FRN-SRS-082
    let output = fairing()
        .args(MEMORY_FRAME)
        .args(["--palette", "no-such-palette"])
        .assert()
        .code(2)
        .get_output()
        .clone();
    let error = json(&output.stderr);
    assert_eq!(error["error"]["code"], "INVALID_ARGUMENT");
    assert_eq!(
        error["error"]["hint"],
        "fairing preview --palette steelbore"
    );
    for bad in [
        ["--seconds", "-1"],
        ["--size", "10"],
        ["--fps", "0"],
        ["--seconds", "nan"],
    ] {
        let output = fairing()
            .args(["preview", "--backend", "memory", "--json"])
            .args(bad)
            .assert()
            .code(2)
            .get_output()
            .clone();
        assert_eq!(
            json(&output.stderr)["error"]["code"],
            "INVALID_ARGUMENT",
            "{bad:?}"
        );
    }
}

#[test]
fn preview_palette_selection_follows_the_resolution_order() {
    // Verifies: FRN-SRS-045
    let report = |cmd: &mut Command| {
        let out = cmd.assert().success().get_output().stdout.clone();
        json(&out)["data"]["palette"].clone()
    };
    let pinned = report(
        fairing()
            .args(MEMORY_FRAME)
            .args(["--palette", "steelbore-high-contrast"]),
    );
    assert_eq!(pinned["slug"], "steelbore-high-contrast");
    assert_eq!(pinned["overlay"], "pinned");
    assert_eq!(pinned["base"], "steelbore");
    assert_eq!(pinned["source"], "command-line");

    let from_env = report(
        fairing()
            .env("SPACECRAFT_THEME", "tokyonight")
            .args(MEMORY_FRAME),
    );
    assert_eq!(from_env["slug"], "tokyonight");
    assert_eq!(from_env["source"], "environment");

    let flag_wins = report(
        fairing()
            .env("SPACECRAFT_THEME", "tokyonight")
            .args(MEMORY_FRAME)
            .args(["--palette", "steelbore-green"]),
    );
    assert_eq!(flag_wins["slug"], "steelbore-green");

    let mono = report(
        fairing()
            .env("NO_COLOR", "1")
            .env("SPACECRAFT_THEME", "steelbore-blue")
            .args(MEMORY_FRAME),
    );
    assert_eq!(mono["slug"], "steelbore-mono");
    assert_eq!(mono["overlay"], "mono");
    assert_eq!(mono["base"], "steelbore-blue");

    let typo = report(
        fairing()
            .env("SPACECRAFT_THEME", "Steelbore")
            .args(MEMORY_FRAME),
    );
    assert_eq!(typo["slug"], "steelbore");
    assert_eq!(typo["skipped"][0]["slug"], "Steelbore");
}

#[test]
fn preview_dry_run_plans_without_opening_a_device() {
    // Verifies: FRN-SRS-081
    let out = fairing()
        .args(["preview", "--dry-run", "--json", "--seconds", "2"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value = json(&out);
    assert_eq!(value["metadata"]["dry_run"], true);
    assert_eq!(value["data"]["planned"], true);
    assert_eq!(value["data"]["backend"], "auto");
    assert_eq!(value["data"]["chain"], serde_json::json!(["drm", "fbdev"]));
    assert!(
        value["data"]["width"].is_null(),
        "a device's mode is not guessed"
    );
    assert_eq!(value["data"]["frames"], 0);
    fairing()
        .args(["preview", "--dry-run", "--format", "human"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "preview\twould try drm, then fbdev\n",
        ));
    let out = fairing()
        .args(["preview", "--dry-run", "--backend", "memory", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value = json(&out);
    assert_eq!(value["data"]["backend"], "memory");
    assert_eq!(value["data"]["chain"], serde_json::json!(["memory"]));
    assert_eq!(value["data"]["width"], 1920);
    assert_eq!(value["data"]["format"], "rgba8888");
    fairing()
        .args([
            "preview",
            "--dry-run",
            "--backend",
            "memory",
            "--format",
            "human",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "preview\twould draw on memory 1920x1080 rgba8888\n",
        ));
}

#[test]
fn preview_without_any_device_exits_not_found_with_the_memory_hint() {
    // Verifies: FRN-SRS-082
    if std::path::Path::new("/dev/dri").exists() || std::path::Path::new("/dev/fb0").exists() {
        return; // A real output exists; the hardware-gated test below covers it.
    }
    let output = fairing()
        .args(["preview", "--seconds", "0", "--json"])
        .assert()
        .code(3)
        .get_output()
        .clone();
    let error = json(&output.stderr);
    assert_eq!(error["error"]["code"], "NOT_FOUND");
    assert_eq!(
        error["error"]["hint"],
        "fairing preview --backend memory --snapshot frame.ppm"
    );
}

#[test]
fn preview_human_report_and_fields_narrowing() {
    // Verifies: FRN-SRS-081
    fairing()
        .args([
            "preview",
            "--backend",
            "memory",
            "--seconds",
            "0",
            "--size",
            "64x36",
            "--format",
            "human",
        ])
        .assert()
        .success()
        .stdout(predicate::str::starts_with(
            "preview\tdrew on memory 64x36 rgba8888\n",
        ))
        .stdout(predicate::str::contains(
            "palette\tsteelbore (base steelbore, from family-default, overlay none)\n",
        ))
        .stdout(predicate::str::contains(
            "frames\t1 in 0.0 s at 30 fps requested, first frame ",
        ))
        .stdout(predicate::str::contains("snapshot").not());
    let out = fairing()
        .args(MEMORY_FRAME)
        .args(["--fields", "backend,frames"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let data = json(&out)["data"].as_object().cloned().unwrap_or_default();
    let mut keys: Vec<&String> = data.keys().collect();
    keys.sort();
    assert_eq!(keys, vec!["backend", "frames"]);
}

#[test]
fn version_honours_fields() {
    // Verifies: FRN-SRS-081
    let out = fairing()
        .args(["--version", "--json", "--fields", "version"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let data = json(&out)["data"].as_object().cloned().unwrap_or_default();
    assert_eq!(data.keys().collect::<Vec<_>>(), vec!["version"]);
}

#[test]
fn preview_snapshot_into_a_missing_directory_exits_not_found() {
    // Verifies: FRN-SRS-082
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let missing = dir.path().join("no-such-dir").join("frame.ppm");
    let output = fairing()
        .args([
            "preview",
            "--backend",
            "memory",
            "--seconds",
            "0",
            "--size",
            "64x36",
            "--snapshot",
        ])
        .arg(&missing)
        .arg("--json")
        .assert()
        .code(3)
        .stdout(predicate::str::is_empty())
        .get_output()
        .clone();
    let error = json(&output.stderr);
    assert_eq!(error["error"]["code"], "NOT_FOUND");
    assert_eq!(
        error["error"]["hint"],
        "fairing preview --backend memory --snapshot ./frame.ppm"
    );
    assert!(!missing.exists());
}

#[test]
fn a_closed_stdout_ends_the_run_quietly() {
    // Verifies: FRN-SRS-081
    let (reader, writer) = std::io::pipe().unwrap_or_else(|e| panic!("{e}"));
    drop(reader);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_fairing"))
        .args(["schema", "--json"])
        .stdout(writer)
        .stderr(std::process::Stdio::piped())
        .output()
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(output.status.success(), "{}", output.status);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("panicked"), "{stderr}");
}

#[test]
fn schema_types_preview_parameters_and_hides_the_theme_flag() {
    // Verifies: FRN-SRS-081
    let out = fairing()
        .args(["schema", "preview"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let document = json(&out);
    let properties = &document["parameters"]["properties"];
    assert_eq!(properties["seconds"]["type"], "number");
    assert_eq!(properties["seconds"]["minimum"], 0);
    assert_eq!(properties["seconds"]["maximum"], 3600);
    assert_eq!(properties["fps"]["type"], "integer");
    assert_eq!(properties["fps"]["default"], 30);
    assert_eq!(properties["size"]["pattern"], "^[0-9]+x[0-9]+$");
    assert!(properties.get("theme").is_none());
    assert_eq!(
        document["output_schema"]["properties"]["data"]["properties"]["backend"]["enum"],
        serde_json::json!(["auto", "drm", "fbdev", "memory"])
    );
    fairing()
        .args(["preview", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--theme").not())
        .stdout(predicate::str::contains("--palette"));
    let out = fairing()
        .arg("schema")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let tool = json(&out);
    assert_eq!(tool["exit_codes"]["5"], "CONFLICT");
    assert!(
        tool["error_codes"]
            .as_array()
            .is_some_and(|codes| codes.iter().any(|c| c == "CONFLICT"))
    );
}

/// Draws on the real output; needs a DRM device or `/dev/fb0` and a free VT.
///
/// `cargo test -p fairing -- --ignored` on a text console (TODO T-029). The
/// FRN-SRS-083 evidence is the maintainer's hardware run, not this assertion.
#[test]
#[ignore = "needs a DRM device or /dev/fb0 and a free VT"]
fn preview_on_the_vt_draws_and_restores_the_console() {
    let out = fairing()
        .args(["preview", "--seconds", "1", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value = json(&out);
    let backend = value["data"]["backend"].as_str().unwrap_or_default();
    assert!(backend == "drm" || backend == "fbdev", "{value}");
    assert!(
        value["data"]["frames"].as_u64().is_some_and(|f| f >= 20),
        "{value}"
    );
}
