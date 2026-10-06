<!--
SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
SPDX-License-Identifier: GPL-3.0-or-later
-->

# Fairing — TODO

Fine-grained tasks for the milestone in progress. `PLAN.md` holds the milestone items;
this file holds the steps behind them and is rewritten as milestones open. Ticked means
done and checked. `cargo xtask progress` reads this file for the `TODO` row.

## M0 — Repository and posture

- [x] T-001 LICENSES/ with the GPL symlink and the CC-BY-SA-4.0 text; REUSE.toml for the lockfiles and SKILL.md
- [x] T-002 .gitattributes, .editorconfig, .gitignore (+ .agent-log.jsonl), rust-toolchain.toml, rustfmt.toml, deny.toml, .cargo/config.toml
- [x] T-003 Workspace Cargo.toml with shared metadata, lints and the §3.2 release-profile flag register; four crate stubs
- [x] T-004 xtask: requirement model and validator, Texinfo generator with --check
- [x] T-005 xtask: marker scanner, traceability matrix (JSON + Markdown), verdict rules, gate
- [x] T-006 xtask: §17.5 parser and §17.2 renderer with the Standard's worked example as a golden test
- [x] T-007 xtask: §6.5 check-eol over the index (git ls-files --eol), BOM and final-newline checks
- [x] T-008 doc/requirements.toml: 8 needs and 68 requirements transcribed from the PRD with the 2026-10-06 decisions
- [x] T-009 doc/fairing.texi shell, doc/Makefile, root Makefile, spacecraft.css; makeinfo builds with zero warnings
- [x] T-010 CLI skeleton: global flags, mode cascade, envelope, errors, diagnostics, schema, describe, --version
- [x] T-011 CLI integration tests with Verifies markers for FRN-SRS-080, 081, 082, 084, 085
- [x] T-012 README, AGENTS, CLAUDE, SKILL, CONTRIBUTING, SECURITY, COMPLIANCE, DEPENDENCIES, NOTICE, CHANGELOG
- [x] T-013 PLAN.md and TODO.md in §17.5 shape; cargo xtask progress renders them
- [x] T-014 .github/workflows/ci.yml: fmt, clippy, test, deny, audit, reuse, eol, texinfo, trace
- [x] T-015 flake.nix and packaging/default.nix
- [ ] T-016 flake.lock generated and committed (needs github.com access; maintainer)
- [x] T-017 PRD doc: open questions Q-1 to Q-7 recorded as answered 2026-10-06
- [ ] T-018 Draft pull request open against main with CI green
- [ ] T-019 PROJECTS.md row added in the Projects repository (maintainer)
- [ ] T-020 G1: requirements baselined by the maintainer
