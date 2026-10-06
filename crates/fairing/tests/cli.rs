// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Black-box tests of the CLI skeleton (R-014) against the built binary.
//!
//! Each test names the requirement it is evidence for; `cargo xtask trace`
//! collects those markers. The requirements stay `draft` until G1, so the
//! markers document intent today and become binding when the set is baselined.

use assert_cmd::Command;
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
