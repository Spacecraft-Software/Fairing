// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Inspection evidence for the posture requirements, automated.
//!
//! FRN-SRS-100, FRN-SRS-101 and FRN-SRS-103 are discharged by inspection of
//! the workspace manifests and the CI workflow. These tests perform that
//! inspection on every run, so a change that drops an attribute, a fuzz
//! target or a gate fails here instead of at the next review. They read the
//! files as text: the properties are single lines, and no YAML or TOML parser
//! is worth a dependency for them.

use std::path::{Path, PathBuf};

/// The repository root: this crate is `crates/fairing`.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(relative: &str) -> String {
    let path = root().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The workspace members, as `[workspace] members` lists them.
fn members() -> Vec<String> {
    let manifest = read("Cargo.toml");
    let list = manifest
        .split_once("members = [")
        .and_then(|(_, rest)| rest.split_once(']'))
        .map_or_else(
            || panic!("Cargo.toml has no `members = [...]`"),
            |(list, _)| list,
        );
    list.split(',')
        .map(|m| m.trim().trim_matches('"').to_owned())
        .filter(|m| !m.is_empty())
        .collect()
}

/// The body of `job` in a GitHub workflow: its lines up to the next job.
fn job<'a>(workflow: &'a str, name: &str) -> &'a str {
    let header = format!("\n  {name}:\n");
    let start = workflow
        .find(&header)
        .unwrap_or_else(|| panic!("the workflow has no `{name}` job"))
        + header.len();
    let body = &workflow[start..];
    let end = body
        .match_indices('\n')
        .map(|(i, _)| i + 1)
        .find(|&i| {
            let line = &body[i..];
            line.starts_with("  ") && !line.starts_with("   ") && !line.starts_with("  #")
        })
        .unwrap_or(body.len());
    &body[..end]
}

#[test]
fn every_crate_forbids_unsafe_code() {
    // Verifies: FRN-SRS-100
    let manifest = read("Cargo.toml");
    let lints = manifest.split_once("[workspace.lints.rust]").map_or_else(
        || panic!("Cargo.toml has no [workspace.lints.rust]"),
        |(_, rest)| rest.split("\n[").next().unwrap_or_default(),
    );
    assert!(
        lints
            .lines()
            .any(|l| l.trim() == r#"unsafe_code = "forbid""#),
        "the workspace lint table does not forbid unsafe code"
    );
    let members = members();
    assert!(members.len() >= 5, "{members:?}");
    for member in &members {
        let crate_manifest = read(&format!("{member}/Cargo.toml"));
        assert!(
            crate_manifest.contains("[lints]\nworkspace = true"),
            "{member} does not inherit the workspace lints"
        );
        let roots: Vec<String> = ["src/lib.rs", "src/main.rs", "build.rs"]
            .iter()
            .map(|file| format!("{member}/{file}"))
            .filter(|path| root().join(path).is_file())
            .collect();
        assert!(!roots.is_empty(), "{member} has no crate root");
        for path in roots {
            assert!(
                read(&path)
                    .lines()
                    .any(|l| l.trim() == "#![forbid(unsafe_code)]"),
                "{path} does not carry #![forbid(unsafe_code)]"
            );
        }
    }
}

#[test]
fn both_parsers_are_fuzzed_in_ci_for_ten_minutes_per_release() {
    // Verifies: FRN-SRS-101
    for target in ["ask_parse", "artefact_decode"] {
        assert!(
            root()
                .join(format!("fuzz/fuzz_targets/{target}.rs"))
                .is_file(),
            "no fuzz target {target}"
        );
    }
    let ci = read(".github/workflows/ci.yml");
    assert!(
        ci.contains("    tags: [\"v*\"]"),
        "CI does not run on release tags"
    );
    let fuzz = job(&ci, "fuzz");
    assert!(
        fuzz.contains("target: [ask_parse, artefact_decode]"),
        "{fuzz}"
    );
    assert!(!fuzz.contains("\n    if:"), "the fuzz job is conditional");
    // Ten minutes on every event that is not a pull request, a tag included.
    assert!(
        fuzz.contains("github.event_name == 'pull_request' && 120 || 600"),
        "{fuzz}"
    );
    assert!(
        fuzz.contains("-max_total_time=\"$SECONDS_TO_RUN\""),
        "{fuzz}"
    );
}

#[test]
fn cargo_audit_and_cargo_deny_gate_every_ci_run() {
    // Verifies: FRN-SRS-103
    let ci = read(".github/workflows/ci.yml");
    let triggers = ci
        .split_once("\non:\n")
        .and_then(|(_, rest)| rest.split_once("\n\n"))
        .map_or_else(|| panic!("ci.yml has no `on:` block"), |(on, _)| on);
    assert!(triggers.contains("  push:"), "{triggers}");
    assert!(triggers.contains("  pull_request:"), "{triggers}");
    let deny = job(&ci, "deny");
    assert!(deny.contains("EmbarkStudios/cargo-deny-action@"), "{deny}");
    assert!(deny.contains("command: check"), "{deny}");
    let audit = job(&ci, "audit");
    assert!(
        audit.lines().any(|l| l.trim() == "- run: cargo audit"),
        "{audit}"
    );
    for (name, body) in [("deny", deny), ("audit", audit)] {
        assert!(!body.contains("\n    if:"), "the {name} job is conditional");
        assert!(
            !body.contains("continue-on-error"),
            "the {name} job cannot fail the run"
        );
    }
}
