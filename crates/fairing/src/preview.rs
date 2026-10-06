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
//!
//! On every error path while drawing, the output is closed — and the console
//! restored — before the result is reported. A panic is the one exception:
//! the release profile aborts without running destructors, so the compositor
//! and backends are written not to panic rather than relying on `Drop`.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fairing_render::{
    Attempt, Choice, Compositor, PixelFormat, Presenter, PresenterConfig, Scene, Size, Surface,
    open,
};
use fairing_theme::{Request, Resolution, Source};
use serde::Serialize;

use crate::cli::{BackendChoice, GlobalFlags, PreviewArgs};
use crate::diagnostic::{Diagnostic, Severity};
use crate::error::AppError;
use crate::output::envelope::Response;
use crate::output::mode::Context;
use crate::output::write_line;

/// The runnable hint for every "could not draw" failure.
const MEMORY_HINT: &str = "fairing preview --backend memory --snapshot frame.ppm";
/// The runnable hint for a snapshot path that cannot be created.
const SNAPSHOT_HINT: &str = "fairing preview --backend memory --snapshot ./frame.ppm";

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
    /// The backend that drew; `auto` only in a plan, where the chain decides at run time.
    backend: &'static str,
    /// The backends tried, in order.
    chain: Vec<&'static str>,
    device: Option<String>,
    /// Output size and format; unknown in a plan for a device backend.
    width: Option<u32>,
    height: Option<u32>,
    format: Option<&'static str>,
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

/// The snapshot target, opened before any device so a bad path fails fast
/// without taking the console.
///
/// The frame is written to a sibling `.part` file and renamed over `path`
/// only once it is complete, so a failed run never truncates or removes
/// whatever the operator already had at that path. A part file that never
/// received a complete frame is removed when the target is dropped.
struct SnapshotFile {
    path: PathBuf,
    part: PathBuf,
    file: Option<File>,
}

impl SnapshotFile {
    fn create(path: &Path, invocation: &str) -> Result<Self, AppError> {
        let part = part_path(path);
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&part)
            .map_err(|e| snapshot_error(&part, &e, invocation))?;
        Ok(Self {
            path: path.to_path_buf(),
            part,
            file: Some(file),
        })
    }

    /// Writes the presenter's last frame as a binary PPM, moves it into place and confirms it.
    fn write(
        mut self,
        presenter: &Presenter,
        context: &Context,
        invocation: &str,
    ) -> Result<String, AppError> {
        let failed = |what: &str, e: &io::Error| {
            AppError::internal(format!("{what} `{}`: {e}", self.path.display()), invocation)
        };
        let Some(mut file) = self.file.take() else {
            return Err(AppError::internal(
                format!("snapshot `{}` was already written", self.path.display()),
                invocation,
            ));
        };
        presenter
            .frame()
            .write_ppm(&mut file)
            .and_then(|()| file.sync_all())
            .map_err(|e| failed("writing", &e))?;
        drop(file);
        std::fs::rename(&self.part, &self.path).map_err(|e| failed("moving into place", &e))?;
        Diagnostic::new(
            Severity::Ok,
            "SNAPSHOT_WRITTEN",
            format!(
                "wrote `{}` ({} {})",
                self.path.display(),
                presenter.frame().size(),
                presenter.frame().format()
            ),
            invocation,
        )
        .emit(context);
        Ok(self.path.display().to_string())
    }
}

impl Drop for SnapshotFile {
    fn drop(&mut self) {
        if self.file.is_some() {
            // Nothing complete reached the part file; do not leave it behind. The
            // operator's own file at `path`, if any, was never touched.
            let _ = std::fs::remove_file(&self.part);
        }
    }
}

/// `frame.ppm` → `frame.ppm.part`, beside the target so the final rename stays on one filesystem.
fn part_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(std::ffi::OsString::from)
        .unwrap_or_default();
    name.push(".part");
    path.with_file_name(name)
}

/// Maps a snapshot creation failure onto the exit-code table by its cause.
fn snapshot_error(path: &Path, error: &io::Error, invocation: &str) -> AppError {
    let message = format!("cannot create `{}`: {error}", path.display());
    match error.kind() {
        io::ErrorKind::NotFound => AppError::not_found(message, SNAPSHOT_HINT, invocation),
        io::ErrorKind::PermissionDenied => {
            AppError::permission_denied(message, SNAPSHOT_HINT, invocation)
        }
        _ => AppError::invalid_argument(message, SNAPSHOT_HINT, invocation),
    }
}

