<!--
SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
SPDX-License-Identifier: GPL-3.0-or-later
-->

# Fairing — TODO

Fine-grained tasks for the milestone in progress. `PLAN.md` holds the milestone items;
this file holds the steps behind them and is rewritten as milestones open. Ticked means
done and checked. `cargo xtask progress` reads this file for the `TODO` row.

## M2 — Boots on Bravais

- [x] T-031 Theme contract `contracts/fairing-theme.ncl` and the reference theme `themes/steelbore.ncl` (the built-in layout written out)
- [x] T-032 fairing-theme: artefact format (magic, version, postcard metadata, role-indexed pixel blob, 4 MiB bound checked before allocation), token lookup across every palette, compile rules for literal colours, foreign tokens, surface-coloured text, bar contrast and geometry; PNG decoding with size caps
- [x] T-033 Nickel evaluation in memory against the embedded contract with plain-text diagnostics, a 64 MiB worker stack and a 30 s deadline (`theme-tool` feature)
- [x] T-034 `fairing theme check|compile|inspect`; `preview --theme` for artefacts and sources; schema and describe rows; CLI tests in both feature sets
- [x] T-035 Compositor from a compiled theme: layout from the spec, role-indexed sprites, status visibility
- [x] T-036 Progress model: initrd estimate and first-boot curve, stage-2 `Manager.Progress` mapping, monotonic eased bar held below 1.00 until the handoff
- [x] T-037 Handoff and duration-cache files: bounded parsing, atomic writes, a stale handoff ignored
- [x] T-038 `fairing splash`: single-threaded loop, SIGTERM through signal-hook, READY=1 and STOPPING=1, journal lines with syslog priority, step-aside paths, failed-unit and maintenance exits
- [x] T-039 zbus client on its own thread (`dbus` feature): property caching off, method timeout, explicit system-bus socket, systemd's private socket refused, p2p fake manager in tests
- [x] T-040 NixOS module `steelbore.fairing`, build-time theme compile, three package variants (full, splash, initrd), flake outputs
- [x] T-041 module-eval check (unit directives, the FRN-SRS-031 negative property, Plymouth assertion), green under `nix flake check --no-build`
- [ ] T-042 VM tests `boot.nix` and `failed-unit.nix` green under KVM (CI `nix` job or the maintainer); then their requirement markers
- [x] T-043 CI: feature-matrix lint and tests, size gate for the initrd binary, `nix` job
- [ ] T-044 DEPENDENCIES.md rows for the M2 crates; accepted advisory RUSTSEC-2026-0292 and the malachite licence exception, both for the maintainer to confirm
- [x] T-045 Manual: splash and theme sections, Themes and Running under systemd chapters; README, AGENTS, SKILL, CHANGELOG
- [ ] T-046 G1 decisions: the FRN-SRS-032 amendment (handoff by stop, not `Conflicts=`), the FRN-SRS-010 carrier, the third unit of FRN-SRS-090 at M4, nickel-lang-core's licence and advisory (maintainer)
- [ ] T-047 Hardware run on the T490s with the module enabled: first frame, 30 Hz, added boot time (maintainer)
- [ ] T-048 M2 pull request reviewed and merged

## M1 — Draws a frame

- [x] T-021 Vendor steelbore.toml byte-identically; build.rs generates the registered set and mono bindings with provenance, sibling and inventory checks
- [x] T-022 fairing-theme API: Role, Rgb (WCAG contrast), Theme, MonoRoles, two-stage resolver with base reporting, kernel command-line parser; golden tests against the TOML
- [x] T-023 fairing-render core: Size/Rect/Viewport (1920x1080 reference, uniform scale, letterbox), Frame with the one-place XRGB byte swap, canonical RenderError with errno classification
- [x] T-024 Compositor: canvas, built-in vector mark, capsule bar with border, percentage beside the bar, status line with ellipsis; bundled Inconsolata glyph cache and lerp blit
- [x] T-025 Backends: DRM/KMS (preferred mode, dumb buffers, page flip, poll with timeout, CRTC restore), fbdev (sysfs geometry, write_at, console save/restore, cursor-blink pause), memory; fallback chain with timings; Presenter with 30 Hz cadence, first-frame stat and re-acquire
- [x] T-026 fairing preview verb: flags, agent rule, dry run, PPM snapshot, structured report; PERMISSION_DENIED error code; schema and describe rows; CLI tests with Verifies markers
- [x] T-027 Font assets with REUSE sidecars and LICENSES/OFL-1.1.txt; .gitattributes binary declarations; deny.toml Zlib
- [x] T-028 DEPENDENCIES.md rows for every M1 crate, accepted advisory RUSTSEC-2026-0192, bundled-asset table; README, AGENTS, SKILL, CHANGELOG, NOTICE, manual chapter
- [ ] T-029 Hardware run on the reference machine (ThinkPad T490s): `cargo test --workspace -- --ignored` on a free VT; record first-frame and 30 Hz figures (maintainer)
- [x] T-030 M1 pull request reviewed and merged

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
- [x] T-018 Draft pull request open against main with CI green
- [ ] T-019 PROJECTS.md row added in the Projects repository (maintainer)
- [ ] T-020 G1: requirements baselined by the maintainer
