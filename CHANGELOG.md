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

- The shutdown splash says what follows (FRN-SRS-107, FRN-SRS-108, draft): "Restarting" on
  a reboot, kexec or soft reboot, "Powering off" on power-off, "Halting" on halt.
  `fairing splash --stage shutdown --action <action>` picks the line and reports it as
  `action`; the theme contract's optional `layouts.shutdown.messages` give each action
  its line and fall back to `message`. The NixOS module installs one
  `fairing-shutdown@<action>.service` per final target in place of
  `fairing-shutdown.service`, so the action is fixed by the target, never guessed.

- The built-in logo is now the Steelbore logo, drawn as a vector in the theme's accent
  role from `crates/fairing-render/assets/logo/steelbore.svg` (the branding trace,
  recoloured to `currentColor`). It replaces the payload-fairing mark, scales to any
  mode and follows the high-contrast and mono variants like the rest of the theme.

- `fairing splash --handoff-console <path>` (FRN-SRS-106, draft): at the greetd handoff,
  before the display is released, stage 2 paints the greeter's console with the theme's
  canvas and text colours (palette entries 0 and 7), hides its cursor and clears it, so
  the kernel never shows a black console between the splash and the greeter. The NixOS
  module passes greetd's VT. Gantry sets its own palette over it when it starts.

- M5 release 0.1. `cargo xtask dist` builds the release archive: the tree at a commit with
  every crate vendored, bit-reproducible, built offline by all three package definitions,
  `packaging/default.nix` (now the §5.5 release form; the flake still builds its own
  tree), the new `packaging/guix.scm` and the new `packaging/PKGBUILD`, into which it
  writes the version and SHA-256. `cargo xtask sbom` writes a reproducible CycloneDX 1.5
  SBOM and `cargo xtask manifest` the §25.3 release manifest; a release workflow on `v*`
  tags rebuilds and checks both from the tag and drafts the release. The budget register
  (`doc/budgets.toml`, `cargo xtask budgets`) gives each ceiling its requirement, method
  and margin, with the initrd binary's 2.5 MiB as FRN-SRS-105 and a Budgets chapter in
  the manual. CI holds every product crate at 80 % line coverage (`cargo xtask
  coverage`) and builds the three packages from the archive; a weekly cargo-mutants run
  over the password agent and the splash lifecycle fails on any survivor not triaged in
  `doc/mutants.toml`. `RELEASING.md` is the release procedure, `SECURITY.md` states the
  support window, and the dev shell carries pdfTeX for `make pdf`.

### Fixed

- A diagnostic written after stderr has closed no longer crashes the splash. `eprintln!`
  panicked on `EPIPE`, and the release profile turns a panic into an abort, so the
  shutdown splash dumped core at the final kill, when journald exits with it. Stderr
  writes now drop a line that cannot be written.

### Changed

- The initrd copy of the splash (`fairing-initrd`) is built with `opt-level = "s"`,
  which keeps it inside its 2.5 MiB budget (2.13 MiB, a 14.8 % margin); the stage-2 and
  full builds are unchanged.

- The compiled theme format is version 2 (the per-action shutdown messages). A version 1
  artefact is refused; rebuild themes with this release, as the NixOS module does.

- The splash report times the waits the reference-machine budgets bound, each the
  longest of the run: `reacquire_ms` (a lost output to its next frame, with an
  `OUTPUT_BACK` journal line), `ask_detect_ms` (a question's file written to the agent
  opening it, also in the `ASK_OPENED` line), `grab_release_ms` (the frame that answers
  and releases the keyboards) and `overlay_entry_ms` (a journal entry to the shown
  overlay). `cargo xtask measure` collects them from the last boots' journal, with the
  first-frame, fbdev and artefact-load figures from `fairing preview` and `fairing theme
  compile`, and `--record` appends them to `doc/measurements.toml`; RELEASING.md has the
  T490s procedure (T-092, for T-089).
