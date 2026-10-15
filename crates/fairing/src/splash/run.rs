// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! The splash loop: one thread, one frame per tick, nothing that blocks.
//!
//! Each tick reads the boot clock, drains the systemd channel, looks for the
//! cache file, moves the bar toward the progress model's target and presents
//! one frame; then it waits for the next 30 Hz deadline. Every input is a
//! non-blocking read, so the loop notices SIGTERM within one tick and a failed
//! unit within one D-Bus poll, and the only sleep is the frame pacing
//! (FRN-SRS-033, FRN-SRS-034, FRN-SRS-037).
//!
//! The password agent is one more input: each tick it drains the question
//! directory and the keyboards, and the open question is drawn into the same
//! frame (FRN-SRS-020, FRN-SRS-021). While it is up, `/run/fairing/agent`
//! exists, and systemd's console agent stands aside for as long as it does.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use fairing_askpass::agent::{Activity, Agent};
use fairing_askpass::input::Keyboards;
use fairing_askpass::watch::monotonic_us;
use fairing_render::{
    Cadence, Choice, Compositor, EchoText, Field, NoBackend, Opened, Presenter, PresenterConfig,
    PromptView, RenderErrorKind, Scene, Size, TextConsole, open,
};
use fairing_theme::{CompiledTheme, Selection};
use serde::Serialize;

use crate::diagnostic::{Diagnostic, Priority, Severity};
use crate::splash::clock::BootClock;
use crate::splash::console::{Sink, TextTarget};
use crate::splash::events::{Event, Snapshot};
use crate::splash::notify::Notifier;
use crate::splash::progress::{Bar, Inputs, Stage, target};
use crate::splash::state::{
    DURATIONS_FILE, Durations, HANDOFF_FILE, HANDOFF_MARKER, Handoff, read_durations, read_handoff,
    write_durations, write_handoff,
};

/// How long stage 2 waits for D-Bus before driving the bar from the cache
/// (FRN-SRS-016).
pub const DBUS_DEADLINE: Duration = Duration::from_secs(2);

/// How often a missing cache file is looked for again.
///
/// In the initrd the file appears when the root is mounted at `/sysroot`; in
/// stage 2 when `/var` is. A stat a second costs nothing measurable.
pub const CACHE_RETRY: Duration = Duration::from_secs(1);

/// Frames per second while booting (FRN-SRS-005).
pub const SPLASH_HZ: u32 = 30;

/// How long the display may stay mid-flip before the splash gives up on it.
///
/// A flip that has not completed by the next frame stays in flight, and the
/// frames presented until it lands are dropped: a busy boot can hold the
/// kernel's commit work that long. A display that has not completed a flip for
/// this long has failed. The bound sits inside the 5 s of FRN-SRS-037.
pub const FLIP_STALL_LIMIT: Duration = Duration::from_secs(2);

/// How long a password prompt may go undrawn because the output is lost
/// before the splash steps aside, so the question is not answered blind:
/// longer than a simpledrm-to-native swap, short of a user typing.
pub const BLIND_PROMPT_LIMIT: Duration = Duration::from_secs(2);

/// The file in the runtime directory that exists while the password agent
/// is up; systemd's console agent is conditioned on its absence.
pub const AGENT_FLAG: &str = "agent";

/// What the prompt says when a question repeats one just answered
/// (FRN-SRS-029).
pub const REJECTED_NOTICE: &str = "Not accepted. Please try again.";

/// Where the password agent finds questions and keyboards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentPaths {
    /// `/run/systemd/ask-password`.
    pub ask_dir: PathBuf,
    /// `/sys/class/input`.
    pub input_sysfs: PathBuf,
    /// `/dev/input`.
    pub input_dev: PathBuf,
}

/// Where the splash reads and writes its files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// `/run/fairing`: the switch-root handoff.
    pub runtime: PathBuf,
    /// `/var/lib/fairing`: the duration cache, as stage 2 sees it.
    pub state: PathBuf,
    /// `/sysroot/var/lib/fairing`: the duration cache, as the initrd sees it.
    pub sysroot_state: PathBuf,
}

/// Everything decided before the loop starts.
#[derive(Debug)]
pub struct Plan {
    /// Which instance this is.
    pub stage: Stage,
    /// Backends to try.
    pub choice: Choice,
    /// Size of the memory backend.
    pub memory_size: Size,
    /// The theme to draw.
    pub theme: CompiledTheme,
    /// The palette §11.6 selected.
    pub selection: Selection,
    /// Files.
    pub paths: Paths,
    /// Behave as if SIGTERM arrived after this long (tests and demos).
    pub exit_after: Option<Duration>,
    /// The password agent; `None` under `--no-agent`.
    pub agent: Option<AgentPaths>,
    /// Accessible mode: lines of text here instead of graphics (FRN-SRS-070).
    pub text: Option<TextTarget>,
    /// Reduced motion: the bar jumps to its target and frame sequences show
    /// their first frame (FRN-SRS-052).
    pub reduced_motion: bool,
}

/// Why the loop ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Reason {
    /// SIGTERM in stage 2 after greetd's start left the handoff marker: greetd
    /// is taking the screen (FRN-SRS-017, FRN-SRS-032).
    Handoff,
    /// SIGTERM in stage 2 without the handoff marker and with no shutdown in
    /// view, such as a password prompt taking the console: no 100 % frame and
    /// no measurement.
    Stopped,
    /// SIGTERM in the initrd: systemd is switching root.
    SwitchRoot,
    /// SIGTERM while the system was stopping.
    Shutdown,
    /// A unit failed (FRN-SRS-034).
    FailedUnit,
    /// Rescue or emergency mode started (FRN-SRS-034).
    Maintenance,
    /// The boot finished with no greetd handoff (booted to a target without
    /// greetd): the splash leaves rather than hold the screen for ever
    /// (FRN-SRS-037).
    BootFinished,
    /// No backend could draw (FRN-SRS-003).
    NoBackend,
    /// Presenting failed for a reason other than a lost output.
    RenderFailed,
    /// `--exit-after` elapsed.
    ExitAfter,
    /// The password agent could not start or lost its question directory:
    /// the splash leaves the questions, and the console, to systemd's
    /// console agent (FRN-SRS-027).
    AgentFailed,
}

impl Reason {
    /// Stable kebab-case name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Handoff => "handoff",
            Self::Stopped => "stopped",
            Self::SwitchRoot => "switch-root",
            Self::Shutdown => "shutdown",
            Self::FailedUnit => "failed-unit",
            Self::Maintenance => "maintenance",
            Self::BootFinished => "boot-finished",
            Self::NoBackend => "no-backend",
            Self::RenderFailed => "render-failed",
            Self::ExitAfter => "exit-after",
            Self::AgentFailed => "agent-failed",
        }
    }
}

/// How the run went.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Outcome {
    /// Why the loop ended.
    pub reason: Reason,
    /// The backend that drew, if any.
    pub backend: Option<&'static str>,
    /// Frames presented.
    pub frames: u64,
    /// Frames the cadence skipped.
    pub dropped: u64,
    /// Process start to first frame.
    pub first_frame_ms: Option<f64>,
    /// The bar's last value.
    pub bar: f32,
    /// Whether D-Bus delivered at least one reading.
    pub dbus: bool,
    /// The bar value the initrd handed over, if any (FRN-SRS-014).
    pub carried_bar: Option<f32>,
    /// The bar value of the first frame presented.
    pub first_bar: Option<f32>,
    /// The last status line drawn (FRN-SRS-050).
    pub last_status: Option<String>,
    /// From the tick that first saw SIGTERM to the output released
    /// (FRN-SRS-033); `None` when the run did not end on SIGTERM. The signal
    /// can arrive up to one frame period before a tick sees it.
    pub release_ms: Option<f64>,
    /// Whether the password agent was up.
    pub agent: bool,
    /// Questions answered.
    pub answered: u32,
}