/// Runs `fairing preview`.
///
/// # Errors
///
/// `FEATURE_UNAVAILABLE` for `--theme` until M2; `INVALID_ARGUMENT` for an
/// unregistered `--palette` or a device backend under an agent harness;
/// `NOT_FOUND`, `PERMISSION_DENIED` or `CONFLICT` when no backend could draw;
/// the backend's classified error if drawing fails midway.
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
        choice: choose_backend(args, context, invocation, flags.dry_run)?,
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

/// The backends a choice tries, by name.
fn chain_names(choice: Choice) -> Vec<&'static str> {
    choice.chain().iter().map(|kind| kind.as_str()).collect()
}

/// The report for `--dry-run`: what would be drawn, where, with what.
///
/// Only the memory backend has a size before it is opened; a device's mode is
/// read from the device, so the plan leaves it null rather than guess.
fn planned_report(plan: &Plan<'_>) -> Report {
    let memory = plan.choice == Choice::Memory;
    Report {
        backend: plan.choice.as_str(),
        chain: chain_names(plan.choice),
        device: None,
        width: memory.then(|| plan.memory_size.width()),
        height: memory.then(|| plan.memory_size.height()),
        format: memory.then_some(PixelFormat::Rgba8888.as_str()),
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
///
/// The snapshot file is created first so a bad path never costs a console
/// switch; the surface is closed on every path so the console comes back
/// before any error is reported.
fn draw(
    plan: &Plan<'_>,
    context: &Context,
    invocation: &str,
    origin: Instant,
) -> Result<Report, AppError> {
    let snapshot = match plan.args.snapshot.as_deref() {
        Some(path) => Some(SnapshotFile::create(path, invocation)?),
        None => None,
    };
    let opened = open(plan.choice, plan.memory_size)
        .map_err(|failure| AppError::from_no_backend(&failure, MEMORY_HINT, invocation))?;
    let fallbacks = report_fallbacks(&opened.attempts, context, invocation);
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

    let outcome = animate(&mut presenter, plan.args, invocation).and_then(|run_time| {
        let written = match snapshot {
            Some(file) => Some(file.write(&presenter, context, invocation)?),
            None => None,
        };
        Ok((run_time, written))
    });
    let closed = presenter
        .close()
        .map_err(|e| AppError::from_render(&e, MEMORY_HINT, invocation));
    let (run_time, snapshot) = outcome?;
    let stats = closed?;

    if let Some(first) = stats.first_frame() {
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
    let intervals = u32::try_from(stats.frames().saturating_sub(1)).unwrap_or(u32::MAX);
    let measured_fps = (run_time > Duration::ZERO && intervals > 0)
        .then(|| f64::from(intervals) / run_time.as_secs_f64());
    Ok(Report {
        backend: backend.as_str(),
        chain: chain_names(plan.choice),
        device,
        width: Some(size.width()),
        height: Some(size.height()),
        format: Some(format.as_str()),
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

/// Warns about each backend that failed before one worked and records it.
fn report_fallbacks(
    attempts: &[Attempt],
    context: &Context,
    invocation: &str,
) -> Vec<FallbackReport> {
    attempts
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
        .collect()
}

/// Presents the simulated bar for the requested duration; returns the wall time taken.
fn animate(
    presenter: &mut Presenter,
    args: &PreviewArgs,
    invocation: &str,
) -> Result<Duration, AppError> {
    let duration = Duration::from_secs_f64(args.seconds);
    let start = Instant::now();
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
        if done {
            return Ok(start.elapsed());
        }
        presenter.pace();
    }
}

/// Picks the palette: `--palette` is the explicit source; `SPACECRAFT_THEME`
/// follows; `NO_COLOR` overlays mono (Steelbore Standard §11.6).
fn resolve_palette(
    args: &PreviewArgs,
    context: &Context,
    invocation: &str,
) -> Result<Resolution, AppError> {
    let environment = std::env::var(fairing_theme::ENV_VAR).ok();
    let request = Request {
        explicit: args.palette.as_deref(),
        kernel_parameter: None,
        environment: environment.as_deref(),
        declared_default: None,
        no_color: context.no_color,
        accessible: false,
    };
    let resolution = fairing_theme::resolve(&request);
    if let Some(slug) = &args.palette
        && (slug.trim().is_empty()
            || resolution
                .skipped
                .iter()
                .any(|s| s.source == Source::Explicit))
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
///
/// A plan opens nothing, so under `--dry-run` an agent may inspect the device
/// chain it could not run; the plan carries a warning instead of a refusal.
fn choose_backend(
    args: &PreviewArgs,
    context: &Context,
    invocation: &str,
    dry_run: bool,
) -> Result<Choice, AppError> {
    if !context.is_agent_environment() {
        return Ok(args.backend.chain());
    }
    match args.backend {
        BackendChoice::Memory => Ok(Choice::Memory),
        BackendChoice::Drm | BackendChoice::Fbdev if dry_run => {
            Diagnostic::new(
                Severity::Warn,
                "AGENT_DEVICE_PLAN_ONLY",
                format!(
                    "agent environment detected; `--backend {}` is planned here but would be refused without --dry-run",
                    args.backend.chain()
                ),
                invocation,
            )
            .with_hint(MEMORY_HINT)
            .emit(context);
            Ok(args.backend.chain())
        }
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

/// The human `preview` line: where the frame went, or would go.
fn where_line(report: &Report) -> String {
    if report.planned && report.chain.len() > 1 {
        return format!("would try {}", report.chain.join(", then "));
    }
    let verb = if report.planned { "would draw" } else { "drew" };
    let geometry = match (report.width, report.height, report.format) {
        (Some(w), Some(h), Some(f)) => format!(" {w}x{h} {f}"),
        _ => String::new(),
    };
    let device = report
        .device
        .as_deref()
        .map(|d| format!(" ({d})"))
        .unwrap_or_default();
    format!("{verb} on {}{geometry}{device}", report.backend)
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
    write_line(&format!("preview\t{}", where_line(report)), invocation)?;
    write_line(
        &format!(
            "palette\t{} (base {}, from {}, overlay {})",
            report.palette.slug, report.palette.base, report.palette.source, report.palette.overlay
        ),
        invocation,
    )?;
    if !report.planned {
        write_line(
            &format!(
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
            ),
            invocation,
        )?;
    }
    if let Some(snapshot) = &report.snapshot {
        write_line(&format!("snapshot\t{snapshot}"), invocation)?;
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
        assert_eq!(chain_names(Choice::Auto), vec!["drm", "fbdev"]);
    }

    #[test]
    fn snapshot_errors_follow_the_cause() {
        let path = Path::new("/nowhere/frame.ppm");
        let missing = io::Error::from(io::ErrorKind::NotFound);
        assert_eq!(snapshot_error(path, &missing, "c").exit_code, 3);
        let refused = io::Error::from(io::ErrorKind::PermissionDenied);
        assert_eq!(snapshot_error(path, &refused, "c").exit_code, 4);
        let odd = io::Error::from(io::ErrorKind::IsADirectory);
        assert_eq!(snapshot_error(path, &odd, "c").exit_code, 2);
    }

    #[test]
    fn unwritten_snapshot_files_are_removed_and_existing_files_are_kept() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let path = dir.path().join("frame.ppm");
        std::fs::write(&path, b"PRECIOUS").unwrap_or_else(|e| panic!("{e}"));
        let target = SnapshotFile::create(&path, "c").unwrap_or_else(|e| panic!("{e}"));
        let part = part_path(&path);
        assert_eq!(part, dir.path().join("frame.ppm.part"));
        assert!(part.exists());
        drop(target);
        assert!(!part.exists(), "a part file must not survive a failed run");
        assert_eq!(
            std::fs::read(&path).unwrap_or_default(),
            b"PRECIOUS",
            "the operator's file is untouched by a failed run"
        );
        // A part file left by a crashed run is reported, never silently overwritten.
        std::fs::write(&part, b"").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            SnapshotFile::create(&path, "c").err().map(|e| e.exit_code),
            Some(2)
        );
    }
}
