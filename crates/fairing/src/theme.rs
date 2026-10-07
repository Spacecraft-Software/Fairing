// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! `fairing theme check | compile | inspect`.
//!
//! `check` and `compile` evaluate a Nickel theme against the embedded
//! contract and the compile rules (FRN-SRS-040 to FRN-SRS-047); they need the
//! `theme-tool` feature, which the initrd build leaves out, and answer
//! `FEATURE_UNAVAILABLE` without it. `inspect` reads a compiled artefact and
//! works in every build.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fairing_theme::{CompiledTheme, FORMAT_VERSION, ImageMeta, Meta, ThemeError, ThemeErrorKind};
use serde::Serialize;

use crate::cli::{GlobalFlags, ThemeCommand};
use crate::error::AppError;
use crate::output::envelope::Response;
use crate::output::mode::Context;
use crate::output::write_line;

/// Times the artefact is loaded to measure load time; the median is reported.
const LOAD_SAMPLES: usize = 21;

#[derive(Debug, Serialize)]
struct Summary<'a> {
    name: &'a str,
    palette: &'a str,
    format_version: u16,
    bytes: usize,
    images: &'a [ImageMeta],
}

#[derive(Debug, Serialize)]
struct CheckReport<'a> {
    source: String,
    valid: bool,
    #[serde(flatten)]
    summary: Summary<'a>,
}

#[derive(Debug, Serialize)]
struct CompileReport<'a> {
    source: String,
    output: String,
    written: bool,
    load_ms: f64,
    #[serde(flatten)]
    summary: Summary<'a>,
}

#[derive(Debug, Serialize)]
struct InspectReport<'a> {
    path: String,
    #[serde(flatten)]
    summary: Summary<'a>,
    meta: &'a Meta,
}

/// Runs a `theme` verb.
///
/// # Errors
///
/// `NOT_FOUND` for a missing file; `INVALID_ARGUMENT` (exit 2) for a theme
/// that breaks the contract or a compile rule, with Nickel's diagnostic as
/// the detail (FRN-SRS-041); `FEATURE_UNAVAILABLE` for `check`/`compile` in a
/// build without `theme-tool`.
pub fn run(
    command: &ThemeCommand,
    context: &Context,
    invocation: &str,
    flags: &GlobalFlags,
) -> Result<(), AppError> {
    match command {
        ThemeCommand::Check(args) => {
            let theme = compile_source(&args.path, invocation)?;
            let bytes = encoded(&theme, invocation)?;
            let report = CheckReport {
                source: args.path.display().to_string(),
                valid: true,
                summary: summary(&theme, bytes.len()),
            };
            let line = format!(
                "theme\t`{}` is valid: `{}` in `{}`, {} bytes, {} image(s)",
                report.source,
                report.summary.name,
                report.summary.palette,
                report.summary.bytes,
                report.summary.images.len()
            );
            emit(&report, &line, context, invocation, flags)
        }
        ThemeCommand::Compile(args) => {
            let theme = compile_source(&args.path, invocation)?;
            let bytes = encoded(&theme, invocation)?;
            let output = args
                .output
                .clone()
                .unwrap_or_else(|| args.path.with_extension("fairing"));
            if !flags.dry_run {
                write_atomically(&output, &bytes, invocation)?;
            }
            let report = CompileReport {
                source: args.path.display().to_string(),
                output: output.display().to_string(),
                written: !flags.dry_run,
                load_ms: median_load(&bytes).as_secs_f64() * 1000.0,
                summary: summary(&theme, bytes.len()),
            };
            let verb = if flags.dry_run {
                "would write"
            } else {
                "wrote"
            };
            let line = format!(
                "theme\t{verb} `{}` ({} bytes, loads in {:.2} ms)",
                report.output, report.summary.bytes, report.load_ms
            );
            emit(&report, &line, context, invocation, flags)
        }
        ThemeCommand::Inspect(args) => {
            let theme =
                crate::theme_file::load(&args.path, "fairing theme compile theme.ncl", invocation)?;
            let bytes = encoded(&theme, invocation)?;
            let report = InspectReport {
                path: args.path.display().to_string(),
                summary: summary(&theme, bytes.len()),
                meta: theme.meta(),
            };
            let line = format!(
                "theme\t`{}` in `{}`, format {}, {} bytes, {} image(s)",
                report.summary.name,
                report.summary.palette,
                report.summary.format_version,
                report.summary.bytes,
                report.summary.images.len()
            );
            emit(&report, &line, context, invocation, flags)
        }
    }
}