/// What the loop needs from the process.
pub struct Environment<'a> {
    /// Boot time.
    pub clock: &'a dyn BootClock,
    /// Set by the SIGTERM/SIGINT handler.
    pub terminate: &'a AtomicBool,
    /// `NOTIFY_SOCKET`.
    pub notifier: &'a Notifier,
    /// The D-Bus thread's messages; `None` in the initrd and under `--bus none`.
    pub systemd: Option<Receiver<Event>>,
    /// Process start, for first-frame latency (FRN-SRS-004).
    pub origin: Instant,
    /// Writes a diagnostic (the caller's context and invocation).
    pub emit: &'a dyn Fn(Diagnostic),
    /// The invocation, for diagnostics.
    pub invocation: &'a str,
}

/// Opens the output and runs until a reason to stop.
///
/// A splash never fails a boot: every path ends in `Ok` with the reason, the
/// console restored and `READY=1` sent, so a `Type=notify` unit always
/// starts (FRN-SRS-003, FRN-SRS-036).
///
/// Implements: FRN-SRS-003, FRN-SRS-014, FRN-SRS-015, FRN-SRS-033, FRN-SRS-034, FRN-SRS-036
pub fn run(plan: &Plan, env: &Environment<'_>) -> Outcome {
    run_with(plan, env, open)
}

/// [`run`] with an injectable chain opener (tests use the memory backend).
pub fn run_with(
    plan: &Plan,
    env: &Environment<'_>,
    open_chain: fn(Choice, Size) -> Result<Opened, NoBackend>,
) -> Outcome {
    let started = env.clock.now();
    // An initrd that "ended" after this process started is a stale or
    // damaged file, not a handoff.
    let carried = match plan.stage {
        Stage::System => read_handoff(&plan.paths.runtime.join(HANDOFF_FILE))
            .filter(|handoff| handoff.initrd_end <= started),
        Stage::Initrd => None,
    };
    // Stage 2 counts from its own start, not from the handoff's initrd end: an
    // initrd splash stopped by a passphrase prompt left long before switch-root,
    // and the typing time is no part of stage 2.
    let timeline = Timeline {
        started,
        stage_origin: match plan.stage {
            Stage::Initrd => Duration::ZERO,
            Stage::System => started,
        },
        carried,
    };
    let mut outcome = Outcome {
        reason: Reason::NoBackend,
        backend: None,
        frames: 0,
        dropped: 0,
        first_frame_ms: None,
        bar: carried.map_or(0.0, |h| h.bar),
        dbus: false,
        carried_bar: carried.map(|h| h.bar),
        first_bar: None,
        last_status: None,
        release_ms: None,
        agent: false,
        answered: 0,
    };
    let mut output = match start(plan, env, open_chain) {
        Ok(output) => output,
        Err(reason) => {
            outcome.reason = reason;
            notify(env, Notifier::ready, "READY=1");
            return outcome;
        }
    };
    outcome.backend = Some(output.kind());

    let mut session = Session::new(outcome.bar);
    if let Some(paths) = &plan.agent {
        match Agent::open(&paths.ask_dir, &paths.input_sysfs, &paths.input_dev) {
            Ok(agent) => {
                session.agent = Some(agent);
                outcome.agent = true;
                raise_flag(plan, env);
            }
            Err(error) => {
                (env.emit)(
                    Diagnostic::new(
                        Severity::Warn,
                        "AGENT_FAILED",
                        format!(
                            "the password agent cannot start ({error}); leaving questions to the console"
                        ),
                        env.invocation,
                    )
                    .with_priority(Priority::Notice),
                );
                outcome.reason = Reason::AgentFailed;
                return finish(plan, env, output, &mut session, &timeline, outcome);
            }
        }
    }
    outcome.reason = loop {
        if let Some(reason) = session.tick(plan, env, &mut output, &timeline, &mut outcome) {
            break reason;
        }
        output.pace();
    };
    finish(plan, env, output, &mut session, &timeline, outcome)
}

/// Where the splash draws: pixels through the presenter, or lines of text on
/// the console in accessible mode.
#[expect(
    clippy::large_enum_variant,
    reason = "one per run, owned by the loop; the pixel case is the usual one"
)]
enum Output {
    /// DRM/KMS, fbdev or memory.
    Pixels(Presenter),
    /// The text backend (FRN-SRS-070).
    Text(TextOutput),
}

/// The text backend and the loop's own pacing, which the presenter keeps for
/// the pixel backends.
struct TextOutput {
    console: TextConsole<Sink>,
    cadence: Cadence,
    origin: Instant,
    first: Option<Duration>,
    frames: u64,
}

impl Output {
    /// The backend's name for the report.
    fn kind(&self) -> &'static str {
        match self {
            Self::Pixels(presenter) => presenter.backend().as_str(),
            Self::Text(_) => "text",
        }
    }

    /// Process start to the first frame or line.
    fn first_frame(&self) -> Option<Duration> {
        match self {
            Self::Pixels(presenter) => presenter.stats().first_frame(),
            Self::Text(text) => text.first,
        }
    }

    /// Waits for the next frame deadline.
    fn pace(&mut self) {
        match self {
            Self::Pixels(presenter) => {
                presenter.pace();
            }
            Self::Text(text) => {
                text.cadence.wait();
            }
        }
    }

    /// Releases the output; frames presented and dropped.
    fn close(self) -> Result<(u64, u64), String> {
        match self {
            Self::Pixels(presenter) => presenter
                .close()
                .map(|stats| (stats.frames(), stats.dropped()))
                .map_err(|error| error.to_string()),
            Self::Text(text) => Ok((text.frames, text.cadence.dropped())),
        }
    }
}

/// How far the theme's frame sequence has played: since this process
/// started, or not at all under reduced motion, which keeps the first frame
/// (FRN-SRS-052).
///
/// Implements: FRN-SRS-052
pub fn animation_time(reduced_motion: bool, running: Duration) -> Duration {
    if reduced_motion {
        Duration::ZERO
    } else {
        running
    }
}

/// Marks the agent up, so systemd's console agent stands aside.
fn raise_flag(plan: &Plan, env: &Environment<'_>) {
    if let Err(error) = std::fs::write(plan.paths.runtime.join(AGENT_FLAG), b"") {
        (env.emit)(Diagnostic::new(
            Severity::Warn,
            "AGENT_FLAG",
            format!(
                "cannot mark the password agent up ({error}); systemd's console agent may also ask"
            ),
            env.invocation,
        ));
    }
}

/// The fixed points of a run on the boot clock.
#[derive(Debug, Clone, Copy)]
struct Timeline {
    /// When this process first read the clock.
    started: Duration,
    /// Where this stage's elapsed time counts from: zero (boot) in the initrd,
    /// this process's start in stage 2.
    stage_origin: Duration,
    /// The initrd's handoff, read once at start.
    carried: Option<Handoff>,
}

