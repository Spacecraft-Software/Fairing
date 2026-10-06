// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! End-to-end checks for `cargo xtask req-texi` against a temporary repository.

use std::path::Path;

use xtask::report::Kind;

const SET: &str = include_str!("fixtures/requirements.toml");

fn run(root: &Path, extra: &[&str]) -> Result<(), xtask::report::Failure> {
    let mut argv = vec![
        "req-texi".to_owned(),
        "--root".to_owned(),
        root.display().to_string(),
    ];
    argv.extend(extra.iter().map(|s| (*s).to_owned()));
    xtask::run(argv)
}

#[test]
fn generates_then_checks_current_then_detects_stale() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    std::fs::create_dir_all(dir.path().join("doc")).unwrap_or_else(|e| panic!("{e}"));
    std::fs::write(dir.path().join("doc/requirements.toml"), SET).unwrap_or_else(|e| panic!("{e}"));

    assert!(run(dir.path(), &[]).is_ok(), "generation succeeds");
    let needs = dir.path().join("doc/needs.texi");
    let reqs = dir.path().join("doc/requirements.texi");
    assert!(needs.is_file() && reqs.is_file());
    let text = std::fs::read_to_string(&reqs).unwrap_or_else(|e| panic!("{e}"));
    assert!(text.contains("@anchor{FRN-SRS-001}"));
    assert!(text.contains("@ref{FRN-NEED-001}"));
    assert!(text.starts_with("@c SPDX-FileCopyrightText"));

    assert!(
        run(dir.path(), &["--check"]).is_ok(),
        "freshly generated files are current"
    );

    std::fs::write(&reqs, "stale\n").unwrap_or_else(|e| panic!("{e}"));
    let failure = run(dir.path(), &["--check"])
        .err()
        .unwrap_or_else(|| panic!("stale must fail"));
    assert_eq!(failure.kind, Kind::GateFailed);
}

#[test]
fn missing_requirement_set_is_not_found() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let failure = run(dir.path(), &[])
        .err()
        .unwrap_or_else(|| panic!("must fail"));
    assert_eq!(failure.kind, Kind::NotFound);
}

#[test]
fn unknown_flag_is_a_usage_error() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let failure = run(dir.path(), &["--bogus"])
        .err()
        .unwrap_or_else(|| panic!("must fail"));
    assert_eq!(failure.kind, Kind::UsageError);
}
