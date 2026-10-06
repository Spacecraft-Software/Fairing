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

[Unreleased]: https://github.com/Spacecraft-Software/Fairing/commits/main
