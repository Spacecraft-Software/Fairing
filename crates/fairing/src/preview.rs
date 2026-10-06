// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! `fairing preview`: draw the splash with a simulated bar, then restore the console.
//!
//! Theme authors iterate without rebooting (FRN-SRS-083). The verb runs the
//! same compositor and backends the splash will use: DRM/KMS, then `/dev/fb0`,
//! or a memory frame for agents and snapshots. Under an agent or CI harness no
//! VT is ever opened (FRN-SRS-084): the automatic chain becomes the memory
//! backend with a warning, and an explicit device backend is a usage error.

use std::path::Path;
use std::time::{Duration, Instant};

use fairing_render::{Choice, Compositor, Presenter, PresenterConfig, Scene, Size, Surface, open};
use fairing_theme::{Request, Resolution, Source};
use serde::Serialize;

use crate::cli::{BackendChoice, GlobalFlags, PreviewArgs};
use crate::diagnostic::{Diagnostic, Severity};
use crate::error::AppError;
use crate::output::envelope::Response;
use crate::output::mode::Context;

/// The runnable hint for every "could not draw" failure.
const MEMORY_HINT: &str = "fairing preview --backend memory --snapshot frame.ppm";

#[derive(Debug, Serialize)]
struct PaletteReport {
    slug: &'static str,
    base: &'static str,
    source: &'static str,
    overlay: &'static str,
    skipped: Vec<SkippedReport>,
}

#[derive(Debug, Serialize)]
struct SkippedReport {
    source: &'static str,
    slug: String,
}

#[derive(Debug, Serialize)]
struct FallbackReport {
    backend: &'static str,
    error: String,
    elapsed_ms: f64,
}

/// What `preview` reports: the output it drew on and how the run went.
#[derive(Debug, Serialize)]
struct Report {
    backend: &'static str,
    device: Option<String>,
    width: u32,
    height: u32,
    format: &'static str,
    palette: PaletteReport,
    seconds: f64,
    fps: u32,
    frames: u64,
    dropped: u64,
    first_frame_ms: Option<f64>,
    measured_fps: Option<f64>,
    snapshot: Option<String>,
    fallbacks: Vec<FallbackReport>,
    /// True under `--dry-run`: nothing was opened or drawn.
    planned: bool,
}

/// Everything decided before a device is touched.
struct Plan<'a> {
    args: &'a PreviewArgs,
    resolution: Resolution,
    choice: Choice,
    memory_size: Size,
}

/// Runs `fairing preview`.
///
/// # Errors
///
/// `FEATURE_UNAVAILABLE` for `--theme` until M2; `INVALID_ARGUMENT` for an
/// unregistered `--palette` or a device backend under an agent harness;
/// `NOT_FOUND` / `PERMISSION_DENIED` when no backend could draw; the
/// backend's classified error if drawing fails midway.
pub fn run(
    args: &PreviewArgs,
    context: &Context,
    invocation: &str,
    flags: &GlobalFlags,
    origin: Instant,
) -> Result<(), AppError> {
    if args.theme.is_some() {
        return Err(AppError::feature_unavailable(
            "`--theme` previews a compiled theme, which arrives with the theme format (M2)",
            invocation,
        ));
    }
    let plan = Plan {
        args,
        resolution: resolve_palette(args, context, invocation)?,
        choice: choose_backend(args, context, invocation)?,
        memory_size: Size::new(args.size.0, args.size.1).map_err(|e| {
            AppError::invalid_argument(
                e.to_string(),
                "fairing preview --size 1920x1080",
                invocation,
            )
        })?,
    };
    Diagnostic::new(
        Severity::Info,
        "PALETTE_RESOLVED",
        format!(
            "palette `{}` from {} (base `{}`, overlay {})",
            plan.resolution.selection,
            plan.resolution.source,
            plan.resolution.base,
            plan.resolution.overlay
        ),
        invocation,
    )
    .emit(context);

    let report = if flags.dry_run {
        planned_report(&plan)
    } else {
        draw(&plan, context, invocation, origin)?
    };
    emit(&report, context, invocation, flags)
}

/// The report for `--dry-run`: what would be drawn, where, with what.
fn planned_report(plan: &Plan<'_>) -> Report {
    Report {
        backend: match plan.choice {
            Choice::Memory => "memory",
            Choice::Fbdev => "fbdev",
            Choice::Auto | Choice::Drm => "drm",
        },
        device: None,
        width: plan.memory_size.width(),
        height: plan.memory_size.height(),
        format: if plan.choice == Choice::Memory {
            "rgba8888"
        } else {
            "xrgb8888"
        },
        palette: palette_report(&plan.resolution),
        seconds: plan.args.seconds,
        fps: plan.args.fps,
        frames: 0,
        dropped: 0,
        first_frame_ms: None,
        measured_fps: None,
        snapshot: plan.args.snapshot.as_ref().map(|p| p.display().to_string()),
        fallbacks: Vec::new(),
        planned: true,
    }
}

