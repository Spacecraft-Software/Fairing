<!--
SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
SPDX-License-Identifier: GPL-3.0-or-later
-->

# Fairing — TODO

Fine-grained tasks for the milestone in progress. `PLAN.md` holds the milestone items;
this file holds the steps behind them and is rewritten as milestones open. Ticked means
done and checked. `cargo xtask progress` reads this file for the `TODO` row.

## M4 — Operator and accessibility

- [x] T-068 Event-driven failure detection (P-045, T-052): systemd's `JobRemoved` and `JobNew` signals and the `NFailedUnits` change end the splash within 1 s however slowly PID 1 answers property reads
- [x] T-069 Accessible mode resolved once: `--accessible`/`--no-accessible`, `SPACECRAFT_A11Y`, kernel `fairing.accessible=1`; off unless a trigger is present; reported with its source
- [x] T-070 Text backend: `Fairing <NN>% <status>` at most once a second, prompts as plain text, no ANSI sequences, on the console
- [x] T-071 Frame sequences played in a loop at the theme's rate; reduced motion (accessible mode, `--reduced-motion`, `fairing.reduced_motion=1`) draws the first frame only and moves the bar without easing
- [x] T-072 Journal overlay: Esc toggles the last 40 entries at notice or higher from `journalctl -o json --follow`; Up/Down, PgUp/PgDn and j/k scroll; keyboards read without a grab while no prompt is open
- [x] T-073 Shutdown splash: `fairing splash --stage shutdown` draws the shutdown layout until killed; `fairing-shutdown.service` from shutdown.target to the final kill
- [x] T-074 M3 follow-ups: the Caps Lock state read from the keyboard's LED when a grab starts; glyphs rasterised for an echoed answer dropped when its prompt closes
- [x] T-075 Module, module-eval and VM tests for the shutdown unit, the overlay's `journalctl` in the initrd and accessible mode
- [x] T-076 Manual (overlay, accessible mode, reduced motion, shutdown), README, AGENTS, SKILL, CHANGELOG
- [x] T-077 Adversarial review folded in
- [ ] T-078 M4 pull request reviewed and merged
- [ ] T-079 Screen-reader run on the console with the text backend (Orca or espeakup, brltty), dated note in README.md (maintainer, P-044)

## M3 — Unlocks a LUKS root

