<!--
SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
SPDX-License-Identifier: GPL-3.0-or-later
-->

# AGENTS.md — Fairing

## Project identity

Fairing is the Rust boot splash for Steelbore OS Bravais: a five-crate Cargo workspace
(four product crates plus the `xtask` task runner) that draws boot progress on DRM/KMS,
acts as the systemd password agent, and hands the screen to greetd. Part of Spacecraft
Software. Assurance **Category B**, with `crates/fairing-askpass` and the initrd
lifecycle unit raised to **Category A** (Steelbore Standard §19). Licence GPL-3.0-or-later.

Milestone state: M0 (repository and posture), M1 (draws a frame), M2 (boots on Bravais:
themes, progress, `fairing splash`, the NixOS module), M3 (the password agent in
`fairing-askpass`, run inside the splash) and M4 (journal overlay, accessible mode and the
text backend, frame sequences and reduced motion, the shutdown splash, failure signals)
and M5 (release 0.1: budgets, coverage and mutation gates, the release archive and the
three package definitions, SBOM, manifest, release workflow) are merged. G1 baselined the
requirement set on 2026-10-16: 61 are `verified`, and 8 stay `implemented` until their
figures are measured on the reference machine (T-089); the tag is the maintainer's. Do not advertise verbs, flags or files
that do not exist yet.

## Build, test, lint

- Build: `cargo build --workspace --locked`
- Test: `cargo test --workspace --locked`
- Lint: `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`,
  then the splash builds: `cargo clippy -p fairing --all-targets --no-default-features
  [--features dbus] --locked -- -D warnings`
- Format check: `cargo fmt --all --check`
- Text-file gate: `cargo xtask check-eol`
- Requirements chapters current: `cargo xtask req-texi --check`
- Traceability gate: `cargo xtask trace` (artifacts in `target/trace/`)
- Progress block: `cargo xtask progress` (add `--explain` for denominators)
- Budgets: `cargo xtask budgets` (CI passes `--measured`), `cargo xtask budgets-texi --check`;
  on the reference machine `cargo xtask measure [--record --machine <text>]` (RELEASING.md)
- Coverage: `cargo llvm-cov --workspace --exclude xtask --locked --json --summary-only
  --output-path target/llvm-cov.json`, then `cargo xtask coverage` (80 % per crate)
- Mutation: `cargo mutants -p fairing-askpass`, `cargo mutants -p fairing --file
  'crates/fairing/src/splash/**' --exclude crates/fairing/src/splash/dbus_off.rs`, and
  `dbus_off.rs` alone with `--no-default-features` (it exists only without `dbus`); then
  `cargo xtask mutants --outcomes mutants.out/outcomes.json` on each (weekly in
  `.github/workflows/mutants.yml`)
- Release archive: `cargo xtask dist [--check]` (stamps the three package definitions),
  `cargo xtask sbom`, `cargo xtask manifest [--check]`; the procedure is `RELEASING.md`
- Manual: `make check` (zero makeinfo warnings), `make info html`
- Dependencies: `cargo deny check` and `cargo audit`
- NixOS module: `nix flake check` (evaluation everywhere; VM tests need x86_64 and KVM).
  Offline, `--override-input nixpkgs path:<a nixpkgs checkout> --no-write-lock-file`; new
  files must be staged (`git add`) before a flake can see them
- Licensing: `reuse lint`

CI (`.github/workflows/ci.yml`) runs exactly these; a command that is not green locally
will not be green there.

## Requirements workflow

- The requirement set is `doc/requirements.toml`. **Never edit `doc/needs.texi` or
  `doc/requirements.texi`** — they are generated. Edit the TOML, run
  `cargo xtask req-texi`, commit the TOML and both generated files together.
- Identifiers `FRN-NEED-NNN` / `FRN-SRS-NNN` are permanent and never reused; a withdrawn
  requirement keeps its row with `status = "withdrawn"`.
- Status flows `draft → baselined → implemented → verified`. Only the maintainer moves a
  requirement out of `draft` (the G1 gate). A `baselined` or `implemented` requirement
  with no `Verifies:` marker fails `cargo xtask trace`; a `draft` one owes nothing yet.
  A requirement is `verified` once its evidence has passed: in CI, or for a hardware test
  in a recorded run on the reference machine. One whose bound is a reference-machine
  figure in `doc/budgets.toml` stays `implemented` until that figure is measured.