/// Opens the chain, draws the simulated run, writes the snapshot and closes.
fn draw(
    plan: &Plan<'_>,
    context: &Context,
    invocation: &str,
    origin: Instant,
) -> Result<Report, AppError> {
    let opened = open(plan.choice, plan.memory_size)
        .map_err(|failure| AppError::from_no_backend(&failure, MEMORY_HINT, invocation))?;
    let fallbacks: Vec<FallbackReport> = opened
        .attempts
        .iter()
        .map(|attempt| {
            Diagnostic::new(
                Severity::Warn,
                "BACKEND_FALLBACK",
                format!("{}; trying the next backend", attempt.error),
                invocation,
            )
            .with_hint(MEMORY_HINT)
            .emit(context);
            FallbackReport {
                backend: attempt.backend.as_str(),
                error: attempt.error.to_string(),
                elapsed_ms: attempt.elapsed.as_secs_f64() * 1000.0,
            }
        })
        .collect();
    let device = device_of(&opened.surface);

    let compositor = Compositor::new(plan.resolution.selection)
        .map_err(|e| AppError::from_render(&e, MEMORY_HINT, invocation))?;
    let config = PresenterConfig {
        hz: plan.args.fps,
        origin,
        choice: plan.choice,
        ..PresenterConfig::default()
    };
    let mut presenter = Presenter::new(opened.surface, compositor, config)
        .map_err(|e| AppError::from_render(&e, MEMORY_HINT, invocation))?;
    let size = presenter.frame().size();
    let format = presenter.frame().format();
    let backend = presenter.backend();

    let run_time = animate(&mut presenter, plan.args, context, invocation)?;
    let snapshot = match &plan.args.snapshot {
        Some(path) => Some(write_snapshot(&presenter, path, context, invocation)?),
        None => None,
    };
    let stats = presenter
        .close()
        .map_err(|e| AppError::from_render(&e, MEMORY_HINT, invocation))?;
    let intervals = u32::try_from(stats.frames().saturating_sub(1)).unwrap_or(u32::MAX);
    let measured_fps = (run_time > Duration::ZERO && intervals > 0)
        .then(|| f64::from(intervals) / run_time.as_secs_f64());
    Ok(Report {
        backend: backend.as_str(),
        device,
        width: size.width(),
        height: size.height(),
        format: format.as_str(),
        palette: palette_report(&plan.resolution),
        seconds: plan.args.seconds,
        fps: plan.args.fps,
        frames: stats.frames(),
        dropped: stats.dropped(),
        first_frame_ms: stats.first_frame().map(|d| d.as_secs_f64() * 1000.0),
        measured_fps,
        snapshot,
        fallbacks,
        planned: false,
    })
}

/// Presents the simulated bar for the requested duration; returns the wall time taken.
fn animate(
    presenter: &mut Presenter,
    args: &PreviewArgs,
    context: &Context,
    invocation: &str,
) -> Result<Duration, AppError> {
    let duration = Duration::from_secs_f64(args.seconds);
    let start = Instant::now();
    let mut first_frame_logged = false;
    loop {
        let elapsed = start.elapsed();
        let done = elapsed >= duration;
        let percent = if done {
            100
        } else {
            simulated_percent(elapsed, duration)
        };
        presenter
            .present(&Scene::new(percent).with_status(args.status.as_str()))
            .map_err(|e| AppError::from_render(&e, MEMORY_HINT, invocation))?;
        if !first_frame_logged && let Some(first) = presenter.stats().first_frame() {
            first_frame_logged = true;
            Diagnostic::new(
                Severity::Info,
                "FIRST_FRAME",
                format!(
                    "first frame presented {:.1} ms after process start",
                    first.as_secs_f64() * 1000.0
                ),
                invocation,
            )
            .emit(context);
        }
        if done {
            return Ok(start.elapsed());
        }
        presenter.pace();
    }
}

/// Writes the last frame as a binary PPM and confirms it.
fn write_snapshot(
    presenter: &Presenter,
    path: &Path,
    context: &Context,
    invocation: &str,
) -> Result<String, AppError> {
    let mut file = std::fs::File::create(path).map_err(|e| {
        AppError::invalid_argument(
            format!("cannot create `{}`: {e}", path.display()),
            "fairing preview --backend memory --snapshot ./frame.ppm",
            invocation,
        )
    })?;
    presenter.frame().write_ppm(&mut file).map_err(|e| {
        AppError::internal(format!("writing `{}`: {e}", path.display()), invocation)
    })?;
    Diagnostic::new(
        Severity::Ok,
        "SNAPSHOT_WRITTEN",
        format!(
            "wrote `{}` ({} {})",
            path.display(),
            presenter.frame().size(),
            presenter.frame().format()
        ),
        invocation,
    )
    .emit(context);
    Ok(path.display().to_string())
}