/// Opens the console in accessible mode, or else the first backend that
/// works and builds the presenter on it.
fn start(
    plan: &Plan,
    env: &Environment<'_>,
    open_chain: fn(Choice, Size) -> Result<Opened, NoBackend>,
) -> Result<Output, Reason> {
    if let Some(target) = &plan.text {
        return match Sink::open(target) {
            Ok(sink) => Ok(Output::Text(TextOutput {
                console: TextConsole::new(sink),
                cadence: Cadence::new(SPLASH_HZ),
                origin: env.origin,
                first: None,
                frames: 0,
            })),
            Err(error) => {
                (env.emit)(
                    Diagnostic::new(
                        Severity::Warn,
                        "NO_BACKEND",
                        format!(
                            "accessible mode cannot write to the console ({error}); the console stays as it is"
                        ),
                        env.invocation,
                    )
                    .with_priority(Priority::Notice),
                );
                Err(Reason::NoBackend)
            }
        };
    }
    let opened = open_chain(plan.choice, plan.memory_size).map_err(|failure| {
        (env.emit)(
            Diagnostic::new(
                Severity::Warn,
                "NO_BACKEND",
                format!(
                    "no backend could draw ({}); the console stays as it is",
                    failure.names()
                ),
                env.invocation,
            )
            .with_priority(Priority::Notice),
        );
        Reason::NoBackend
    })?;
    for attempt in &opened.attempts {
        (env.emit)(Diagnostic::new(
            Severity::Warn,
            "BACKEND_FALLBACK",
            format!("{}; trying the next backend", attempt.error),
            env.invocation,
        ));
    }
    Compositor::with_theme(plan.selection, &plan.theme)
        .and_then(|compositor| {
            Presenter::new(
                opened.surface,
                compositor,
                PresenterConfig {
                    hz: SPLASH_HZ,
                    origin: env.origin,
                    choice: plan.choice,
                    // One re-open attempt per frame, never a wait inside one:
                    // a lost output must not hold up SIGTERM (FRN-SRS-033).
                    reacquire_budget: Duration::ZERO,
                    ..PresenterConfig::default()
                },
            )
        })
        .map(Output::Pixels)
        .map_err(|error| {
            (env.emit)(Diagnostic::new(
                Severity::Warn,
                "RENDER_FAILED",
                format!("cannot start drawing: {error}"),
                env.invocation,
            ));
            Reason::RenderFailed
        })
}

/// Releases the output, writes what outlives the process and says goodbye.
fn finish(
    plan: &Plan,
    env: &Environment<'_>,
    output: Output,
    session: &mut Session,
    timeline: &Timeline,
    mut outcome: Outcome,
) -> Outcome {
    // The keyboards first: releasing a grab is one ioctl, and a question still
    // open goes back to the console agent with them (FRN-SRS-028).
    if let Some(agent) = session.agent.as_mut() {
        agent.close();
        let _gone = std::fs::remove_file(plan.paths.runtime.join(AGENT_FLAG));
    }
    outcome.bar = session.bar.shown();
    outcome.dbus = session.latest.is_some();
    // Release the output first: greetd is waiting for it (FRN-SRS-033).
    let closed = output.close();
    outcome.release_ms = session
        .terminate_seen
        .map(|seen| seen.elapsed().as_secs_f64() * 1000.0);
    match closed {
        Ok((frames, dropped)) => {
            outcome.frames = frames;
            outcome.dropped = dropped;
        }
        Err(error) => (env.emit)(Diagnostic::new(
            Severity::Warn,
            "RELEASE_FAILED",
            format!("releasing the output: {error}"),
            env.invocation,
        )),
    }
    persist(
        plan,
        env,
        &outcome,
        timeline,
        session.durations,
        env.clock.now(),
    );
    // A run that never presented a frame still tells systemd it started, or a
    // `Type=notify` unit that exits 0 would be failed with result 'protocol'.
    if !session.ready_sent {
        notify(env, Notifier::ready, "READY=1");
    }
    notify(env, Notifier::stopping, "STOPPING=1");
    outcome
}

/// Frames dropped in a row because the display had not completed its last
/// flip, timed on the boot clock from the first of them.
#[derive(Debug, Default)]
struct Stall {
    since: Option<Duration>,
}

impl Stall {
    /// A frame was presented, so the last flip landed: any stall is over.
    fn clear(&mut self) {
        self.since = None;
    }

    /// A frame was dropped at `now`. `true` once frames have been dropped for
    /// longer than [`FLIP_STALL_LIMIT`].
    fn late(&mut self, now: Duration) -> bool {
        let since = *self.since.get_or_insert(now);
        now.saturating_sub(since) > FLIP_STALL_LIMIT
    }
}

/// Mutable loop state.
struct Session {
    bar: Bar,
    durations: Option<Durations>,
    next_cache_look: Duration,
    latest: Option<Snapshot>,
    connected: bool,
    dbus_notice_sent: bool,
    lost_output_reported: bool,
    /// When the output was last found lost, while it still is.
    lost_since: Option<Duration>,
    late_flip_reported: bool,
    stall: Stall,
    ready_sent: bool,
    /// When the loop first saw the terminate flag.
    terminate_seen: Option<Instant>,
    /// Failed units at the first reading. Units that failed before the splash
    /// could see them (the initrd's, or any before the bus came up) are not a
    /// unit entering the failed state now (FRN-SRS-034).
    failed_baseline: Option<u32>,
    /// The password agent, while it is up.
    agent: Option<Agent<Keyboards>>,
}

impl Session {
    fn new(bar: f32) -> Self {
        Self {
            bar: Bar::new(bar),
            durations: None,
            next_cache_look: Duration::ZERO,
            latest: None,
            connected: false,
            dbus_notice_sent: false,
            lost_output_reported: false,
            lost_since: None,
            late_flip_reported: false,
            stall: Stall::default(),
            ready_sent: false,
            terminate_seen: None,
            failed_baseline: None,
            agent: None,
        }
    }

    /// One frame: read every input, draw, and decide whether to stop.
    fn tick(
        &mut self,
        plan: &Plan,
        env: &Environment<'_>,
        output: &mut Output,
        timeline: &Timeline,
        outcome: &mut Outcome,
    ) -> Option<Reason> {
        let now = env.clock.now();
        let running = now.saturating_sub(timeline.started);
        self.look_for_cache(plan, now);
        if let Some(reason) = self.drain(env) {
            return Some(reason);
        }
        if plan.stage == Stage::System {
            self.check_dbus_deadline(env, running);
        }
        let exit_after = plan.exit_after.is_some_and(|after| running >= after);
        let terminating = env.terminate.load(Ordering::SeqCst) || exit_after;
        if terminating && self.terminate_seen.is_none() {
            self.terminate_seen = Some(Instant::now());
        }
        let stopping = self.latest.as_ref().is_some_and(Snapshot::is_stopping);
        // Only greetd's start leaves the marker; `--exit-after` stands in for it.
        let handed_over = plan.stage == Stage::System
            && terminating
            && !stopping
            && (exit_after || plan.paths.runtime.join(HANDOFF_MARKER).exists());
        let inputs = Inputs {
            elapsed: now.saturating_sub(timeline.stage_origin),
            cached: self.durations.and_then(|d| match plan.stage {
                Stage::Initrd => d.initrd,
                Stage::System => d.stage2,
            }),
            systemd_progress: self
                .latest
                .as_ref()
                .filter(|_| self.connected)
                .map(|s| s.progress),
            handoff: handed_over,
        };
        let shown = self.bar.advance(
            target(plan.stage, &inputs),
            now,
            terminating || plan.reduced_motion,
        );
        let prompt = match self.ask(env, outcome) {
            Ok(prompt) => prompt,
            Err(reason) => return Some(reason),
        };
        let scene = Scene::from_fraction(shown)
            .with_time(animation_time(plan.reduced_motion, running))
            .with_status(
                self.latest
                    .as_ref()
                    .and_then(|s| s.job.clone())
                    .unwrap_or_default(),
            )
            .with_prompt(prompt);
        if let Err(reason) = self.present(env, output, &scene, outcome, now) {
            return Some(reason);
        }
        // Ready after the first attempt: after the first frame, or at once when
        // the output is lost mid-swap, since a splash that waits for the native
        // driver is still running and must not be killed for a late READY=1
        // (FRN-SRS-036).
        if !self.ready_sent {
            notify(env, Notifier::ready, "READY=1");
            self.ready_sent = true;
        }
        if outcome.first_frame_ms.is_none()
            && let Some(first) = output.first_frame()
        {
            outcome.first_frame_ms = Some(first.as_secs_f64() * 1000.0);
            (env.emit)(Diagnostic::new(
                Severity::Info,
                "FIRST_FRAME",
                format!(
                    "first frame presented {:.1} ms after process start",
                    first.as_secs_f64() * 1000.0
                ),
                env.invocation,
            ));
        }
        terminating.then_some(match (exit_after, plan.stage, stopping, handed_over) {
            (true, _, _, _) => Reason::ExitAfter,
            (false, Stage::Initrd, _, _) => Reason::SwitchRoot,
            (false, Stage::System, true, _) => Reason::Shutdown,
            (false, Stage::System, false, true) => Reason::Handoff,
            (false, Stage::System, false, false) => Reason::Stopped,
        })
    }