- Cite requirements from code and tests, never from prose:

  ```rust
  /// Resolve the theme variant.
  ///
  /// Implements: FRN-SRS-045
  pub fn resolve_variant(...) {}

  #[test]
  fn no_color_selects_mono() {
      // Verifies: FRN-SRS-045
  }
  ```

  The scanner covers `crates/`, `.github/workflows/`, `packaging/`, `flake.nix` and the
  Makefiles. It does not scan `xtask/` (tooling, and its tests hold marker strings as data)
  or any Markdown/Texinfo. An identifier that is not in the set fails the gate.
- Every change that completes work ticks the matching item in `PLAN.md` or `TODO.md` in
  the same commit; a requirement-backed item is ticked only when its requirement is
  `verified`.

## Architectural invariants

- Every crate is `#![forbid(unsafe_code)]` (workspace lint; FRN-SRS-100). Unsafe lives only
  in qualified dependencies listed in `DEPENDENCIES.md`.
- `crates/fairing` is an application: one `AppError` type (`src/error.rs`), one
  `Diagnostic` type (`src/diagnostic.rs`), one `Response<T>` envelope
  (`src/output/envelope.rs`). stdout carries data only; everything else is stderr.
- The output `Context` (`src/output/mode.rs`) is resolved once in `main` and threaded
  through every verb. Never re-read `AI_AGENT`, `NO_COLOR` or `isatty` elsewhere.
- Agent detection is presence-based: `AI_AGENT`/`AGENT` set to any non-empty value other
  than `0`/`false`. Only `CI` carries a truthy value. `CLAUDECODE`, `CURSOR_AGENT` and
  `GEMINI_CLI` are informational: they disable colour and prompts but never change the
  format.
- `fairing schema` is derived from the clap `Command` tree plus `schema::SPECS`. Adding a
  verb means adding a clap variant **and** a `CommandSpec` row in the same commit (a unit
  test enforces both directions). `schema` emits the schema document itself, not the
  envelope.
- Timestamps are `jiff::Timestamp` rendered `YYYY-MM-DDTHH:MM:SSZ`. No local time anywhere.
- Release profile is `panic = "abort"`: `Drop` does not run on panic. Anything that must
  zeroise a secret does so eagerly, never through drop glue alone (design input for
  `fairing-askpass`).
- Colour values come from one place: `crates/fairing-theme/assets/steelbore.toml`, a
  byte-identical copy of the house palette file, read by `fairing-theme`'s build script into
  generated role tables. No bare hex or ANSI colour constants anywhere else. The single
  documented exception is the Linux console's default 16-colour palette in
  `fairing-render/src/palette.rs`, kernel data that stands in for ANSI slots when the mono
  theme is drawn on a framebuffer. Human-mode terminal colour for diagnostics is still
  deferred to M2; tags stay `[TAG]`-only.
- `fairing-render` renders into a heap `Frame` in the output's byte order and copies rows to
  the device. The XRGB8888 red/blue swap lives only in `Frame::paint`/`PixelFormat::encode`
  and is pinned by a test. tiny-skia, fontdue, drm and rustix types never appear in a `pub`
  signature; backends are the closed `Surface` enum, never `Box<dyn Backend>`. The DRM
  backend never waits for a page flip in `present`: it queues the flip and collects the
  event, without blocking, on the next present (a frame drawn while it is pending is
  dropped as `Timeout`); `close` gives a flip in flight at most 50 ms.
- Cargo features of `crates/fairing`: `theme-tool` (Nickel evaluation for `theme
  check|compile`) and `dbus` (the zbus client of `--stage system`), both default. The
  initrd binary is built with neither and must stay within 2.5 MiB, measured on the Nix
  build the module ships (`packages.fairing-initrd`, CI `nix` job; G1, T-091); code
  that only one feature needs is `#[cfg]`-gated, and both feature-off builds are linted
  and tested in CI.
- `fairing splash` never fails a boot: every path after argument checking exits 0, sends
  `READY=1` (after the first frame, or at once when it steps aside) and writes the reason
  in its report. The loop is single-threaded and polls non-blocking inputs once per frame;
  the D-Bus client is the only other thread and talks to the loop over an mpsc channel.
  `NOTIFY_SOCKET` is read in `splash/mod.rs`, `/proc/cmdline` once per run.
