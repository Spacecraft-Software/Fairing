// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! `fairing splash`: the boot splash itself, run by the systemd units.
//!
//! `fairing-initrd.service` runs `--stage initrd` from early boot until
//! switch-root; `fairing.service` runs `--stage system` from there until
//! greetd's start stops it (FRN-SRS-030, FRN-SRS-032). Both draw the
//! same compiled theme; the bar is the hybrid progress model of
//! [`progress`]. Nothing here can fail the boot: every way out is exit 0
//! with the console restored and the reason in the journal.

pub mod clock;
#[cfg(feature = "dbus")]
pub mod dbus;
#[cfg(not(feature = "dbus"))]
#[path = "dbus_off.rs"]
pub mod dbus;
pub mod events;
pub mod notify;
pub mod progress;
pub mod run;
pub mod state;

use std::io::Read as _;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use fairing_render::Size;
use fairing_theme::{CompiledTheme, Resolution};
use serde::Serialize;

use crate::cli::{GlobalFlags, SplashArgs, StageArg};
use crate::diagnostic::{Diagnostic, Priority, Severity};
use crate::error::AppError;
use crate::output::envelope::Response;
use crate::output::mode::Context;
use crate::output::write_line;
use crate::selection::{PaletteSources, choose_backend, resolve_palette};
use crate::splash::clock::Boottime;
use crate::splash::notify::Notifier;
use crate::splash::progress::Stage;
use crate::splash::run::{Environment, Outcome, Paths, Plan, Reason};

/// The runnable hint for splash failures.
const HINT: &str = "fairing splash --stage system --dry-run";

/// The longest kernel command line read (the kernel's own limit is 4 KiB on
/// x86-64, `COMMAND_LINE_SIZE`; anything longer is not a command line).
const MAX_CMDLINE_BYTES: u64 = 4096;

#[derive(Debug, Serialize)]
struct PaletteReport {
    slug: &'static str,
    base: &'static str,
    source: &'static str,
    overlay: &'static str,
}

/// What `splash` reports when it ends (or, under `--dry-run`, what it would do).
#[derive(Debug, Serialize)]
struct Report {
    stage: &'static str,
    reason: Option<&'static str>,
    chain: Vec<&'static str>,
    backend: Option<&'static str>,
    theme: String,
    palette: PaletteReport,
    frames: u64,
    dropped: u64,
    first_frame_ms: Option<f64>,
    bar: f32,
    carried_bar: Option<f32>,
    dbus: bool,
    last_status: Option<String>,
    release_ms: Option<f64>,
    planned: bool,
}

/// Runs `fairing splash`.
///
/// # Errors
///
/// `INVALID_ARGUMENT` for an unregistered `--palette`, a bad `--size`, or a
/// device backend under an agent harness; `INTERNAL_ERROR` when the signal
/// handler cannot be installed. A missing output, an unreadable theme or a
/// failed boot are not errors: the splash steps aside with exit 0.
///
/// Implements: FRN-SRS-084
pub fn run(
    args: &SplashArgs,
    context: &Context,
    invocation: &str,
    flags: &GlobalFlags,
    origin: Instant,
) -> Result<(), AppError> {
    let stage = match args.stage {
        StageArg::Initrd => Stage::Initrd,
        StageArg::System => Stage::System,
    };
    let choice = choose_backend(args.backend, HINT, context, invocation, flags.dry_run)?;
    let memory_size = Size::new(args.size.0, args.size.1)
        .map_err(|e| AppError::invalid_argument(e.to_string(), HINT, invocation))?;
    dbus::check_bus(&args.bus).map_err(|why| AppError::invalid_argument(why, HINT, invocation))?;
    let theme = match &args.theme {
        None => CompiledTheme::builtin(),
        Some(path) => match crate::theme_file::load(path, HINT, invocation) {
            Ok(theme) => theme,
            // A splash with an unreadable theme steps aside rather than fail
            // the boot; the journal says why (PRD interface table).
            Err(error) if !flags.dry_run => {
                return step_aside(&error, stage, context, invocation, flags);
            }
            Err(error) => return Err(error),
        },
    };
    let cmdline = match &args.cmdline {
        Some(text) => Some(text.clone()),
        None => read_cmdline(Path::new("/proc/cmdline")),
    };
    let resolution = resolve_palette(
        &PaletteSources {
            explicit: args.palette.as_deref(),
            kernel_cmdline: cmdline.as_deref(),
            declared_default: Some(theme.meta().palette.as_str()),
            verb: "splash",
            strict: false,
        },
        context,
        invocation,
    )?;
    let mut report = Report {
        stage: stage.as_str(),
        reason: None,
        chain: choice.chain().iter().map(|k| k.as_str()).collect(),
        backend: None,
        theme: theme.meta().name.clone(),
        palette: palette_report(&resolution),
        frames: 0,
        dropped: 0,
        first_frame_ms: None,
        bar: 0.0,
        carried_bar: None,
        dbus: false,
        last_status: None,
        release_ms: None,
        planned: flags.dry_run,
    };
    if flags.dry_run {
        return emit(&report, context, invocation, flags);
    }

    let terminate = Arc::new(AtomicBool::new(false));
    for signal in [signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT] {
        signal_hook::flag::register(signal, Arc::clone(&terminate)).map_err(|e| {
            AppError::internal(
                format!("cannot install the signal handler: {e}"),
                invocation,
            )
        })?;
    }
    let notifier = Notifier::new(std::env::var_os("NOTIFY_SOCKET").as_deref());
    let systemd = match (stage, args.bus.as_str()) {
        (Stage::Initrd, _) | (_, "none") => None,
        (Stage::System, bus) => Some(dbus::spawn(bus)),
    };
    let plan = Plan {
        stage,
        choice,
        memory_size,
        theme,
        selection: resolution.selection,
        paths: Paths {
            runtime: args.runtime_dir.clone(),
            state: args.state_dir.clone(),
            sysroot_state: args.sysroot_state_dir.clone(),
        },
        exit_after: args.exit_after.map(Duration::from_secs_f64),
    };
    let emit_diagnostic = |diagnostic: Diagnostic| diagnostic.emit(context);
    let environment = Environment {
        clock: &Boottime,
        terminate: &terminate,
        notifier: &notifier,
        systemd,
        origin,
        emit: &emit_diagnostic,
        invocation,
    };
    let outcome = run::run(&plan, &environment);
    fill(&mut report, &outcome);
    emit(&report, context, invocation, flags)
}

