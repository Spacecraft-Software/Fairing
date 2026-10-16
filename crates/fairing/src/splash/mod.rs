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
pub mod console;
#[cfg(feature = "dbus")]
pub mod dbus;
#[cfg(not(feature = "dbus"))]
#[path = "dbus_off.rs"]
pub mod dbus;
pub mod events;
pub mod journal;
pub mod keys;
pub mod notify;
pub mod overlay;
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
use crate::output::mode::{Accessible, Context};
use crate::output::write_line;
use crate::selection::{PaletteSources, choose_backend, report_accessible, resolve_palette};
use crate::splash::clock::Boottime;
use crate::splash::console::TextTarget;
use crate::splash::notify::Notifier;
use crate::splash::progress::Stage;
use crate::splash::run::{AgentPaths, Environment, Outcome, OverlayPaths, Paths, Plan, Reason};

/// The runnable hint for splash failures.
const HINT: &str = "fairing splash --stage system --dry-run";

/// The kernel parameter that turns accessible mode on (`1`) or keeps it off
/// (`0`), below `--accessible` and `SPACECRAFT_A11Y` (FRN-SRS-070).
pub const KERNEL_ACCESSIBLE: &str = "fairing.accessible";

/// The kernel parameter that turns reduced motion on (`1`, FRN-SRS-052).
pub const KERNEL_REDUCED_MOTION: &str = "fairing.reduced_motion";

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
    agent: bool,
    answered: u32,
    accessible: bool,
    reduced_motion: bool,
    planned: bool,
}