- The splash reaches greetd by ordering, not conflict: `fairing.service` is
  `Before=greetd.service`, and `fairing-handoff.service` (a oneshot greetd wants and
  waits for) writes `/run/fairing/handoff` and then stops it. The handoff is ordered
  `After=systemd-user-sessions.service`, as greetd is: ordered after `fairing.service`
  alone it runs at the splash's `READY=1` and ends it at the start of stage 2. Never
  put the stop in greetd's own `ExecStartPre`: greetd is `Type=idle` and systemd holds
  it 5 s. Only a
  SIGTERM with the marker is the handoff (100 % frame, durations cached); any other
  stage-2 SIGTERM is reason `stopped`, and a boot that finishes without greetd ends
  the splash with reason `boot-finished`. Never add
  `Conflicts=greetd.service` (systemd drops greetd's start from the boot transaction).
  At the handoff, and only then, `--handoff-console` (greetd's VT, from the
  module) is painted with the theme canvas before the output is released
  (FRN-SRS-106); never write it on any other reason or under an agent harness.
  `fairing.service` only starts while `/run/fairing` exists (the initrd unit leaves it,
  the stage-2 unit removes it), so `nixos-rebuild switch` never restarts the splash.
- The password agent (`fairing_askpass::agent::Agent`) is one more non-blocking input of
  the splash loop, ticked once per frame; `--no-agent` turns it off. The secret lives only
  in `fairing-askpass` (`Secret`, a fixed 512-byte buffer); the rest of the process sees
  a `PromptView` with the count typed (nothing under `Silent=1`), or the text only
  under `Echo=1`, carried in `EchoText`, a fixed buffer zeroised on clear and drop;
  never copy it into a `String`. Keyboards are
  grabbed only while a question is open and released the tick it closes. While the agent
  is up, `/run/fairing/agent` exists and systemd's console agent, its `.path` watch as
  well as its service, is conditioned on its absence (as upstream does for Plymouth).
  Both units' `ExecStartPost=` try-restart the watch so the condition is checked once the
  agent is up (a watch left running fires the skipped service until it hits its trigger
  limit and fails), and their `ExecStopPost=` remove the flag and restart the watch, so a
  question survives a crashed splash. With
  the agent on, never add `Conflicts=systemd-ask-password-console.service`: the conflict
  is applied when the start job is queued, before the condition is checked, and would
  stop the splash. An agent that cannot start, a question with no keyboard for
  10 s, and a prompt left undrawn by a lost output for 2 s each end the run as
  `agent-failed`, so the console agent asks. The initrd console agent is ordered
  after `fairing-initrd.service`; never order the stage-2 one after
  `fairing.service`, which waits for the `/var` mount an encrypted `/var` needs
  the agent for. The agent is off by default on a non-US console keymap.
- `ask.*` files are parsed by `fairing_askpass::ask::Ask::parse` only. A key it reads
  that appears twice in `[Ask]` refuses the file: systemd writes `Message=` raw, after
  `Socket=` and `Echo=`, so a newline in a device label could otherwise smuggle in a second
  `Socket=` and redirect the passphrase. Never relax that to "last value wins".
- The agent answers only a `Socket=` directly inside the question directory it watches,
  and `reply::send` refuses a path that is a link rather than a socket (maintainer
  decision, 2026-10-14, T-067). Never widen this, and never hand a refused question to
  the console agent: it reads the file with the last value winning.
- zbus connects to `/run/dbus/system_bus_socket` by name, never through
  `DBUS_SYSTEM_BUS_ADDRESS`, and refuses systemd's private socket: zbus 5.19 panics on
  PID 1's replies there and release builds abort on panic.
- Accessible mode is one toggle, `Context::accessible`: `--accessible`/`--no-accessible`,
  then `SPACECRAFT_A11Y=1/0` (both resolved in `mode.rs`), then the kernel's
  `fairing.accessible=1/0`, applied with `Accessible::with_kernel` in `splash/mod.rs`
  only. Off without a trigger; never add auto-detect hints. It selects the text backend
  (`fairing_render::TextConsole`, on `/dev/console`, stderr under an agent harness) and
  implies reduced motion. The text backend writes plain text only: every control
  character becomes a space, and nothing typed into a prompt is ever written.
- The journal overlay reads keyboards read-only and never grabs them (FRN-SRS-063); only
  the agent grabs, and only while a question is open. While a prompt is open the overlay
  is neither toggled nor drawn. journalctl runs as a child only once the overlay is first
  shown, is read without blocking once per frame, and is killed in `finish`.
- `fairing.service` and `fairing-shutdown.service` run with `PrivateTmp=disconnected`
  (FRN-SRS-093, G1): `PrivateTmp=yes` backs the private /tmp and /var/tmp on the host's,
  which orders a unit after `systemd-tmpfiles-setup.service` and the /var mount, too
  late for the splash.
- `fairing-shutdown.service` has no default dependencies and must never conflict with or
  be ordered before `shutdown.target`: it draws until systemd-shutdown's final kill.
- The D-Bus client forwards systemd's `NFailedUnits`, `JobNew` (rescue/emergency) and
  `JobRemoved` (`failed`/`timeout`) signals from a second thread on the same connection;
  on the system bus only signals whose sender is the unique name owning
  `org.freedesktop.systemd1` count. A failed job never ends the splash by itself (a
  `Restart=` service or a device timeout is no failed unit): only a count above the
  baseline for `SIGNAL_SETTLE` does. A failed poll closes the connection, which ends the
  signal thread.
- `fairing preview` under `AI_AGENT`, `CI`, `CLAUDECODE`, `CURSOR_AGENT` or `GEMINI_CLI`
  never opens a VT (`Context::is_agent_environment`): `--backend auto` becomes memory with a
  `[WARN]`, an explicit device backend is exit 2. The theme resolver takes `no_color` from
  the `Context`; `SPACECRAFT_THEME` is read once, in `selection.rs` (`resolve_palette`),
  for both `preview` and `splash`.
- Tests that need a DRM device or `/dev/fb0` are `#[ignore]`d (`cargo test --workspace --
  --ignored` on a free text console; they fail without a device, as they should). One
  carries a `Verifies:` marker only where it is the requirement's evidence; that
  requirement is `verified` by a recorded run on the reference machine, never by CI. Everything else runs against the memory backend, fake sysfs
  trees in temporary directories, and `MemoryBackend::fail_next` (test-only) for the
  lost-output path. The presenter re-opens through an injectable `Reopen` function.
- Prose, comments and documentation use British spelling (colour, serialise); identifiers
  follow the ecosystem's American spelling (`color`, `Selection::Color`, `NO_COLOR`).
- `xtask` is internal tooling, not a shipped CLI: it honours `--json` and the error
  envelope but not the full CLI Standard flag set.
- The release archive and the SBOM are bit-reproducible. Their timestamps are the commit
  time of the last commit that changed archived content (`dist::content_epoch`), and
  `dist::EXPORT_IGNORED` (`packaging/`, `flake.nix`, `flake.lock`, `doc/releases/`) is
  `export-ignore` in `.gitattributes` (a test keeps the two in step), so stamping a
  checksum changes nothing archived. `cargo xtask dist` runs only with rustup's pinned
  cargo (`cargo vendor` output differs between releases; never from `nix develop`) and
  only in a full clone. Never hand-edit a definition's version or checksum line (`cargo
  xtask dist` writes them, matched by exact line format in `dist::stamp`), and never put
  archived content in a release commit.
- `packaging/default.nix` is the §5.5 release form: without `workingTree` it fetches the
  archive and builds from its `vendor/`. The flake passes its own tree as `workingTree`
  and sets the module's default `package` to that tree built with the system's own
  `pkgs` (never `self.packages`, which would bring a second nixpkgs into the closure);
  `module.nix` alone defaults to the release. `callPackage` would fill an argument named `src` from nixpkgs' `src`
  alias, so never rename it to `src`.
- The coverage floor leaves out only `coverage::HARDWARE_ONLY`, each file with its
  reason; a mutant survivor is killed by a test or triaged in `doc/mutants.toml` by
  file, function and replacement, never by line.

## Forbidden patterns

- Editing generated files (`doc/needs.texi`, `doc/requirements.texi`, `target/trace/*`).
- `unwrap()` / `expect()` outside `#[cfg(test)]` (clippy `unwrap_used`/`expect_used`
  warn, CI denies warnings). Use `?` with `AppError` or `Failure`.
- `println!` anywhere in `crates/fairing`: stdout goes through `output::write_line`, which
  ends quietly on a closed pipe and reports any other write failure. `eprintln!` outside
  `src/diagnostic.rs` and `src/error.rs`.
- Hand-written argument parsing, hand-maintained schema JSON, hand-maintained
  traceability tables.
- `chrono::Local`, `jiff::Zoned` with a non-UTC zone, or any `%H:%M` without `T…Z`.
- Adding a dependency without a `DEPENDENCIES.md` row (§24.1) and a green
  `cargo deny check` / `cargo audit`.
- Pushing to a remote outside `Spacecraft-Software` / `UnbreakableMJ`, publishing to
  crates.io, or opening an upstream PR (§6.4). Commits are signed (§6.3); never disable
  signing or touch key material.
- A bare `Verifies:` on a requirement that is not actually tested by that code.
- `as` casts between numeric types in rendering code; use the `geometry` helpers
  (`as_f32`, `round_i32`, `round_u32`) or `try_from`, and `#[expect(..., reason)]` where a
  cast is unavoidable.

## Environment expectations

- Toolchain pinned by `rust-toolchain.toml` (1.97.0, rustfmt, clippy); edition 2024; the
  declared minimum (`rust-version`) is 1.93, Guix's Rust, checked by CI's `msrv` job;
  `Cargo.lock` is committed and `--locked` is the norm.
- `cargo-deny`, `cargo-audit`, `reuse`, `texinfo` (makeinfo/texi2any) and `nushell` come
  from `nix develop`. Without Nix: `cargo install --locked cargo-deny cargo-audit`,
  `pip install reuse`, and texinfo from the host.
- Linux only. No DRM/KMS device is needed for M0/M1 CLI work; rendering tests will need one
  or a VM from M1 onwards.
- Shell scripts and Makefile recipes are POSIX sh (§7); Nushell variants are provided
  where structured data is handled. No Bashisms in committed files.

## Where to look for ___

| Concern | Location |
|---|---|
| Global flags and verb tree | `crates/fairing/src/cli.rs` |
| Output-mode cascade, colour precedence | `crates/fairing/src/output/mode.rs` |
| JSON envelope, `--fields` | `crates/fairing/src/output/envelope.rs` |
| stdout writer, closed-pipe handling | `crates/fairing/src/output/mod.rs` |
| Structured errors and exit codes | `crates/fairing/src/error.rs` |
| Diagnostics and severity floor | `crates/fairing/src/diagnostic.rs` |
| `schema` / `describe` | `crates/fairing/src/schema.rs`, `describe.rs` |
| `preview` | `crates/fairing/src/preview.rs` |
| `splash`: loop, progress model, state files, notify, D-Bus | `crates/fairing/src/splash/` |
| `theme` verbs, theme loading for preview and splash | `crates/fairing/src/{theme,theme_file,selection}.rs` |
| Theme contract, reference theme | `contracts/fairing-theme.ncl`, `themes/steelbore.ncl` |
| Artefact format, compiler, Nickel evaluation, PNG | `crates/fairing-theme/src/{artefact,compile,nickel,png_asset,tokens}.rs` |
| NixOS module, theme build, module and VM tests | `packaging/nixos/` |
| Palette tokens, theme resolution (§11.6) | `crates/fairing-theme/src/{theme,resolve}.rs`, generated by `build.rs` |
| Frame, byte order, PPM snapshot | `crates/fairing-render/src/frame.rs` |
| Layout and scaling | `crates/fairing-render/src/{layout,geometry}.rs` |
| Compositor, logo, text | `crates/fairing-render/src/{compositor,logo,text,shapes}.rs` |
| Backends and fallback chain | `crates/fairing-render/src/backend/` |
| Frame pacing, re-acquire, first-frame timing | `crates/fairing-render/src/{cadence,presenter}.rs` |
| Accessible mode, text backend | `crates/fairing/src/output/mode.rs`, `crates/fairing/src/splash/console.rs`, `crates/fairing-render/src/backend/text.rs` |
| Journal overlay: keys, journalctl, state | `crates/fairing/src/splash/{keys,journal,overlay}.rs` |
| CLI integration tests | `crates/fairing/tests/cli.rs` |
| Password agent: ask files, watch, keyboards, secret, reply | `crates/fairing-askpass/src/` |
| Requirement model, validation, Texinfo generator | `xtask/src/requirements/` |
| Marker scanner, matrix, verdicts | `xtask/src/trace/` |
| §17 progress block | `xtask/src/progress.rs` |
| §6.5 text-file gate | `xtask/src/eol.rs` |
| Tailoring register | `COMPLIANCE.md` |
| Dependency qualification | `DEPENDENCIES.md` |
| Third-party credits (§15.3) | `CREDITS.md` |
| Budgets, coverage and mutation gates | `xtask/src/{budgets,coverage,mutants}.rs`, `doc/{budgets,measurements,mutants}.toml` |
| Reference-machine measurement | `xtask/src/measure.rs`, the `*_ms` fields of the splash report, RELEASING.md |
| Release archive, SBOM, manifest | `xtask/src/{dist,sbom,manifest}.rs`, `doc/releases/`, `RELEASING.md`, `.github/workflows/release.yml` |
| Package definitions | `packaging/{default.nix,guix.scm,PKGBUILD}` |
| Plan and task list | `PLAN.md`, `TODO.md` |

## Standards compliance

This project follows The Steelbore Standard v2.12 and the Spacecraft Software Dual-Mode
Self-Documenting CLI Standard v1.1.0. The `spacecraft-steelbore-standard`,
`spacecraft-cli-standard` and `spacecraft-agentic-cli` skills are authoritative on
house conventions; this file records project-specific invariants and deviations only.
