// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Fairing: the boot splash for Steelbore OS Bravais.
//!
//! This binary is the single entry point for every mode Fairing runs in. It
//! carries the CLI skeleton of the Spacecraft Software CLI Standard (R-014):
//! global flags, the output-mode cascade, the `metadata` + `data` envelope,
//! structured errors and diagnostics, `schema` and `describe`; and, from
//! milestone M1, `preview`, which draws the splash with a simulated bar. The
//! splash, shutdown and theme verbs arrive with their milestones and are not
//! advertised until they exist.

#![forbid(unsafe_code)]

mod cli;
mod describe;
mod diagnostic;
mod error;
mod output;
mod preview;
mod schema;
mod selection;
mod splash;
mod theme;
mod theme_file;
mod time;

use std::process::ExitCode;
use std::time::Instant;

use clap::Parser as _;

use crate::cli::{Cli, Command};
use crate::diagnostic::{Diagnostic, Severity};
use crate::error::AppError;
use crate::output::mode::Context;

fn main() -> ExitCode {
    // First-frame latency is measured from here (FRN-SRS-004).
    let origin = Instant::now();
    let argv: Vec<String> = std::env::args().collect();
    let invocation = cli::invocation(&argv);
    let cli = match Cli::try_parse_from(&argv) {
        Ok(cli) => cli,
        Err(error) => return handle_parse_error(&error, &argv, &invocation),
    };
    let context = Context::resolve(&cli.global);
    let result = run(&cli, &context, &invocation, origin);
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => ExitCode::from(error.report(&context)),
    }
}

fn run(cli: &Cli, context: &Context, invocation: &str, origin: Instant) -> Result<(), AppError> {
    Diagnostic::new(
        Severity::Info,
        "OUTPUT_MODE",
        format!(
            "resolved output mode `{}` (color {}, interactive {})",
            context.mode.name(),
            if context.color { "on" } else { "off" },
            if context.interactive { "yes" } else { "no" }
        ),
        invocation,
    )
    .emit(context);
    if context.explore_requested {
        Diagnostic::new(
            Severity::Warn,
            "TUI_FALLBACK",
            "interactive explore mode unavailable; falling back to `--format json`",
            invocation,
        )
        .with_hint("fairing describe --json")
        .emit(context);
    }

    if cli.global.version {
        return cli::print_version(context, invocation, &cli.global.fields);
    }
    match &cli.command {
        Some(Command::Describe) => describe::run(context, invocation, &cli.global),
        Some(Command::Schema { command }) => schema::run(command, context, invocation, &cli.global),
        Some(Command::Preview(args)) => {
            preview::run(args, context, invocation, &cli.global, origin)
        }
        Some(Command::Splash(args)) => splash::run(args, context, invocation, &cli.global, origin),
        Some(Command::Theme { command }) => theme::run(command, context, invocation, &cli.global),
        None => Err(AppError::missing_argument(
            "no subcommand given",
            "fairing describe --json",
            invocation,
        )),
    }
}

/// Clap's own help and errors are mapped onto the house exit codes and envelopes.
fn handle_parse_error(error: &clap::Error, argv: &[String], invocation: &str) -> ExitCode {
    use clap::error::ErrorKind;
    match error.kind() {
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
            // Help is a human surface; clap renders it (exit 0). Its own
            // `print` only fails when stdout is closed, which we cannot report anywhere.
            let _ = error.print();
            ExitCode::SUCCESS
        }
        _ => {
            let context = Context::from_argv(argv);
            let message = error
                .to_string()
                .lines()
                .next()
                .unwrap_or("invalid arguments")
                .trim_start_matches("error: ")
                .trim()
                .to_owned();
            let app_error = AppError::invalid_argument(message, "fairing --help", invocation);
            ExitCode::from(app_error.report(&context))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::output::mode::Mode;

    #[test]
    fn mode_names_are_stable() {
        assert_eq!(Mode::Human.name(), "human");
        assert_eq!(Mode::Json.name(), "json");
    }
}