    /// One agent tick: the open question as the frame draws it. An agent that
    /// can no longer read its directory ends the run, so the console agent
    /// takes the questions (FRN-SRS-027).
    fn ask(
        &mut self,
        env: &Environment<'_>,
        outcome: &mut Outcome,
    ) -> Result<Option<PromptView>, Reason> {
        let Some(agent) = self.agent.as_mut() else {
            return Ok(None);
        };
        let now_us = monotonic_us();
        let activity = match agent.tick(now_us) {
            Ok(activity) => activity,
            Err(error) => {
                (env.emit)(
                    Diagnostic::new(
                        Severity::Warn,
                        "AGENT_FAILED",
                        format!(
                            "the password agent stopped ({error}); leaving questions to the console"
                        ),
                        env.invocation,
                    )
                    .with_priority(Priority::Notice),
                );
                return Err(Reason::AgentFailed);
            }
        };
        report(env, activity, outcome);
        if agent.stranded(now_us) {
            (env.emit)(
                Diagnostic::new(
                    Severity::Warn,
                    "ASK_STRANDED",
                    "no keyboard for the open password question; leaving it to the console",
                    env.invocation,
                )
                .with_priority(Priority::Notice),
            );
            return Err(Reason::AgentFailed);
        }
        Ok(agent.view().map(|view| PromptView {
            message: view.message.to_owned(),
            field: match view.echo {
                Some(text) => Field::Clear(EchoText::new(text)),
                None => Field::Masked(view.typed),
            },
            notice: view.rejected.then(|| REJECTED_NOTICE.to_owned()),
        }))
    }

    /// What systemd's signals said since the last tick (P-045): a failed job,
    /// a queued rescue or emergency target, or more failed units than at the
    /// first reading. Signals arrive as the events happen, so the exit does
    /// not wait for the next poll's answers (FRN-SRS-034). A failure while
    /// the system is stopping is part of the shutdown, not of the boot.
    ///
    /// Implements: FRN-SRS-034
    fn signalled(
        &self,
        env: &Environment<'_>,
        failed_job: Option<String>,
        maintenance_queued: bool,
        failed_count: Option<u32>,
    ) -> Option<Reason> {
        if self.latest.as_ref().is_some_and(Snapshot::is_stopping) {
            return None;
        }
        if maintenance_queued {
            (env.emit)(Diagnostic::new(
                Severity::Warn,
                "MAINTENANCE",
                "rescue or emergency mode was queued; leaving the screen to the console",
                env.invocation,
            ));
            return Some(Reason::Maintenance);
        }
        let more_failed = failed_count
            .zip(self.failed_baseline)
            .is_some_and(|(count, baseline)| count > baseline);
        if failed_job.is_none() && !more_failed {
            return None;
        }
        let message = failed_job.map_or_else(
            || "a unit failed during the boot; leaving the screen to the console".to_owned(),
            |unit| format!("`{unit}` failed during the boot; leaving the screen to the console"),
        );
        (env.emit)(Diagnostic::new(
            Severity::Warn,
            "UNIT_FAILED",
            message,
            env.invocation,
        ));
        Some(Reason::FailedUnit)
    }

    /// Reports, once, that D-Bus missed its deadline (FRN-SRS-016).
    fn check_dbus_deadline(&mut self, env: &Environment<'_>, running: Duration) {
        if self.connected || self.dbus_notice_sent || running < DBUS_DEADLINE {
            return;
        }
        self.dbus_notice_sent = true;
        (env.emit)(
            Diagnostic::new(
                Severity::Warn,
                "DBUS_DEGRADED",
                format!(
                    "no D-Bus connection within {} s; the bar follows the cached boot duration",
                    DBUS_DEADLINE.as_secs()
                ),
                env.invocation,
            )
            .with_priority(Priority::Notice),
        );
    }

    /// Presents one frame. A lost output is retried next tick with the last
    /// frame left up, and a frame dropped while the last flip is still in
    /// flight is skipped until the display has been stuck for
    /// [`FLIP_STALL_LIMIT`]; any other failure ends the run.
    fn present(
        &mut self,
        env: &Environment<'_>,
        output: &mut Output,
        scene: &Scene,
        outcome: &mut Outcome,
        now: Duration,
    ) -> Result<(), Reason> {
        let presenter = match output {
            Output::Pixels(presenter) => presenter,
            Output::Text(text) => return self.write(env, text, scene, outcome, now),
        };
        match presenter.present(scene) {
            Ok(()) => {
                self.stall.clear();
                self.lost_since = None;
                outcome.first_bar.get_or_insert(self.bar.shown());
                outcome.last_status = scene.status().map(str::to_owned);
                Ok(())
            }
            Err(error) if error.is_lost() => {
                // The output is mid-swap (simpledrm to the native driver).
                if !self.lost_output_reported {
                    self.lost_output_reported = true;
                    (env.emit)(Diagnostic::new(
                        Severity::Warn,
                        "OUTPUT_LOST",
                        format!("{error}; retrying every frame"),
                        env.invocation,
                    ));
                }
                let since = *self.lost_since.get_or_insert(now);
                // A question nobody can see is not answered blind: the
                // console agent takes it once the splash has gone.
                if scene.prompt().is_some() && now.saturating_sub(since) > BLIND_PROMPT_LIMIT {
                    (env.emit)(
                        Diagnostic::new(
                            Severity::Warn,
                            "ASK_BLIND",
                            "a password question is open but the output is lost; leaving it to the console",
                            env.invocation,
                        )
                        .with_priority(Priority::Notice),
                    );
                    return Err(Reason::AgentFailed);
                }
                Ok(())
            }
            Err(error) => {
                if error.kind() == RenderErrorKind::Timeout && !self.stall.late(now) {
                    if !self.late_flip_reported {
                        self.late_flip_reported = true;
                        (env.emit)(Diagnostic::new(
                            Severity::Warn,
                            "FLIP_LATE",
                            format!("{error}; dropping the frame"),
                            env.invocation,
                        ));
                    }
                    return Ok(());
                }
                (env.emit)(Diagnostic::new(
                    Severity::Warn,
                    "RENDER_FAILED",
                    format!("drawing failed: {error}"),
                    env.invocation,
                ));
                Err(Reason::RenderFailed)
            }
        }
    }