fn summary(theme: &CompiledTheme, bytes: usize) -> Summary<'_> {
    Summary {
        name: &theme.meta().name,
        palette: &theme.meta().palette,
        format_version: FORMAT_VERSION,
        bytes,
        images: &theme.meta().images,
    }
}

fn encoded(theme: &CompiledTheme, invocation: &str) -> Result<Vec<u8>, AppError> {
    theme
        .to_bytes()
        .map_err(|e| theme_error(&e, Path::new("theme"), invocation))
}

/// Median time to load `bytes` (FRN-SRS-042 is measured on the reference
/// machine; this is the same measurement wherever `compile` runs).
fn median_load(bytes: &[u8]) -> Duration {
    let mut samples: Vec<Duration> = (0..LOAD_SAMPLES)
        .map(|_| {
            let start = Instant::now();
            let _ = CompiledTheme::from_bytes(bytes);
            start.elapsed()
        })
        .collect();
    samples.sort();
    samples[LOAD_SAMPLES / 2]
}

/// Evaluates and compiles the Nickel theme at `path` in memory.
///
/// # Errors
///
/// As [`run`] describes for `check`.
#[cfg(feature = "theme-tool")]
pub fn compile_source(path: &Path, invocation: &str) -> Result<CompiledTheme, AppError> {
    if !path.is_file() {
        return Err(AppError::not_found(
            format!("theme `{}` does not exist", path.display()),
            "fairing theme check themes/steelbore.ncl",
            invocation,
        ));
    }
    fairing_theme::compile::compile_file(path).map_err(|e| theme_error(&e, path, invocation))
}

/// Without `theme-tool` a Nickel theme cannot be evaluated.
///
/// # Errors
///
/// Always `FEATURE_UNAVAILABLE`.
#[cfg(not(feature = "theme-tool"))]
pub fn compile_source(_path: &Path, invocation: &str) -> Result<CompiledTheme, AppError> {
    Err(AppError::feature_unavailable(
        "this build of fairing cannot evaluate Nickel themes (built without `theme-tool`)",
        invocation,
    ))
}

/// Every theme fault is a usage error: the theme is the argument (FRN-SRS-041).
fn theme_error(error: &ThemeError, path: &Path, invocation: &str) -> AppError {
    let hint = format!("fairing theme check {}", path.display());
    match error.kind() {
        ThemeErrorKind::Contract => AppError::invalid_argument(
            format!(
                "theme `{}` breaks the Fairing theme contract",
                path.display()
            ),
            hint,
            invocation,
        )
        .with_detail(error.input()),
        _ => AppError::invalid_argument(
            format!("theme `{}`: {error}", path.display()),
            hint,
            invocation,
        ),
    }
}

fn write_atomically(path: &Path, bytes: &[u8], invocation: &str) -> Result<(), AppError> {
    let mut part = path.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    let written = std::fs::write(&part, bytes).and_then(|()| std::fs::rename(&part, path));
    written.map_err(|e| {
        let _ = std::fs::remove_file(&part);
        let message = format!("cannot write `{}`: {e}", path.display());
        match e.kind() {
            std::io::ErrorKind::NotFound => AppError::not_found(
                message,
                "fairing theme compile theme.ncl --output ./theme.fairing",
                invocation,
            ),
            std::io::ErrorKind::PermissionDenied => AppError::permission_denied(
                message,
                "fairing theme compile theme.ncl --output ./theme.fairing",
                invocation,
            ),
            _ => AppError::internal(message, invocation),
        }
    })
}

fn emit<T: Serialize>(
    report: &T,
    line: &str,
    context: &Context,
    invocation: &str,
    flags: &GlobalFlags,
) -> Result<(), AppError> {
    if context.mode.is_machine() {
        return Response::new(invocation, report)
            .with_context(context, flags.dry_run)
            .emit(context, &flags.fields);
    }
    write_line(line, invocation)
}