- G1: the maintainer baselined the 69 requirements on 2026-10-16. FRN-SRS-010 names the
  first-boot curve until the cached duration is readable under `/sysroot`; FRN-SRS-032
  names `fairing-handoff.service`; FRN-SRS-034 counts a rise in the failed-unit count
  that lasts 200 ms, or a rescue or emergency start, not a failed unit already present;
  FRN-SRS-090 names all four units; FRN-SRS-093 and the module now use
  `PrivateTmp=disconnected` for `fairing.service` and `fairing-shutdown.service`, so
  neither waits for tmpfiles setup and the /var mount; FRN-SRS-105 binds the Nix build of
  `fairing-initrd`, which CI's `nix` job and the release workflow now measure (2,335,680 B,
  10.9 % margin, amber). The malachite licence exception and the accepted Nickel
  advisories are confirmed. 61 requirements are verified; 8 stay implemented until their
  figures are measured on the T490s. New evidence: posture tests for FRN-SRS-100, 101,
  103 and 104, module checks for FRN-SRS-090, and hardware tests for FRN-SRS-001, 002,
  004 and 083. `fairing-theme`'s build script now carries `#![forbid(unsafe_code)]`.
- `rust-version` is now 1.93, the Rust Guix ships, so `packaging/guix.scm` builds; CI's
  `msrv` job checks every crate and both splash builds on it. The development toolchain
  stays pinned at 1.97.0.
- M4 operator and accessibility. Escape shows the journal over the splash: the boot's last
  40 entries at notice or higher from `journalctl -o json --follow`, newest at the bottom,
  scrolled with Up/Down, PgUp/PgDn and j/k; keyboards are read without a grab and the
  overlay is off while a password prompt is open. Accessible mode (`--accessible`,
  `SPACECRAFT_A11Y=1`, kernel `fairing.accessible=1`; off otherwise, `--no-accessible`
  wins) writes `Fairing <NN>% <status>` to the console at most once a second, prompts as
  plain text, no ANSI sequences, and selects the high-contrast palette. Theme frame
  sequences play in a loop at their rate; reduced motion (accessible mode,
  `--reduced-motion`, `fairing.reduced_motion=1`) keeps the first frame and moves the
  bar without easing. `fairing splash --stage shutdown` and `fairing-shutdown.service`
  draw the theme's shutdown layout from `shutdown.target` until the final kill.
- The stage-2 splash ends on systemd's signals as they arrive, not only on its polls: a
  queued rescue or emergency target (`JobNew`), or an `NFailedUnits` count that stays
  above the first reading for 200 ms. A failed job alone (a restarted service, a device
  timeout) does not end it, and only signals from systemd's own connection count.
- Caps Lock is read from the keyboard's LED when the agent grabs it, and the glyphs drawn
  for an echoed answer are zeroised and dropped once its prompt closes.

- The password agent refuses an `ask.*` file that repeats a key it reads, and answers
  only a socket directly inside the question directory, never through a link. systemd
  writes `Message=` raw after `Socket=`, so a device label with a newline could otherwise
  redirect the passphrase.
- M3 password agent (`fairing-askpass`, Category A), run inside the splash. It watches
  `/run/systemd/ask-password` with inotify and never blocks, so a question is seen within
  a frame. Parsing is bounded: an `ask.*` file over 64 KiB or not UTF-8 is refused before
  any field is read, and every field goes into a fixed buffer. While a question is open,
  every keyboard is grabbed exclusively; keyboards are found from sysfs capabilities,
  keys go through a US layout, and every grab is released the frame the prompt closes.
  The passphrase lives in a fixed 512-byte buffer, zeroised eagerly on every path, and
  the reply is one `+`/`-` datagram. The prompt appears in the theme's prompt region,
  masked or echoed, with a notice when an answer is rejected. The process is
  non-dumpable. `fairing splash --no-agent` turns the agent off; an agent that cannot
  start ends the splash as `agent-failed`. In the NixOS module, `agent.enable` (default
  on) keeps systemd's console agent aside while `/run/fairing/agent` exists. Both units
  remove that file on any exit and re-arm the console agent, and the initrd splash is
  ordered before `cryptsetup-pre.target`. cargo-fuzz targets for the `ask.*` parser and
  the theme artefact run in CI, and a crate-local clippy configuration refuses
  content-sized allocation. A LUKS VM test runs with and without the agent, and the
  manual gains a "Password agent" chapter with the safety argument.
- `flake.lock`, pinning nixpkgs (nixos-unstable) at `7fd4004` from 2026-10-11. CI's `nix`
  job now runs against it with `--no-update-lock-file`.
- M1 figures from the reference machine (ThinkPad T490s, 1920x1080, 2026-10-14): the DRM
  backend drew its first frame in 20.5 ms and held 30.06 Hz; fbdev 37.8 ms and 30.13 Hz;
  no dropped frames in either 5 s run. Both hardware-only tests pass on a free VT.
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
