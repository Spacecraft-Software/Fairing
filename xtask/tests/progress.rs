// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! End-to-end checks for `cargo xtask progress`.

use xtask::report::Kind;

#[test]
fn renders_plan_and_todo_from_a_temporary_repository() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    std::fs::write(
        dir.path().join("PLAN.md"),
        "# Demo — Plan\n\nMVP: M0–M0 — Demo v0.1\n\n## M0 — Start\n\n- [x] P-001 a\n- [ ] P-002 b\n",
    )
    .unwrap_or_else(|e| panic!("{e}"));
    std::fs::write(
        dir.path().join("TODO.md"),
        "# Demo — TODO\n\n- [ ] T-001 a\n",
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let root = dir.path().display().to_string();
    assert!(xtask::run(["progress", "--root", &root].map(str::to_owned)).is_ok());
    assert!(xtask::run(["progress", "--root", &root, "--json"].map(str::to_owned)).is_ok());
}

#[test]
fn missing_plan_is_not_found() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let root = dir.path().display().to_string();
    let failure = xtask::run(["progress", "--root", &root].map(str::to_owned))
        .err()
        .unwrap_or_else(|| panic!("fail"));
    assert_eq!(failure.kind, Kind::NotFound);
}
