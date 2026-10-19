// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! In-tree task runner for the Fairing repository.
//!
//! `cargo xtask <verb>` runs one of the maintenance gates:
//!
//! - `req-texi`  — generate the Texinfo `Needs` and `Requirements` chapters from
//!   `doc/requirements.toml` (Steelbore Standard §20.1), or check they are current.
//! - `trace`     — build the bidirectional traceability matrix from `Verifies:` /
//!   `Implements:` markers and fail on orphans or unknown identifiers (§21.3).
//! - `progress`  — render the §17.1 progress block from `PLAN.md` / `TODO.md`.
//! - `check-eol` — the §6.5 text-file gate (stored line endings, BOM, final newline).
//! - `budgets`, `budgets-texi` — the §22 resource budgets: margins and bands,
//!   and the table the manual includes.
//! - `measure` — the reference-machine figures from a boot's splash reports.
//!
//! This is an internal tool, not a shipped Spacecraft Software CLI: it accepts
//! `--json` and emits the house `error` envelope on stderr, but does not carry the
//! full CLI Standard flag set. Each verb returns `Ok(())` on success and a
//! [`report::Failure`] otherwise; the binary maps that to the exit code.

#![forbid(unsafe_code)]

pub mod args;
pub mod budgets;
pub mod coverage;
pub mod dist;
pub mod eol;
pub mod manifest;
pub mod measure;
pub mod mutants;
pub mod paths;
pub mod progress;
pub mod report;
pub mod requirements;
pub mod sbom;
pub mod trace;

use crate::args::Args;
use crate::report::Failure;

/// Usage text printed by `xtask help`.
pub const USAGE: &str = "\
usage: cargo xtask <verb> [--root <dir>] [--json] [flags]

verbs:
  req-texi   [--requirements <toml>] [--out-dir <dir>] [--check]
  trace      [--requirements <toml>] [--out <dir>]
  progress   [--plan <md>] [--todo <md>] [--prd <md>] [--matrix <json>] [--explain]
  check-eol
  budgets      [--measured id=value,...] [--release]
  budgets-texi [--check]
  measure      [--boots <n>] [--journal <file>] [--preview-drm <json>] [--preview-fbdev <json>]
               [--theme-compile <json>] [--record --machine <text>]
  dist         [--ref <commit>] [--out <dir>] [--check]
  coverage     [--report <json>] [--min <percent>]
  mutants      --outcomes <json> [--triage <toml>]
  sbom         [--ref <commit>] [--out <dir>]
  manifest     [--coverage <json>] [--mutants <json>,...] [--out <dir>] [--check]
  help

exit codes: 0 ok, 1 gate failed, 2 usage error, 3 input not found
";

/// Runs the task runner on `argv` (program name excluded).
///
/// # Errors
///
/// Returns the [`Failure`] the selected verb reported; the caller renders it and
/// exits with [`Failure::exit_code`].
pub fn run<I>(argv: I) -> Result<(), Failure>
where
    I: IntoIterator<Item = String>,
{
    let parsed = Args::parse(argv)?;
    match parsed.verb() {
        "req-texi" => requirements::command::run(&parsed),
        "trace" => trace::command::run(&parsed),
        "progress" => progress::run(&parsed),
        "check-eol" => eol::run(&parsed),
        "budgets" => budgets::run(&parsed),
        "budgets-texi" => budgets::run_texi(&parsed),
        "measure" => measure::run(&parsed),
        "dist" => dist::run_dist(&parsed),
        "coverage" => coverage::run(&parsed),
        "mutants" => mutants::run(&parsed),
        "sbom" => sbom::run(&parsed),
        "manifest" => manifest::run(&parsed),
        "help" | "--help" | "-h" => {
            print!("{USAGE}");
            Ok(())
        }
        other => Err(Failure::usage(
            format!("unknown verb `{other}`"),
            "cargo xtask help",
        )),
    }
}