/// Logs why the splash will not draw, tells systemd it started, and exits 0.
fn step_aside(
    error: &AppError,
    stage: Stage,
    context: &Context,
    invocation: &str,
    flags: &GlobalFlags,
) -> Result<(), AppError> {
    Diagnostic::new(
        Severity::Warn,
        "THEME_UNREADABLE",
        format!("{}; the splash steps aside", error.message),
        invocation,
    )
    .with_priority(Priority::Notice)
    .emit(context);
    let notifier = Notifier::new(std::env::var_os("NOTIFY_SOCKET").as_deref());
    if let Err(e) = notifier.ready() {
        Diagnostic::new(
            Severity::Warn,
            "NOTIFY_FAILED",
            format!("cannot send READY=1 to the service manager: {e}"),
            invocation,
        )
        .emit(context);
    }
    let report = Report {
        stage: stage.as_str(),
        reason: Some("theme-unreadable"),
        chain: Vec::new(),
        backend: None,
        theme: String::new(),
        palette: PaletteReport {
            slug: "",
            base: "",
            source: "",
            overlay: "",
        },
        frames: 0,
        dropped: 0,
        first_frame_ms: None,
        bar: 0.0,
        carried_bar: None,
        dbus: false,
        last_status: None,
        release_ms: None,
        planned: false,
    };
    emit(&report, context, invocation, flags)
}

fn fill(report: &mut Report, outcome: &Outcome) {
    report.reason = Some(outcome.reason.as_str());
    report.backend = outcome.backend;
    report.frames = outcome.frames;
    report.dropped = outcome.dropped;
    report.first_frame_ms = outcome.first_frame_ms;
    report.bar = outcome.bar;
    report.carried_bar = outcome.carried_bar;
    report.dbus = outcome.dbus;
    report.last_status.clone_from(&outcome.last_status);
    report.release_ms = outcome.release_ms;
    if outcome.reason == Reason::NoBackend {
        report.chain.clear();
    }
}

/// The kernel command line, if it can be read.
fn read_cmdline(path: &Path) -> Option<String> {
    let mut text = String::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_CMDLINE_BYTES)
        .read_to_string(&mut text)
        .ok()?;
    Some(text)
}

fn palette_report(resolution: &Resolution) -> PaletteReport {
    PaletteReport {
        slug: resolution.selection.slug(),
        base: resolution.base.slug,
        source: resolution.source.as_str(),
        overlay: resolution.overlay.as_str(),
    }
}

fn emit(
    report: &Report,
    context: &Context,
    invocation: &str,
    flags: &GlobalFlags,
) -> Result<(), AppError> {
    if context.mode.is_machine() {
        return Response::new(invocation, report)
            .with_context(context, flags.dry_run)
            .emit(context, &flags.fields);
    }
    let line = if report.planned {
        format!(
            "splash\twould draw the {} stage with theme `{}` in `{}`, trying {}",
            report.stage,
            report.theme,
            report.palette.slug,
            report.chain.join(", then ")
        )
    } else {
        format!(
            "splash\t{} stage ended ({}) after {} frames at {:.0}%",
            report.stage,
            report.reason.unwrap_or("unknown"),
            report.frames,
            report.bar * 100.0
        )
    };
    write_line(&line, invocation)
}
