// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! End-to-end checks for `cargo xtask trace` against a temporary repository.

use std::path::Path;

use xtask::report::Kind;

const SET: &str = include_str!("fixtures/requirements.toml");

fn repo(source: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    std::fs::create_dir_all(dir.path().join("doc")).unwrap_or_else(|e| panic!("{e}"));
    std::fs::create_dir_all(dir.path().join("crates/x/src")).unwrap_or_else(|e| panic!("{e}"));
    std::fs::write(dir.path().join("doc/requirements.toml"), SET).unwrap_or_else(|e| panic!("{e}"));
    std::fs::write(dir.path().join("crates/x/src/lib.rs"), source)
        .unwrap_or_else(|e| panic!("{e}"));
    dir
}

fn run(root: &Path) -> Result<(), xtask::report::Failure> {
    xtask::run(["trace", "--root", &root.display().to_string()].map(str::to_owned))
}

#[test]
fn draft_set_with_valid_markers_passes_and_writes_artifacts() {
    let dir = repo("// Implements: FRN-SRS-001\n// Verifies: FRN-SRS-002\n");
    assert!(run(dir.path()).is_ok());
    let json = std::fs::read_to_string(dir.path().join("target/trace/matrix.json"))
        .unwrap_or_else(|e| panic!("{e}"));
    let matrix: serde_json::Value = serde_json::from_str(&json).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(matrix["ok"], true);
    assert_eq!(matrix["summary"]["total"], 2);
    assert_eq!(matrix["summary"]["unknown"], 0);
    assert!(dir.path().join("target/trace/matrix.md").is_file());
}

#[test]
fn unknown_identifier_fails_the_gate() {
    let dir = repo("// Verifies: FRN-SRS-999\n");
    let failure = run(dir.path()).err().unwrap_or_else(|| panic!("must fail"));
    assert_eq!(failure.kind, Kind::GateFailed);
}

#[test]
fn prose_is_not_scanned() {
    let dir = repo("");
    std::fs::write(dir.path().join("README.md"), "Verifies: FRN-SRS-999\n")
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(run(dir.path()).is_ok(), "markers in Markdown never count");
}