/// Picks the palette: `--palette` outranks `SPACECRAFT_THEME`; `NO_COLOR` overlays mono.
fn resolve_palette(
    args: &PreviewArgs,
    context: &Context,
    invocation: &str,
) -> Result<Resolution, AppError> {
    let environment = std::env::var(fairing_theme::ENV_VAR).ok();
    let request = Request {
        kernel_parameter: args.palette.as_deref(),
        environment: environment.as_deref(),
        declared_default: None,
        no_color: context.no_color,
        accessible: false,
    };
    let resolution = fairing_theme::resolve(&request);
    if let Some(slug) = &args.palette
        && resolution
            .skipped
            .iter()
            .any(|s| s.source == Source::KernelParameter)
    {
        return Err(AppError::invalid_argument(
            format!("palette `{slug}` is not a registered theme"),
            format!("fairing preview --palette {}", fairing_theme::DEFAULT_SLUG),
            invocation,
        ));
    }
    Ok(resolution)
}

/// Applies the agent rule (FRN-SRS-084) to the requested backend.
fn choose_backend(
    args: &PreviewArgs,
    context: &Context,
    invocation: &str,
) -> Result<Choice, AppError> {
    if !context.is_agent_environment() {
        return Ok(args.backend.chain());
    }
    match args.backend {
        BackendChoice::Memory => Ok(Choice::Memory),
        BackendChoice::Auto => {
            Diagnostic::new(
                Severity::Warn,
                "AGENT_MEMORY_BACKEND",
                "agent environment detected; rendering to memory instead of a VT",
                invocation,
            )
            .with_hint(MEMORY_HINT)
            .emit(context);
            Ok(Choice::Memory)
        }
        BackendChoice::Drm | BackendChoice::Fbdev => Err(AppError::invalid_argument(
            format!(
                "`--backend {}` opens a VT, which an agent environment must not",
                args.backend.chain()
            ),
            MEMORY_HINT,
            invocation,
        )),
    }
}

/// The bar position for `elapsed` out of `duration`, in whole percent below 100.
fn simulated_percent(elapsed: Duration, duration: Duration) -> u8 {
    if duration.is_zero() {
        return 100;
    }
    let fraction = (elapsed.as_secs_f64() / duration.as_secs_f64()).clamp(0.0, 0.99);
    whole_percent(fraction * 100.0)
}

/// A clamped `0.0..=99.0` value as whole percent.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is clamped to 0.0..=99.0 before the cast"
)]
fn whole_percent(value: f64) -> u8 {
    value.clamp(0.0, 99.0).floor() as u8
}

fn device_of(surface: &Surface) -> Option<String> {
    match surface {
        Surface::Drm(drm) => Some(drm.path().display().to_string()),
        Surface::Fbdev(fb) => Some(fb.path().display().to_string()),
        Surface::Memory(_) => None,
    }
}

fn palette_report(resolution: &Resolution) -> PaletteReport {
    PaletteReport {
        slug: resolution.selection.slug(),
        base: resolution.base.slug,
        source: resolution.source.as_str(),
        overlay: resolution.overlay.as_str(),
        skipped: resolution
            .skipped
            .iter()
            .map(|s| SkippedReport {
                source: s.source.as_str(),
                slug: s.slug.clone(),
            })
            .collect(),
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
    let verb = if report.planned { "would draw" } else { "drew" };
    println!(
        "preview\t{verb} on {} {}x{} {}{}",
        report.backend,
        report.width,
        report.height,
        report.format,
        report
            .device
            .as_deref()
            .map(|d| format!(" ({d})"))
            .unwrap_or_default()
    );
    println!(
        "palette\t{} (base {}, from {}, overlay {})",
        report.palette.slug, report.palette.base, report.palette.source, report.palette.overlay
    );
    if !report.planned {
        println!(
            "frames\t{} in {:.1} s at {} fps requested{}{}",
            report.frames,
            report.seconds,
            report.fps,
            report
                .measured_fps
                .map(|f| format!(", {f:.1} measured"))
                .unwrap_or_default(),
            report
                .first_frame_ms
                .map(|ms| format!(", first frame {ms:.1} ms"))
                .unwrap_or_default()
        );
    }
    if let Some(snapshot) = &report.snapshot {
        println!("snapshot\t{snapshot}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use fairing_render::BackendKind;

    use super::*;

    #[test]
    fn simulated_bar_never_reaches_100_before_the_end() {
        let total = Duration::from_secs(10);
        assert_eq!(simulated_percent(Duration::ZERO, total), 0);
        assert_eq!(simulated_percent(Duration::from_secs(5), total), 50);
        assert_eq!(simulated_percent(Duration::from_secs(10), total), 99);
        assert_eq!(simulated_percent(Duration::from_secs(99), total), 99);
        assert_eq!(simulated_percent(Duration::ZERO, Duration::ZERO), 100);
        assert_eq!(whole_percent(-3.0), 0);
        assert_eq!(whole_percent(250.0), 99);
    }

    #[test]
    fn backend_names_are_the_render_crate_names() {
        for (choice, name) in [
            (BackendChoice::Auto, "auto"),
            (BackendChoice::Drm, "drm"),
            (BackendChoice::Fbdev, "fbdev"),
            (BackendChoice::Memory, "memory"),
        ] {
            assert_eq!(choice.chain().as_str(), name);
        }
        assert_eq!(BackendKind::Memory.as_str(), "memory");
    }
}
