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

Milestone state: M0 (repository and posture) and R-014 (CLI skeleton) are done; rendering
(M1), progress and lifecycle (M2), the password agent (M3) and accessibility (M4) are not
started. Do not advertise verbs, flags or files that do not exist yet.

## Build, test, lint

- Build: `cargo build --workspace --locked`
- Test: `cargo test --workspace --locked`
- Lint: `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- Format check: `cargo fmt --all --check`
- Text-file gate: `cargo xtask check-eol`
- Requirements chapters current: `cargo xtask req-texi --check`
- Traceability gate: `cargo xtask trace` (artifacts in `target/trace/`)
- Progress block: `cargo xtask progress` (add `--explain` for denominators)
- Manual: `make check` (zero makeinfo warnings), `make info html`
- Dependencies: `cargo deny check` and `cargo audit`
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
- Human-mode colour is deferred to `fairing-theme` (M2), which carries the §11.1 role
  tokens read from the house `steelbore.toml`. Until then diagnostics are `[TAG]`-only; no
  bare hex or ANSI colour constants anywhere in the code.
- `xtask` is internal tooling, not a shipped CLI: it honours `--json` and the error
  envelope but not the full CLI Standard flag set.

## Forbidden patterns

- Editing generated files (`doc/needs.texi`, `doc/requirements.texi`, `target/trace/*`).
- `unwrap()` / `expect()` outside `#[cfg(test)]` (clippy `unwrap_used`/`expect_used`
  warn, CI denies warnings). Use `?` with `AppError` or `Failure`.
- `println!` / `eprintln!` for data or diagnostics outside `src/output/`,
  `src/diagnostic.rs`, `src/error.rs` and the verb renderers.
- Hand-written argument parsing, hand-maintained schema JSON, hand-maintained
  traceability tables.
- `chrono::Local`, `jiff::Zoned` with a non-UTC zone, or any `%H:%M` without `T…Z`.
- Adding a dependency without a `DEPENDENCIES.md` row (§24.1) and a green
  `cargo deny check` / `cargo audit`.
- Pushing to a remote outside `Spacecraft-Software` / `UnbreakableMJ`, publishing to
  crates.io, or opening an upstream PR (§6.4). Commits are signed (§6.3); never disable
  signing or touch key material.
- A bare `Verifies:` on a requirement that is not actually tested by that code.

## Environment expectations

- Toolchain pinned by `rust-toolchain.toml` (1.97.0, rustfmt, clippy); edition 2024;
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
| Structured errors and exit codes | `crates/fairing/src/error.rs` |
| Diagnostics and severity floor | `crates/fairing/src/diagnostic.rs` |
| `schema` / `describe` | `crates/fairing/src/schema.rs`, `describe.rs` |
| CLI integration tests | `crates/fairing/tests/cli.rs` |
| Requirement model, validation, Texinfo generator | `xtask/src/requirements/` |
| Marker scanner, matrix, verdicts | `xtask/src/trace/` |
| §17 progress block | `xtask/src/progress.rs` |
| §6.5 text-file gate | `xtask/src/eol.rs` |
| Tailoring register | `COMPLIANCE.md` |
| Dependency qualification | `DEPENDENCIES.md` |
| Plan and task list | `PLAN.md`, `TODO.md` |

## Standards compliance

This project follows The Steelbore Standard v2.12 and the Spacecraft Software Dual-Mode
Self-Documenting CLI Standard v1.1.0. The `spacecraft-steelbore-standard`,
`spacecraft-cli-standard` and `spacecraft-agentic-cli` skills are authoritative on
house conventions; this file records project-specific invariants and deviations only.