impl Report {
    /// A report for `stage` with nothing drawn, decided or planned yet.
    fn new(stage: Stage) -> Self {
        Self {
            stage: stage.as_str(),
            reason: None,
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
            agent: false,
            answered: 0,
            accessible: false,
            reduced_motion: false,
            planned: false,
        }
    }
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
        StageArg::Shutdown => Stage::Shutdown,
    };
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
    let Modes {
        accessible,
        reduced_motion,
        text,
    } = modes(args, context, cmdline.as_deref(), invocation);
    // The text backend opens no VT, so the agent rule has nothing to decide.
    let choice = if text.is_some() {
        fairing_render::Choice::Memory
    } else {
        choose_backend(args.backend, HINT, context, invocation, flags.dry_run)?
    };
    let resolution = resolve_palette(
        &PaletteSources {
            explicit: args.palette.as_deref(),
            kernel_cmdline: cmdline.as_deref(),
            declared_default: Some(theme.meta().palette.as_str()),
            verb: "splash",
            strict: false,
            accessible: accessible.on,
        },
        context,
        invocation,
    )?;
    let mut report = Report {
        chain: if text.is_some() {
            vec!["text"]
        } else {
            choice.chain().iter().map(|k| k.as_str()).collect()
        },
        theme: theme.meta().name.clone(),
        palette: palette_report(&resolution),
        agent: !args.no_agent && stage != Stage::Shutdown,
        accessible: accessible.on,
        reduced_motion,
        planned: flags.dry_run,
        ..Report::new(stage)
    };
    if flags.dry_run {
        return emit(&report, context, invocation, flags);
    }

    let terminate = termination_flag(invocation)?;
    let notifier = Notifier::new(std::env::var_os("NOTIFY_SOCKET").as_deref());
    let systemd = match (stage, args.bus.as_str()) {
        (Stage::Initrd | Stage::Shutdown, _) | (_, "none") => None,
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
        // Nothing asks for a password while the system shuts down; a question
        // that does is the console agent's.
        agent: agent_paths(args).filter(|_| stage != Stage::Shutdown),
        overlay: text.is_none().then(|| overlay_paths(args)),
        text,
        reduced_motion,
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

/// The password agent's directories, unless `--no-agent`.
fn agent_paths(args: &SplashArgs) -> Option<AgentPaths> {
    (!args.no_agent).then(|| AgentPaths {
        ask_dir: args.ask_dir.clone(),
        input_sysfs: args.input_sysfs.clone(),
        input_dev: args.input_dev.clone(),
    })
}

/// What the journal overlay reads.
fn overlay_paths(args: &SplashArgs) -> OverlayPaths {
    OverlayPaths {
        input_sysfs: args.input_sysfs.clone(),
        input_dev: args.input_dev.clone(),
        journalctl: args.journalctl.clone(),
    }
}

/// A flag SIGTERM and SIGINT set.
fn termination_flag(invocation: &str) -> Result<Arc<AtomicBool>, AppError> {
    let terminate = Arc::new(AtomicBool::new(false));
    for signal in [signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT] {
        signal_hook::flag::register(signal, Arc::clone(&terminate)).map_err(|e| {
            AppError::internal(
                format!("cannot install the signal handler: {e}"),
                invocation,
            )
        })?;
    }
    Ok(terminate)
}

/// Accessible mode, reduced motion and the text target, from the flags, the
/// environment and the kernel command line.
struct Modes {
    accessible: Accessible,
    reduced_motion: bool,
    text: Option<TextTarget>,
}

/// Resolves [`Modes`] and reports accessible mode under `--verbose`.
fn modes(args: &SplashArgs, context: &Context, cmdline: Option<&str>, invocation: &str) -> Modes {
    let kernel = |key: &str| cmdline.and_then(|line| fairing_theme::kernel_value(line, key));
    let accessible = context.accessible.with_kernel(kernel(KERNEL_ACCESSIBLE));
    report_accessible(accessible, context, invocation);
    Modes {
        accessible,
        // Accessible mode has no animation (§18); the other two triggers are
        // reduced motion alone, for a graphical splash that keeps still.
        reduced_motion: accessible.on
            || args.reduced_motion
            || kernel(KERNEL_REDUCED_MOTION) == Some("1"),
        text: accessible
            .on
            .then(|| TextTarget::choose(&args.console, context.is_agent_environment())),
    }
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
        reason: Some("theme-unreadable"),
        ..Report::new(stage)
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
    report.agent = outcome.agent;
    report.answered = outcome.answered;
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

#[cfg(test)]
mod tests {
    use clap::Parser as _;
    use serde_json::Value;

    use super::*;
    use crate::cli::{Cli, Command};

    /// `fairing <words>`, parsed; the words must name `splash`.
    fn parse(words: &[&str]) -> (SplashArgs, GlobalFlags) {
        let cli = Cli::try_parse_from(std::iter::once("fairing").chain(words.iter().copied()))
            .unwrap_or_else(|e| panic!("{e}"));
        let Some(Command::Splash(args)) = cli.command else {
            panic!("not a splash invocation: {words:?}");
        };
        (args, cli.global)
    }

    #[test]
    fn the_command_line_is_read_whole() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let path = dir.path().join("cmdline");
        std::fs::write(&path, "quiet fairing.accessible=1\n").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            read_cmdline(&path).as_deref(),
            Some("quiet fairing.accessible=1\n")
        );
        assert_eq!(read_cmdline(&dir.path().join("missing")), None);
    }

    #[test]
    fn the_flag_alone_asks_for_reduced_motion() {
        // Verifies: FRN-SRS-052
        let (args, flags) = parse(&[
            "--no-accessible",
            "splash",
            "--stage",
            "system",
            "--reduced-motion",
        ]);
        let context = Context::resolve(&flags);
        let resolved = modes(&args, &context, Some("quiet"), "fairing splash");
        assert!(!resolved.accessible.on);
        assert!(resolved.reduced_motion);
        assert_eq!(resolved.text, None);
    }

    #[test]
    fn a_dry_run_reports_an_unreadable_theme_as_an_error() {
        let (args, flags) = parse(&[
            "--dry-run",
            "splash",
            "--stage",
            "system",
            "--backend",
            "memory",
            "--cmdline",
            "",
            "--theme",
            "/nonexistent/theme.fairing",
        ]);
        let context = Context::resolve(&flags);
        let planned = run(&args, &context, "fairing splash", &flags, Instant::now());
        assert!(planned.is_err(), "a plan does not step aside");
    }

    /// Set to the splash arguments of a dry run [`dry_run_child`] makes.
    const CHILD: &str = "FAIRING_TEST_DRY_RUN";

    /// Not a test on its own: runs `fairing --json --dry-run splash` with
    /// the arguments in [`CHILD`] when the parent test, [`dry_run`], starts
    /// this test binary again, so the report on stdout can be read.
    #[test]
    fn dry_run_child() {
        let Ok(extra) = std::env::var(CHILD) else {
            return;
        };
        let mut words = vec![
            "--json",
            "--dry-run",
            "splash",
            "--backend",
            "memory",
            "--cmdline",
            "",
        ];
        words.extend(extra.split_whitespace());
        let (args, flags) = parse(&words);
        let context = Context::resolve(&flags);
        run(&args, &context, "fairing splash", &flags, Instant::now())
            .unwrap_or_else(|e| panic!("{}", e.message));
    }

    /// The `data` of the dry-run report for the splash arguments `extra`.
    fn dry_run(extra: &str) -> Value {
        let exe = std::env::current_exe().unwrap_or_else(|e| panic!("{e}"));
        let out = std::process::Command::new(exe)
            .args([
                "--exact",
                "splash::tests::dry_run_child",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD, extra)
            .env_remove("SPACECRAFT_A11Y")
            .output()
            .unwrap_or_else(|e| panic!("{e}"));
        let stdout = String::from_utf8_lossy(&out.stdout);
        let report = stdout
            .lines()
            .find_map(|line| line.find("{\"").map(|at| &line[at..]))
            .unwrap_or_else(|| panic!("no report in {stdout:?}"));
        let value: Value = serde_json::from_str(report).unwrap_or_else(|e| panic!("{e}"));
        value["data"].clone()
    }

    #[test]
    fn a_dry_run_names_the_theme_and_whether_the_agent_will_run() {
        let system = dry_run("--stage system");
        assert_eq!(system["planned"], true, "{system}");
        assert_eq!(
            system["theme"].as_str(),
            Some(CompiledTheme::builtin().meta().name.as_str())
        );
        assert_eq!(system["agent"], true);
        assert_eq!(dry_run("--stage system --no-agent")["agent"], false);
        assert_eq!(
            dry_run("--stage shutdown")["agent"],
            false,
            "nothing asks at shutdown"
        );
    }
}