- [x] T-053 `ask.*` parser in fairing-askpass: 64 KiB and UTF-8 checked before any field, fixed-capacity fields, `Id=`, `Message=`, `Socket=`, `NotAfter=`, `Echo=`, `PID=`
- [x] T-054 inotify watcher on `/run/systemd/ask-password/`, polled once per frame, new file seen within 50 ms
- [x] T-055 Secret buffer of fixed capacity, zeroised eagerly on reply, cancel, expiry, error and exit (release builds abort on panic); `PR_SET_DUMPABLE=0`, `LimitCORE=0`
- [x] T-056 Reply as one datagram on `Socket=`: `+` and the secret on Enter, `-` on Escape
- [x] T-057 Keyboard input through evdev: discovery from sysfs capabilities, exclusive grab only while a prompt is open and released within 50 ms, US keymap
- [x] T-058 Prompt in the theme's prompt region: message, masked field, `Echo=1` unmasked, rejection notice on a repeated `Id=`, prompt removed at `NotAfter=`
- [x] T-059 Splash integration and fail-open: the agent runs in the splash loop; an agent that cannot start exits 0 within 1 s so the console agent prompts; the console agent stands aside only while Fairing's agent is up
- [x] T-060 cargo-fuzz targets for the `ask.*` parser and the theme artefact, run in CI
- [x] T-061 Safety argument for zeroisation and no secret leak (manual node), lint against content-sized allocation in fairing-askpass
- [x] T-062 LUKS VM test: encrypted root, the prompt answered inside the splash, boot reaches greetd; agent-disabled variant boots through the console agent
- [x] T-063 DEPENDENCIES.md rows for the M3 crates; manual, README, AGENTS, SKILL, CHANGELOG
- [x] T-064 Adversarial review folded in
- [x] T-066 A key the parser reads that appears twice in `[Ask]` refuses the file: systemd 260 writes `Message=` raw after `Socket=` and `Echo=`, so a newline in a device label could smuggle in a second `Socket=`; fixture from the file systemd 260 wrote on the T490s (folded in from PR #6)
- [x] T-067 Reply only to a `Socket=` directly inside the question directory, never through a link (maintainer decision 2026-10-14, secure by design; folded in from PR #6)
- [x] T-065 M3 pull request reviewed and merged

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
- [x] T-042 VM tests `boot.nix` and `failed-unit.nix` green (KVM in CI, or `tcg = true` locally); then their requirement markers
- [x] T-043 CI: feature-matrix lint and tests, size gate for the initrd binary, `nix` job
- [x] T-044 DEPENDENCIES.md rows for the M2 crates; accepted advisory RUSTSEC-2026-0292 and the malachite licence exception, both for the maintainer to confirm
- [x] T-045 Manual: splash and theme sections, Themes and Running under systemd chapters; README, AGENTS, SKILL, CHANGELOG
- [x] T-049 Adversarial review and research critique folded in: explicit handoff marker, READY=1 on every path, stage 2 timed from its own start, one re-open per frame, bounded D-Bus handshake, job start order for the status line, failed-unit baseline, emergency and rescue conflicts, private socket refused by identity, palette skipped not fatal in the splash, tint and image limits checked early, Nickel nesting bound, `fairing-handoff.service` (greetd is `Type=idle`), `boot-finished` exit, `release_ms` in the report, named missing arguments; from the TCG VM runs, the handoff held until logins are allowed (`After=systemd-user-sessions.service`) and a late page flip left in flight, its frames dropped until it lands (2 s stall bound); from the Codex review, flips queued without waiting so SIGTERM never waits on the display
- [ ] T-050 Evaluate Nickel in a child process under memory and stack limits, so any runaway theme (unbounded recursion, a deep import) exits 2 instead of aborting; M2's mitigation is the 5 s deadline and the 256-level nesting bound on the theme file
- [ ] T-051 A stage-2 splash with no D-Bus connection on a boot without greetd has nothing to end it but shutdown; decide a module-side stop (a oneshot after `multi-user.target`) or a time bound
- [x] T-052 (scheduled for M4 as P-045) Event-driven failure detection (systemd's `JobRemoved` and `PropertiesChanged` signals) so FRN-SRS-034's 1 s holds when PID 1 is slow: polled, a failure shows within 250 ms plus five property reads, inside 1 s only while each read takes under about 140 ms
- [ ] T-046 G1 decisions: the FRN-SRS-032 amendment (handoff by stop, not `Conflicts=`); the FRN-SRS-010 wording and carrier; FRN-SRS-034 as a transition (failures already present at the first reading do not count); `PrivateTmp=disconnected` for FRN-SRS-093 (`yes` orders the splash after tmpfiles setup); the third unit of FRN-SRS-090 at M4; nickel-lang-core's licence and advisory; the stage-2 binary over 2.5 MiB (maintainer)
- [ ] T-047 Hardware run on the T490s with the module enabled: first frame, 30 Hz, added boot time (maintainer)
- [x] T-048 M2 pull request reviewed and merged

## M1 — Draws a frame

- [x] T-021 Vendor steelbore.toml byte-identically; build.rs generates the registered set and mono bindings with provenance, sibling and inventory checks
- [x] T-022 fairing-theme API: Role, Rgb (WCAG contrast), Theme, MonoRoles, two-stage resolver with base reporting, kernel command-line parser; golden tests against the TOML
- [x] T-023 fairing-render core: Size/Rect/Viewport (1920x1080 reference, uniform scale, letterbox), Frame with the one-place XRGB byte swap, canonical RenderError with errno classification
- [x] T-024 Compositor: canvas, built-in vector mark, capsule bar with border, percentage beside the bar, status line with ellipsis; bundled Inconsolata glyph cache and lerp blit
- [x] T-025 Backends: DRM/KMS (preferred mode, dumb buffers, page flip, poll with timeout, CRTC restore), fbdev (sysfs geometry, write_at, console save/restore, cursor-blink pause), memory; fallback chain with timings; Presenter with 30 Hz cadence, first-frame stat and re-acquire
- [x] T-026 fairing preview verb: flags, agent rule, dry run, PPM snapshot, structured report; PERMISSION_DENIED error code; schema and describe rows; CLI tests with Verifies markers
- [x] T-027 Font assets with REUSE sidecars and LICENSES/OFL-1.1.txt; .gitattributes binary declarations; deny.toml Zlib
- [x] T-028 DEPENDENCIES.md rows for every M1 crate, accepted advisory RUSTSEC-2026-0192, bundled-asset table; README, AGENTS, SKILL, CHANGELOG, NOTICE, manual chapter
- [x] T-029 Hardware run on the reference machine (ThinkPad T490s): `cargo test --workspace -- --ignored` on a free VT, both ignored tests green; 1920x1080, 5 s `preview`, 152 frames, none dropped: DRM first frame 20.5 ms at 30.06 Hz, fbdev 37.8 ms at 30.13 Hz (2026-10-14, kernel 7.2.10)
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
- [x] T-016 flake.lock generated and committed (nixos-unstable at 7fd4004, 2026-10-11)
- [x] T-017 PRD doc: open questions Q-1 to Q-7 recorded as answered 2026-10-06
- [x] T-018 Draft pull request open against main with CI green
- [ ] T-019 PROJECTS.md row added in the Projects repository (maintainer)
- [ ] T-020 G1: requirements baselined by the maintainer