    /// Writes one scene as text (FRN-SRS-071). A console that cannot be
    /// written ends the run, so a question is never asked where nobody sees it.
    fn write(
        &self,
        env: &Environment<'_>,
        text: &mut TextOutput,
        scene: &Scene,
        outcome: &mut Outcome,
        now: Duration,
    ) -> Result<(), Reason> {
        match text.console.present(scene, now) {
            Ok(_lines) => {
                text.frames += 1;
                text.first.get_or_insert_with(|| text.origin.elapsed());
                outcome.first_bar.get_or_insert(self.bar.shown());
                outcome.last_status = scene.status().map(str::to_owned);
                Ok(())
            }
            Err(error) => {
                (env.emit)(Diagnostic::new(
                    Severity::Warn,
                    "RENDER_FAILED",
                    format!("writing to the console failed: {error}"),
                    env.invocation,
                ));
                Err(Reason::RenderFailed)
            }
        }
    }

    /// Reads the duration cache once it exists; looks again every second until then.
    fn look_for_cache(&mut self, plan: &Plan, now: Duration) {
        if self.durations.is_some() || now < self.next_cache_look {
            return;
        }
        self.next_cache_look = now + CACHE_RETRY;
        let dir = match plan.stage {
            Stage::Initrd => &plan.paths.sysroot_state,
            Stage::System => &plan.paths.state,
        };
        let path = dir.join(DURATIONS_FILE);
        if path.exists() {
            self.durations = Some(read_durations(&path));
        }
    }

    /// Takes every pending D-Bus message; returns a reason to stop, if any.
    fn drain(&mut self, env: &Environment<'_>) -> Option<Reason> {
        let rx = env.systemd.as_ref()?;
        let mut failed_job = None;
        let mut maintenance_queued = false;
        let mut failed_count = None;
        while let Ok(event) = rx.try_recv() {
            match event {
                Event::Connected => self.connected = true,
                Event::JobFailed(unit) => {
                    failed_job.get_or_insert(unit);
                }
                Event::MaintenanceQueued => maintenance_queued = true,
                Event::FailedUnits(count) => failed_count = Some(count),
                Event::Snapshot(snapshot) => {
                    self.connected = true;
                    let baseline = self.failed_baseline.unwrap_or(snapshot.failed_units);
                    // A `reset-failed` lowers the count; a failure after it still counts.
                    self.failed_baseline = Some(baseline.min(snapshot.failed_units));
                    self.latest = Some(snapshot);
                }
                Event::Lost(why) => {
                    if self.connected {
                        (env.emit)(Diagnostic::new(
                            Severity::Warn,
                            "DBUS_LOST",
                            format!("lost the D-Bus connection: {why}; reconnecting"),
                            env.invocation,
                        ));
                    }
                    self.connected = false;
                }
            }
        }
        if let Some(reason) = self.signalled(env, failed_job, maintenance_queued, failed_count) {
            return Some(reason);
        }
        let latest = self.latest.as_ref()?;
        let baseline = self.failed_baseline.unwrap_or(0);
        if latest.failed_units > baseline {
            (env.emit)(Diagnostic::new(
                Severity::Warn,
                "UNIT_FAILED",
                format!(
                    "{} unit(s) failed during the boot; leaving the screen to the console",
                    latest.failed_units - baseline
                ),
                env.invocation,
            ));
            return Some(Reason::FailedUnit);
        }
        if latest.is_maintenance() {
            (env.emit)(Diagnostic::new(
                Severity::Warn,
                "MAINTENANCE",
                "rescue or emergency mode started; leaving the screen to the console",
                env.invocation,
            ));
            return Some(Reason::Maintenance);
        }
        if latest.is_finished() {
            (env.emit)(Diagnostic::new(
                Severity::Info,
                "BOOT_FINISHED",
                "the boot finished without a greeter; leaving the screen to the console",
                env.invocation,
            ));
            return Some(Reason::BootFinished);
        }
        None
    }
}

/// One journal line for what the agent did; never the secret, nor its length.
///
/// Each is an `[OK]` side effect or a `[WARN]`, never `[INFO]`, which the
/// default severity floor hides: the journal must say when a question opened,
/// which keyboards were grabbed and how it ended.
fn report(env: &Environment<'_>, activity: Activity, outcome: &mut Outcome) {
    let (severity, code, message) = match activity {
        Activity::Idle => return,
        Activity::Opened(0) => (
            Severity::Warn,
            "ASK_NO_KEYBOARD",
            "a password question is open, but no keyboard was found (keyboards are read \
             through evdev, which must be loaded); looking again every second"
                .to_owned(),
        ),
        Activity::Opened(n) => (
            Severity::Ok,
            "ASK_OPENED",
            format!("a password question is open; {n} keyboard(s) grabbed"),
        ),
        Activity::Keyboards(0) => (
            Severity::Warn,
            "ASK_NO_KEYBOARD",
            "the keyboards of the open password question have gone; looking again every second"
                .to_owned(),
        ),
        Activity::Keyboards(n) => (
            Severity::Ok,
            "ASK_KEYBOARDS",
            format!("{n} keyboard(s) grabbed for the open password question"),
        ),
        Activity::KeyboardRefused(kind) => (
            Severity::Warn,
            "ASK_KEYBOARD_REFUSED",
            format!(
                "a keyboard was found but could not be grabbed ({kind}); looking again every second"
            ),
        ),
        Activity::Answered => {
            outcome.answered = outcome.answered.saturating_add(1);
            (
                Severity::Ok,
                "ASK_ANSWERED",
                "the password question was answered".to_owned(),
            )
        }
        Activity::Cancelled => (
            Severity::Ok,
            "ASK_CANCELLED",
            "the password question was cancelled".to_owned(),
        ),
        Activity::Closed => (
            Severity::Ok,
            "ASK_CLOSED",
            "the password question closed; keyboards released".to_owned(),
        ),
        Activity::ReplyFailed(kind) => (
            Severity::Warn,
            "ASK_REPLY_FAILED",
            format!("the answer could not be sent ({kind}); the question stays with systemd"),
        ),
    };
    (env.emit)(Diagnostic::new(severity, code, message, env.invocation));
}

/// Writes what outlives the process: the handoff at switch-root (FRN-SRS-014),
/// the measured durations at the greetd handoff (FRN-SRS-015).
fn persist(
    plan: &Plan,
    env: &Environment<'_>,
    outcome: &Outcome,
    timeline: &Timeline,
    cached: Option<Durations>,
    now: Duration,
) {
    let written = match (plan.stage, outcome.reason) {
        (Stage::Initrd, Reason::SwitchRoot | Reason::ExitAfter) => {
            let path = plan.paths.runtime.join(HANDOFF_FILE);
            write_handoff(
                &path,
                &Handoff {
                    bar: outcome.bar,
                    initrd_end: now,
                },
            )
            .map(|()| path)
        }
        (Stage::System, Reason::Handoff | Reason::ExitAfter) => {
            let path = plan.paths.state.join(DURATIONS_FILE);
            // An initrd that never ran leaves the previous measurement standing.
            let initrd = timeline
                .carried
                .map(|h| h.initrd_end)
                .or_else(|| cached.and_then(|d| d.initrd));
            write_durations(
                &path,
                &Durations {
                    initrd,
                    stage2: Some(now.saturating_sub(timeline.stage_origin)),
                },
            )
            .map(|()| path)
        }
        _ => return,
    };
    if let Err(error) = written {
        (env.emit)(Diagnostic::new(
            Severity::Warn,
            "STATE_NOT_WRITTEN",
            format!("cannot write the splash state: {error}"),
            env.invocation,
        ));
    }
}

