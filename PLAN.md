<!--
SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
SPDX-License-Identifier: GPL-3.0-or-later
-->

# Fairing — Plan

MVP: M0–M3 — Fairing v0.1

This plan sequences the work against the PRD ("Fairing — Product Requirements Document",
2026-10-06, the Claude Doc the maintainer keeps). `P-` items mirror the PRD's `R-` items
one-to-one (`P-001` ↔ `R-001`); items the PRD does not list carry numbers from `P-006`
within M0 and from `P-100` for gates. A requirement-backed item is ticked only when its
requirements reach `verified` (Steelbore Standard §17.5); until the maintainer baselines
the set at G1, those items stay open even where the code exists. `cargo xtask progress`
reads this file.

## M0 — Repository and posture

- [ ] P-001 Workspace with four crates, forbid(unsafe_code), REUSE headers, LICENSES/ (FRN-SRS-100)
- [x] P-002 README.md, AGENTS.md, CLAUDE.md, SECURITY.md, COMPLIANCE.md, CONTRIBUTING.md, NOTICE.md
- [ ] P-003 CI: build, test, cargo audit, cargo deny, reuse lint, LF check (FRN-SRS-103)
- [ ] P-004 PROJECTS.md entry: Fairing, FRN, Category B with raised subsystems (maintainer, in the Projects repository)
- [x] P-005 doc/fairing.texi with Needs and Requirements nodes carrying this PRD's set
- [x] P-006 xtask task runner: req-texi, trace, progress, check-eol, with tests
- [x] P-007 PLAN.md, TODO.md, DEPENDENCIES.md, CHANGELOG.md, SKILL.md
- [x] P-008 flake.nix with development shell and package via packaging/default.nix

## M1 — Draws a frame

- [ ] P-010 Backend trait; DRM backend with dumb buffer and page flip (FRN-SRS-001, 006, 007)
- [ ] P-011 fbdev backend and fallback chain (FRN-SRS-002, 003)
- [ ] P-012 Compositor: canvas, logo, bar, percentage text, status line (FRN-SRS-053)
- [ ] P-013 fairing preview on a VT (FRN-SRS-083)
- [ ] P-014 CLI skeleton: --version, --json, exit codes, env detection (FRN-SRS-080, 081, 082, 084, 085)

## M2 — Boots on Bravais

- [ ] P-020 Nickel contract, theme check, theme compile, palette resolution (FRN-SRS-040 to 047)
- [ ] P-021 Reference theme steelbore with high-contrast and mono siblings
- [ ] P-022 Hybrid progress: cache, estimate, D-Bus, carry-over, monotonic bar (FRN-SRS-010 to 017)
- [ ] P-023 Lifecycle: three units, READY=1, SIGTERM exit, failure exit (FRN-SRS-030 to 037)
- [ ] P-024 NixOS module steelbore.fairing and Plymouth assertion (FRN-SRS-090, 091, 093)
- [ ] P-025 NixOS VM test, unencrypted boot to greetd, in CI

## M3 — Unlocks a LUKS root

- [ ] P-030 fairing-askpass: inotify watcher, ask.* parser with bounds, socket reply (FRN-SRS-020 to 023, 102)
- [ ] P-031 evdev grab/release and prompt rendering (FRN-SRS-021, 026, 028, 029)
- [ ] P-032 Zeroisation and no-leak analysis with written safety argument (FRN-SRS-024, 025, 104)
- [ ] P-033 Fuzz targets for both parsers in CI (FRN-SRS-101)
- [ ] P-034 VM test extended to a LUKS root with an in-splash prompt (FRN-SRS-092)
- [ ] P-035 Fail-open test: agent disabled, tty agent prompts, boot completes (FRN-SRS-027)

## M4 — Operator and accessibility

- [ ] P-040 Esc journal overlay with scrolling (FRN-SRS-060 to 063)
- [ ] P-041 Text backend and accessible-mode triggers (FRN-SRS-070 to 073)
- [ ] P-042 Reduced-motion handling and frame-sequence playback (FRN-SRS-051, 052)
- [ ] P-043 Shutdown splash (FRN-SRS-035)
- [ ] P-044 Screen-reader verification on the console with a dated note in README.md (§18.4)

## M5 — Release 0.1

- [ ] P-050 Traceability matrix generated in CI, no orphans (§21.3)
- [ ] P-051 Budgets measured on the reference machine, margins in band (§22)
- [ ] P-052 Mutation run triaged, ≥ 80 % line coverage (§21.2 B)
- [ ] P-053 packaging/default.nix, guix.scm, PKGBUILD build the binary (§5.5)
- [ ] P-054 SBOM, release manifest, signed tag (§24.3, §25.3)
- [ ] P-055 Texinfo manual builds to info, html, pdf (§8) (optional)

## Gates

Maintainer actions that unblock ticks above; counted in the PLAN row, not in a milestone.

- [ ] P-100 G1: confirm the 68 draft requirements and set `status = "baselined"` in doc/requirements.toml; then P-001 and P-003 can be verified (inspection) and ticked
- [ ] P-101 Generate and commit flake.lock on a host with GitHub access (`nix flake lock`; the sandbox that produced M0 could not reach github.com)
- [ ] P-102 Re-sign the M0 commits with the maintainer key at squash-merge (§6.3)
- [ ] P-103 Add the PROJECTS.md row in the Projects repository: `| Fairing | FRN | Boot splash for Steelbore OS Bravais | B (fairing-askpass and initrd unit raised to A) | https://github.com/Spacecraft-Software/Fairing | pre-release |`
