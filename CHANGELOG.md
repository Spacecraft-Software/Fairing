<!--
SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
SPDX-License-Identifier: GPL-3.0-or-later
-->

# Changelog

All notable changes to **Fairing** are documented in this file.

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) ·
Versioning: [Semantic Versioning](https://semver.org/spec/v2.0.0.html) · Dates: ISO 8601.

## [Unreleased]

### Added

- M2 boot integration. Themes: the Nickel contract `contracts/fairing-theme.ncl` and the
  reference theme `themes/steelbore.ncl`; `fairing theme check|compile|inspect`, which
  evaluate a theme in memory against the embedded contract and print Nickel's own
  diagnostic on a violation, refuse literal colours, tokens of another palette,
  surface-coloured text and a bar below 3:1, and write a versioned artefact of at most
  4 MiB (postcard metadata and role-indexed images, so one artefact draws every variant of
  its palette); `preview --theme` for artefacts and sources. Progress: the initrd estimate
  against the previous boot's duration with an 8 s first-boot curve, stage 2 from
  `Manager.Progress`, a monotonic eased bar that reaches 100 % only at the greetd handoff,
  the bar carried across switch-root in `/run/fairing/state`, and measured durations
  cached in `/var/lib/fairing/boot-duration`. Lifecycle:
  `fairing splash --stage initrd|system`, one thread and one frame per tick, `READY=1`
  after the first frame, exit within 100 ms of `SIGTERM`, exit within a second of a failed
  unit or rescue/emergency mode, always exit 0, a late page flip costing only the frames
  drawn while it is pending (two seconds without a completed flip end the splash), journal
  lines with a syslog priority; a systemd D-Bus client on its own thread (cargo feature
  `dbus`) with property caching off and every call bounded. NixOS: the module
  `steelbore.fairing` (`nixosModules.fairing`) with `fairing-initrd.service` and
  `fairing.service`, the theme compiled at build time, simpledrm and the configured KMS
  drivers in the initrd, `quiet splash`, a Plymouth assertion, least-privilege hardening,
  a boot-only start condition, and the handoff: `fairing-handoff.service`, which greetd's
  start pulls in once logins are allowed, writes `/run/fairing/handoff` and stops the
  splash, and only that stop fills the bar and records the durations; a boot that finishes
  without greetd ends the splash too. A module evaluation check and two VM tests in
  `nix flake check`, runnable without KVM. CI: the feature matrix, a size gate for the
  initrd binary, and a `nix` job.

- M1 rendering. `fairing-theme`: the house palette file vendored byte-identically and
  compiled by a build script into role tables for the twenty registered colour themes and
  the mono theme (never a retyped hex), `Role`/`Rgb`/`Theme`/`MonoRoles` types, WCAG
  contrast helpers, and the two-stage §11.6 variant resolver with a kernel-faithful
  command-line parser. `fairing-render`: a heap `Frame` in the output's byte order,
  compositor (canvas, built-in vector mark, capsule bar, percentage text beside the bar,
  status line) over a 1920×1080 reference layout scaled uniformly and letterboxed, glyph
  rendering with bundled Inconsolata (OFL-1.1), DRM/KMS backend (preferred mode, two dumb
  buffers, page flips, re-acquire on `ENODEV`), `/dev/fb0` backend through sysfs and
  positional writes with console save/restore, memory backend, fallback chain that names
  every failing backend, 30 Hz cadence with a mockable clock, presenter with first-frame
  timing. `fairing preview` verb with `--seconds`, `--backend`, `--snapshot` (binary PPM),
  `--palette`, `--status`, `--size`, `--fps`, `--dry-run`; agent environments never open a
  VT; the console is restored on every exit path and the snapshot path is opened before any
  device. `PERMISSION_DENIED` (exit 4) and `CONFLICT` (exit 5) error codes. stdout writes
  end quietly on a closed pipe. `fairing schema` types numeric parameters with their bounds
  and omits hidden flags. Dependency qualification rows for every new crate; `cargo deny`
  allows Zlib; `CREDITS.md` (§15.3) for the bundled font and the console palette.
  Review hardening: the snapshot is written beside its target and renamed into place so a
  failed run never truncates an existing file; `--no-color` and `--color never` select the
  mono theme like `NO_COLOR`; only a sibling named on the command line or in
  `SPACECRAFT_THEME` is pinned (§11.6.3), a kernel-declared one is overlaid; the palette
  build rejects non-text fills, base-theme `Lift` tokens and duplicate registrations, and
  the golden tests re-derive every recorded contrast ratio and polarity; frames are capped
  at 256 MiB; `--dry-run` plans a device backend under an agent instead of refusing it.

- M0 repository posture: Cargo workspace with `fairing`, `fairing-render`,
  `fairing-theme`, `fairing-askpass` (all `#![forbid(unsafe_code)]`) and the `xtask` task
  runner; REUSE-compliant licensing (GPL-3.0-or-later software, CC-BY-SA-4.0 manual);
  `.gitattributes`, `.editorconfig`, pinned toolchain, `deny.toml`.
- Requirement set `doc/requirements.toml` (8 needs, 68 requirements, status draft) and
  the Texinfo manual `doc/fairing.texi` with generated Needs and Requirements chapters.
- `cargo xtask`: `req-texi` (generate/check chapters), `trace` (§21.3 traceability matrix
  and gate), `progress` (§17.1 block), `check-eol` (§6.5 gate).
- CLI skeleton (R-014): global flags, output-mode cascade with presence-based agent
  detection, `metadata`+`data` envelope, structured errors and diagnostics,
  `fairing describe`, `fairing schema`, `--version` with attribution.
- Posture and context files: README, AGENTS, CLAUDE, SKILL, CONTRIBUTING, SECURITY,
  COMPLIANCE, DEPENDENCIES, NOTICE, PLAN, TODO.
- CI: fmt, clippy, test, deny, audit, reuse, eol, texinfo and trace jobs; `flake.nix`
  with a development shell and package.

### Changed

- FRN-SRS-032 (draft) no longer asks for `Conflicts=greetd.service`: with both units in
  the boot transaction, systemd resolves that conflict by dropping greetd's start. The
  handoff is now ordering plus a stop from greetd's start, for confirmation at G1.
- FRN-SRS-010 (draft) now reads as the scaled formula the design uses (0.30 times elapsed
  over the cached duration), for confirmation at G1.
- The package derivation builds from an explicit file set and takes `withThemeTool` and
  `withDbus` switches.

[Unreleased]: https://github.com/Spacecraft-Software/Fairing/commits/main