/// Sends a notification; a failure is reported, never fatal.
fn notify(env: &Environment<'_>, send: fn(&Notifier) -> std::io::Result<()>, what: &str) {
    if let Err(error) = send(env.notifier) {
        (env.emit)(Diagnostic::new(
            Severity::Warn,
            "NOTIFY_FAILED",
            format!("cannot send {what} to the service manager: {error}"),
            env.invocation,
        ));
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::os::unix::net::UnixDatagram;
    use std::sync::mpsc;

    use fairing_theme::Theme;

    use super::*;

    /// A boot clock that advances 10 ms every time it is read.
    struct Ticking(Cell<Duration>);

    impl BootClock for Ticking {
        fn now(&self) -> Duration {
            let now = self.0.get() + Duration::from_millis(10);
            self.0.set(now);
            now
        }
    }

    fn memory(_choice: Choice, size: Size) -> Result<Opened, NoBackend> {
        open(Choice::Memory, size)
    }

    fn nothing(_choice: Choice, _size: Size) -> Result<Opened, NoBackend> {
        let attempt = |backend| fairing_render::Attempt {
            backend,
            error: fairing_render::RenderError::new(
                fairing_render::RenderErrorKind::NotFound,
                "no device",
            ),
            elapsed: Duration::ZERO,
        };
        Err(NoBackend {
            attempts: vec![
                attempt(fairing_render::BackendKind::Drm),
                attempt(fairing_render::BackendKind::Fbdev),
            ],
            elapsed: Duration::ZERO,
        })
    }

    struct Harness {
        dir: tempfile::TempDir,
        listener: UnixDatagram,
        notifier: Notifier,
        diagnostics: RefCell<Vec<Diagnostic>>,
    }

    impl Harness {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
            let socket = dir.path().join("notify");
            let listener = UnixDatagram::bind(&socket).unwrap_or_else(|e| panic!("{e}"));
            listener
                .set_nonblocking(true)
                .unwrap_or_else(|e| panic!("{e}"));
            let notifier = Notifier::new(Some(socket.as_os_str()));
            Self {
                dir,
                listener,
                notifier,
                diagnostics: RefCell::new(Vec::new()),
            }
        }

        fn paths(&self) -> Paths {
            Paths {
                runtime: self.dir.path().join("run"),
                state: self.dir.path().join("var"),
                sysroot_state: self.dir.path().join("sysroot"),
            }
        }

        fn plan(&self, stage: Stage, exit_after: Option<Duration>) -> Plan {
            Plan {
                stage,
                choice: Choice::Memory,
                memory_size: Size::new(160, 90).unwrap_or_else(|e| panic!("{e}")),
                theme: CompiledTheme::builtin(),
                selection: Selection::Color(Theme::family_default()),
                paths: self.paths(),
                exit_after,
                agent: None,
                text: None,
                reduced_motion: false,
            }
        }

        /// The agent's directories, under the harness's temporary directory.
        fn agent_paths(&self) -> AgentPaths {
            AgentPaths {
                ask_dir: self.dir.path().join("ask-password"),
                input_sysfs: self.dir.path().join("sys-class-input"),
                input_dev: self.dir.path().join("dev-input"),
            }
        }

        fn run(
            &self,
            plan: &Plan,
            terminate: bool,
            events: Vec<Event>,
            opener: fn(Choice, Size) -> Result<Opened, NoBackend>,
        ) -> Outcome {
            let flag = AtomicBool::new(terminate);
            let clock = Ticking(Cell::new(Duration::from_secs(5)));
            let (tx, rx) = mpsc::channel();
            for event in events {
                tx.send(event).unwrap_or_else(|e| panic!("{e}"));
            }
            let emit = |d: Diagnostic| self.diagnostics.borrow_mut().push(d);
            let env = Environment {
                clock: &clock,
                terminate: &flag,
                notifier: &self.notifier,
                systemd: (plan.stage == Stage::System).then_some(rx),
                origin: Instant::now(),
                emit: &emit,
                invocation: "fairing splash",
            };
            run_with(plan, &env, opener)
        }

        /// What greetd's start does before it stops the splash.
        fn mark_handoff(&self) {
            let runtime = self.paths().runtime;
            std::fs::create_dir_all(&runtime).unwrap_or_else(|e| panic!("{e}"));
            std::fs::write(runtime.join(HANDOFF_MARKER), b"").unwrap_or_else(|e| panic!("{e}"));
        }

        fn notifications(&self) -> Vec<String> {
            let mut buf = [0_u8; 64];
            let mut out = Vec::new();
            while let Ok(n) = self.listener.recv(&mut buf) {
                out.push(String::from_utf8_lossy(&buf[..n]).trim().to_owned());
            }
            out
        }

        fn codes(&self) -> Vec<&'static str> {
            self.diagnostics.borrow().iter().map(|d| d.code).collect()
        }
    }

    #[test]
    fn an_agent_that_cannot_start_steps_aside_at_once() {
        // Verifies: FRN-SRS-027
        let harness = Harness::new();
        let mut plan = harness.plan(Stage::Initrd, Some(Duration::from_mins(1)));
        let mut paths = harness.agent_paths();
        // A file where the directory should be: the watch cannot be set.
        paths.ask_dir = harness.dir.path().join("not-a-directory");
        std::fs::write(&paths.ask_dir, b"").unwrap_or_else(|e| panic!("{e}"));
        plan.agent = Some(paths);
        std::fs::create_dir_all(&plan.paths.runtime).unwrap_or_else(|e| panic!("{e}"));
        let started = Instant::now();
        let outcome = harness.run(&plan, false, Vec::new(), memory);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(outcome.reason, Reason::AgentFailed);
        assert!(!outcome.agent);
        assert!(harness.codes().contains(&"AGENT_FAILED"));
        assert_eq!(harness.notifications(), ["READY=1", "STOPPING=1"]);
        assert!(!plan.paths.runtime.join(AGENT_FLAG).exists());
    }

    #[test]
    fn the_agent_runs_in_the_loop_and_lowers_its_flag() {
        let harness = Harness::new();
        let mut plan = harness.plan(Stage::Initrd, Some(Duration::from_millis(100)));
        let paths = harness.agent_paths();
        std::fs::create_dir_all(&paths.ask_dir).unwrap_or_else(|e| panic!("{e}"));
        std::fs::create_dir_all(&paths.input_sysfs).unwrap_or_else(|e| panic!("{e}"));
        // The socket sits in the question directory, as systemd puts it (T-061);
        // nobody listens on it.
        std::fs::write(
            paths.ask_dir.join("ask.1"),
            format!(
                "[Ask]\nSocket={}/sck.1\nMessage=Passphrase\nId=cryptsetup:x\n",
                paths.ask_dir.display()
            ),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        plan.agent = Some(paths);
        std::fs::create_dir_all(&plan.paths.runtime).unwrap_or_else(|e| panic!("{e}"));
        let outcome = harness.run(&plan, false, Vec::new(), memory);
        assert_eq!(outcome.reason, Reason::ExitAfter);
        assert!(outcome.agent);
        // No keyboard in the empty sysfs tree: the question is shown and the
        // journal says why nothing can be typed.
        assert!(
            harness.codes().contains(&"ASK_NO_KEYBOARD"),
            "{:?}",
            harness.codes()
        );
        assert!(
            !plan.paths.runtime.join(AGENT_FLAG).exists(),
            "flag lowered"
        );
    }

    fn snapshot(progress: f64, failed: u32, state: &str, job: Option<&str>) -> Event {
        Event::Snapshot(Snapshot {
            progress,
            failed_units: failed,
            system_state: state.to_owned(),
            job: job.map(str::to_owned),
        })
    }

    #[test]
    fn handoff_draws_one_hundred_percent_and_caches_the_durations() {
        // Verifies: FRN-SRS-015, FRN-SRS-017, FRN-SRS-036
        let harness = Harness::new();
        let plan = harness.plan(Stage::System, None);
        harness.mark_handoff();
        let outcome = harness.run(
            &plan,
            true,
            vec![snapshot(0.4, 0, "starting", None)],
            memory,
        );
        assert_eq!(outcome.reason, Reason::Handoff);
        assert!((outcome.bar - 1.0).abs() < f32::EPSILON, "{}", outcome.bar);
        assert_eq!(outcome.frames, 1);
        assert_eq!(harness.notifications(), ["READY=1", "STOPPING=1"]);
        let cached = read_durations(&plan.paths.state.join(DURATIONS_FILE));
        assert!(cached.stage2.is_some(), "{cached:?}");
        assert_eq!(cached.initrd, None, "no initrd ran, none was cached");
    }

    #[test]
    fn stage_two_starts_from_the_initrd_handoff() {
        // Verifies: FRN-SRS-014
        let harness = Harness::new();
        let plan = harness.plan(Stage::System, Some(Duration::from_millis(150)));
        write_handoff(
            &plan.paths.runtime.join(HANDOFF_FILE),
            &Handoff {
                bar: 0.27,
                initrd_end: Duration::from_secs(4),
            },
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let outcome = harness.run(&plan, false, Vec::new(), memory);
        assert_eq!(outcome.carried_bar, Some(0.27));
        let first = outcome.first_bar.unwrap_or_default();
        // The carried value is rounded up to the file's resolution, never down.
        assert!(
            (0.27..0.2702).contains(&first),
            "first frame showed {first}"
        );
        let cached = read_durations(&plan.paths.state.join(DURATIONS_FILE));
        assert_eq!(cached.initrd, Some(Duration::from_secs(4)));
    }

    #[test]
    fn stage_two_counts_from_its_own_start() {
        // An initrd splash stopped by a passphrase prompt at 1 s; stage 2
        // starts at 5 s. The typing time is no part of stage 2.
        let harness = Harness::new();
        let plan = harness.plan(Stage::System, Some(Duration::from_millis(150)));
        write_handoff(
            &plan.paths.runtime.join(HANDOFF_FILE),
            &Handoff {
                bar: 0.1,
                initrd_end: Duration::from_secs(1),
            },
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let outcome = harness.run(&plan, false, Vec::new(), memory);
        assert_eq!(outcome.carried_bar, Some(0.1));
        let cached = read_durations(&plan.paths.state.join(DURATIONS_FILE));
        let stage2 = cached.stage2.unwrap_or(Duration::MAX);
        assert!(
            stage2 < Duration::from_secs(1),
            "stage 2 measured {stage2:?}"
        );
        assert_eq!(cached.initrd, Some(Duration::from_secs(1)));
    }

    #[test]
    fn a_sigterm_without_the_handoff_marker_is_a_plain_stop() {
        // Verifies: FRN-SRS-017
        let harness = Harness::new();
        let plan = harness.plan(Stage::System, None);
        let outcome = harness.run(
            &plan,
            true,
            vec![snapshot(0.4, 0, "starting", None)],
            memory,
        );
        assert_eq!(outcome.reason, Reason::Stopped);
        assert!(outcome.bar < 1.0, "{}", outcome.bar);
        assert!(
            !plan.paths.state.join(DURATIONS_FILE).exists(),
            "a stop is not a measurement"
        );
    }

    #[test]
    fn a_run_that_draws_nothing_still_reports_ready() {
        // Verifies: FRN-SRS-036
        let harness = Harness::new();
        let plan = harness.plan(Stage::System, None);
        let outcome = harness.run(
            &plan,
            false,
            vec![
                snapshot(0.4, 0, "starting", None),
                snapshot(0.5, 1, "degraded", None),
            ],
            memory,
        );
        assert_eq!(outcome.reason, Reason::FailedUnit);
        assert_eq!(outcome.frames, 0);
        assert_eq!(harness.notifications(), ["READY=1", "STOPPING=1"]);
    }

    #[test]
    fn a_handoff_from_after_this_start_is_ignored() {
        let harness = Harness::new();
        let plan = harness.plan(Stage::System, Some(Duration::from_millis(50)));
        write_handoff(
            &plan.paths.runtime.join(HANDOFF_FILE),
            &Handoff {
                bar: 0.27,
                initrd_end: Duration::from_hours(100),
            },
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let outcome = harness.run(&plan, false, Vec::new(), memory);
        assert_eq!(outcome.carried_bar, None);
    }

    #[test]
    fn switch_root_writes_the_handoff_for_stage_two() {
        // Verifies: FRN-SRS-014
        let harness = Harness::new();
        let plan = harness.plan(Stage::Initrd, None);
        let outcome = harness.run(&plan, true, Vec::new(), memory);
        assert_eq!(outcome.reason, Reason::SwitchRoot);
        let handoff = read_handoff(&plan.paths.runtime.join(HANDOFF_FILE))
            .unwrap_or_else(|| panic!("no handoff written"));
        assert!(handoff.initrd_end > Duration::from_secs(5));
        assert!((handoff.bar - outcome.bar).abs() < 1e-3);
        assert!(outcome.bar <= 0.30, "the initrd owns 30 % at most");
    }

    #[test]
    fn a_failed_unit_or_maintenance_mode_ends_the_splash() {
        // Verifies: FRN-SRS-034
        let harness = Harness::new();
        let plan = harness.plan(Stage::System, Some(Duration::from_mins(1)));
        let failed = harness.run(
            &plan,
            false,
            vec![
                snapshot(0.4, 0, "starting", None),
                snapshot(0.5, 1, "degraded", None),
            ],
            memory,
        );
        assert_eq!(failed.reason, Reason::FailedUnit);
        let rescue = harness.run(
            &plan,
            false,
            vec![snapshot(0.5, 0, "maintenance", None)],
            memory,
        );
        assert_eq!(rescue.reason, Reason::Maintenance);
        assert!(harness.codes().contains(&"UNIT_FAILED"));
        assert!(
            !plan.paths.state.join(DURATIONS_FILE).exists(),
            "a failed boot is not a measurement"
        );
    }

    #[test]
    fn failure_signals_end_the_splash_without_a_poll() {
        // Verifies: FRN-SRS-034
        let harness = Harness::new();
        let plan = harness.plan(Stage::System, Some(Duration::from_mins(1)));
        let failed = harness.run(
            &plan,
            false,
            vec![
                Event::Connected,
                Event::JobFailed("nginx.service".to_owned()),
            ],
            memory,
        );
        assert_eq!(failed.reason, Reason::FailedUnit);
        assert_eq!(failed.frames, 0, "the first tick ends it");
        assert!(
            harness
                .diagnostics
                .borrow()
                .iter()
                .any(|d| d.code == "UNIT_FAILED" && d.message.contains("`nginx.service`"))
        );
        let rescue = harness.run(&plan, false, vec![Event::MaintenanceQueued], memory);
        assert_eq!(rescue.reason, Reason::Maintenance);
        let counted = harness.run(
            &plan,
            false,
            vec![snapshot(0.3, 1, "starting", None), Event::FailedUnits(2)],
            memory,
        );
        assert_eq!(counted.reason, Reason::FailedUnit);
    }

    #[test]
    fn signals_of_no_new_failure_change_nothing() {
        let harness = Harness::new();
        let plan = harness.plan(Stage::System, Some(Duration::from_millis(100)));
        // The count is the baseline's; a count before any reading has no
        // baseline to rise above.
        for events in [
            vec![snapshot(0.3, 1, "starting", None), Event::FailedUnits(1)],
            vec![Event::FailedUnits(4)],
            // A stop job failing during a shutdown is the shutdown's.
            vec![
                snapshot(0.9, 0, "stopping", None),
                Event::JobFailed("x.service".to_owned()),
            ],
        ] {
            let outcome = harness.run(&plan, false, events, memory);
            assert_eq!(outcome.reason, Reason::ExitAfter);
        }
    }

    #[test]
    fn units_that_failed_before_the_first_reading_do_not_end_the_splash() {
        // Verifies: FRN-SRS-034
        let harness = Harness::new();
        let plan = harness.plan(Stage::System, Some(Duration::from_millis(100)));
        let outcome = harness.run(
            &plan,
            false,
            vec![snapshot(0.3, 2, "starting", None)],
            memory,
        );
        assert_eq!(outcome.reason, Reason::ExitAfter);
    }

    #[test]
    fn reduced_motion_holds_the_first_frame() {
        // Verifies: FRN-SRS-052
        for running in [
            Duration::ZERO,
            Duration::from_millis(40),
            Duration::from_mins(3),
        ] {
            assert_eq!(animation_time(true, running), Duration::ZERO);
            assert_eq!(animation_time(false, running), running);
        }
    }

    #[test]
    fn reduced_motion_moves_the_bar_without_easing() {
        // Verifies: FRN-SRS-052
        let harness = Harness::new();
        let mut plan = harness.plan(Stage::Initrd, Some(Duration::from_millis(100)));
        let eased = harness.run(&plan, false, Vec::new(), memory);
        plan.reduced_motion = true;
        let jumped = harness.run(&plan, false, Vec::new(), memory);
        // Five seconds into a first boot the target is about 0.19: the
        // eased bar starts from zero, the reduced one is there at once.
        let eased = eased.first_bar.unwrap_or(1.0);
        let jumped = jumped.first_bar.unwrap_or(0.0);
        assert!(eased < 0.01, "{eased}");
        assert!(jumped > 0.15, "{jumped}");
    }

    #[test]
    fn accessible_mode_writes_lines_and_steps_aside_without_a_console() {
        // Verifies: FRN-SRS-070
        let harness = Harness::new();
        let mut plan = harness.plan(Stage::Initrd, Some(Duration::from_millis(100)));
        let console = harness.dir.path().join("console");
        std::fs::write(&console, b"").unwrap_or_else(|e| panic!("{e}"));
        plan.text = Some(TextTarget::Console(console.clone()));
        let outcome = harness.run(&plan, false, Vec::new(), memory);
        assert_eq!(outcome.backend, Some("text"));
        assert_eq!(outcome.reason, Reason::ExitAfter);
        assert!(outcome.first_frame_ms.is_some());
        let written = std::fs::read_to_string(&console).unwrap_or_default();
        assert!(written.starts_with("Fairing "), "{written:?}");

        plan.text = Some(TextTarget::Console(harness.dir.path().join("missing")));
        let outcome = harness.run(&plan, false, Vec::new(), memory);
        assert_eq!(outcome.reason, Reason::NoBackend);
        assert!(harness.notifications().contains(&"READY=1".to_owned()));
    }

    #[test]
    fn a_late_flip_is_a_dropped_frame() {
        let mut stall = Stall::default();
        let first = Duration::from_secs(10);
        assert!(!stall.late(first));
        assert!(!stall.late(first + FLIP_STALL_LIMIT));
        // A frame landed; the next late flip starts a new stall.
        stall.clear();
        assert!(!stall.late(first + FLIP_STALL_LIMIT * 2));
    }

    #[test]
    fn flips_late_for_longer_than_the_limit_end_the_run() {
        let mut stall = Stall::default();
        let first = Duration::from_secs(10);
        assert!(!stall.late(first));
        assert!(stall.late(first + FLIP_STALL_LIMIT + Duration::from_millis(1)));
    }

    #[test]
    fn a_boot_that_finishes_without_greetd_ends_the_splash() {
        let harness = Harness::new();
        let plan = harness.plan(Stage::System, None);
        let outcome = harness.run(
            &plan,
            false,
            vec![
                snapshot(0.9, 0, "starting", None),
                snapshot(1.0, 0, "running", None),
            ],
            memory,
        );
        assert_eq!(outcome.reason, Reason::BootFinished);
        assert!(outcome.bar < 1.0, "{}", outcome.bar);
        assert!(!plan.paths.state.join(DURATIONS_FILE).exists());
    }

    #[test]
    fn sigterm_during_shutdown_is_not_a_handoff() {
        let harness = Harness::new();
        let plan = harness.plan(Stage::System, None);
        let outcome = harness.run(
            &plan,
            true,
            vec![snapshot(1.0, 0, "stopping", None)],
            memory,
        );
        assert_eq!(outcome.reason, Reason::Shutdown);
        assert!(outcome.bar < 1.0);
        assert!(!plan.paths.state.join(DURATIONS_FILE).exists());
    }

    #[test]
    fn no_backend_is_a_notice_and_a_clean_start() {
        // Verifies: FRN-SRS-003
        let harness = Harness::new();
        let plan = harness.plan(Stage::System, None);
        let outcome = harness.run(&plan, false, Vec::new(), nothing);
        assert_eq!(outcome.reason, Reason::NoBackend);
        assert_eq!(outcome.frames, 0);
        assert_eq!(harness.notifications(), ["READY=1"]);
        let diagnostics = harness.diagnostics.borrow();
        let notice = diagnostics
            .iter()
            .find(|d| d.code == "NO_BACKEND")
            .unwrap_or_else(|| panic!("no notice"));
        assert!(
            notice.journal_line().starts_with("<5>"),
            "{}",
            notice.journal_line()
        );
        assert!(
            notice.message.contains("drm") && notice.message.contains("fbdev"),
            "{}",
            notice.message
        );
        assert_eq!(diagnostics.len(), 1, "exactly one journal entry");
    }

    #[test]
    fn missing_dbus_is_reported_once_after_two_seconds() {
        // Verifies: FRN-SRS-016
        let harness = Harness::new();
        // The ticking clock covers 2 s in about 70 frames.
        let plan = harness.plan(Stage::System, Some(Duration::from_millis(2600)));
        let outcome = harness.run(&plan, false, Vec::new(), memory);
        assert_eq!(outcome.reason, Reason::ExitAfter);
        let codes = harness.codes();
        assert_eq!(
            codes.iter().filter(|c| **c == "DBUS_DEGRADED").count(),
            1,
            "{codes:?}"
        );
        assert!(!outcome.dbus);
    }

    #[test]
    fn the_status_line_shows_the_newest_job() {
        // Verifies: FRN-SRS-050
        let harness = Harness::new();
        let plan = harness.plan(Stage::System, Some(Duration::from_millis(100)));
        let outcome = harness.run(
            &plan,
            false,
            vec![snapshot(0.2, 0, "starting", Some("Network Manager"))],
            memory,
        );
        assert_eq!(outcome.last_status.as_deref(), Some("Network Manager"));
        assert!(outcome.dbus);
    }
}
