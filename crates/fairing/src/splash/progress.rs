// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! The hybrid progress model: two sources, one bar that only moves forward.
//!
//! In the initrd there is no D-Bus, so the bar is a time estimate: elapsed
//! boot time over the initrd duration measured on the previous boot, mapped
//! onto the first 30 % of the bar (FRN-SRS-010), or a fixed curve that reaches
//! 30 % at 8 s when nothing is cached yet (FRN-SRS-011). After switch-root the
//! bar is `0.30 + 0.70 × Manager.Progress` (FRN-SRS-012), or the same mapping
//! of the cached stage-2 duration when D-Bus is not there in time
//! (FRN-SRS-016). Only the greetd handoff takes it to 100 % (FRN-SRS-017).
//!
//! The model is pure: callers pass the elapsed time and what they know, so
//! every requirement here is tested with a virtual clock.

use std::time::Duration;

/// Share of the bar the initrd owns (FRN-SRS-010, FRN-SRS-012).
pub const INITRD_SHARE: f32 = 0.30;

/// Time the first-boot initrd curve takes to reach [`INITRD_SHARE`] (FRN-SRS-011).
pub const FIRST_BOOT_INITRD: Duration = Duration::from_secs(8);

/// Stage-2 duration assumed when neither D-Bus nor a cached measurement exists.
///
/// The PRD fixes only the initrd curve. A cold Bravais boot on the reference
/// machine spends roughly 6–12 s between switch-root and greetd; 15 s keeps a
/// first boot from parking the bar at 99 % for long while never claiming more
/// progress than the boot has made. Only a first boot with D-Bus down uses it.
pub const FIRST_BOOT_STAGE2: Duration = Duration::from_secs(15);

/// Highest value the bar may show before the greetd handoff (FRN-SRS-017).
///
/// `Manager.Progress` reaches 1.0 when the boot transaction is done, which can
/// be before greetd is started; the bar waits at 99 % for the real handoff.
pub const BEFORE_HANDOFF: f32 = 0.99;

/// Time constant of the bar's ease toward a jump in the target.
///
/// `Manager.Progress` can leap from 0.0 to 1.0 within 300 ms on an `NVMe` boot
/// (PRD risk register, S3). Three time constants (400 ms) close 95 % of a jump,
/// so the bar visibly travels instead of teleporting, and stays well inside
/// the 500 ms a person reads as "immediate".
pub const EASE_TIME_CONSTANT: Duration = Duration::from_millis(133);

/// Distance at which an eased bar snaps to its target instead of creeping.
const SNAP: f32 = 0.0005;

/// Which instance of Fairing is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stage {
    /// `fairing-initrd.service`, before switch-root.
    Initrd,
    /// `fairing.service`, after switch-root until greetd.
    System,
}

impl Stage {
    /// Stable lowercase name, as the CLI spells it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Initrd => "initrd",
            Self::System => "system",
        }
    }
}

/// What the model knows at one instant.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Inputs {
    /// Time since the stage began: kernel start for the initrd, the end of the
    /// initrd for stage 2.
    pub elapsed: Duration,
    /// This stage's duration measured on a previous boot, if any.
    pub cached: Option<Duration>,
    /// The latest `Manager.Progress`, stage 2 only, if D-Bus delivered one.
    pub systemd_progress: Option<f64>,
    /// greetd has taken over: the one event that may show 100 %.
    pub handoff: bool,
}

/// The value the bar is heading for, in `0.0..=1.0`.
///
/// Implements: FRN-SRS-010, FRN-SRS-011, FRN-SRS-012, FRN-SRS-016, FRN-SRS-017
#[must_use]
pub fn target(stage: Stage, inputs: &Inputs) -> f32 {
    if inputs.handoff {
        return 1.0;
    }
    let value = match stage {
        Stage::Initrd => {
            INITRD_SHARE * ratio(inputs.elapsed, usable(inputs.cached, FIRST_BOOT_INITRD))
        }
        Stage::System => {
            let share = 1.0 - INITRD_SHARE;
            let progress = match inputs.systemd_progress {
                Some(progress) if progress.is_finite() => clamp_progress(progress),
                _ => ratio(inputs.elapsed, usable(inputs.cached, FIRST_BOOT_STAGE2)),
            };
            share.mul_add(progress, INITRD_SHARE)
        }
    };
    value.clamp(0.0, BEFORE_HANDOFF)
}

/// A cached duration, unless it is missing or zero (a zero would divide by zero).
fn usable(cached: Option<Duration>, fallback: Duration) -> Duration {
    cached.filter(|d| !d.is_zero()).unwrap_or(fallback)
}

/// `elapsed / total`, clamped to `0.0..=1.0`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a ratio clamped to 0.0..=1.0 is exact enough in f32 for a progress bar"
)]
fn ratio(elapsed: Duration, total: Duration) -> f32 {
    (elapsed.as_secs_f64() / total.as_secs_f64()).clamp(0.0, 1.0) as f32
}

/// `Manager.Progress` clamped to `0.0..=1.0`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a value clamped to 0.0..=1.0 is exact enough in f32 for a progress bar"
)]
fn clamp_progress(progress: f64) -> f32 {
    progress.clamp(0.0, 1.0) as f32
}

/// The value on screen: eased toward the target, never decreasing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bar {
    shown: f32,
    last: Option<Duration>,
}

impl Bar {
    /// A bar that starts at `carried` (the initrd's last value, FRN-SRS-014).
    #[must_use]
    pub fn new(carried: f32) -> Self {
        let shown = if carried.is_finite() {
            carried.clamp(0.0, BEFORE_HANDOFF)
        } else {
            0.0
        };
        Self { shown, last: None }
    }

    /// The value on screen.
    #[must_use]
    pub const fn shown(&self) -> f32 {
        self.shown
    }

    /// Moves toward `target` as of `now` and returns the value to draw.
    ///
    /// A lower target is held, never shown (FRN-SRS-013). A higher one is
    /// approached exponentially with [`EASE_TIME_CONSTANT`], or reached at once
    /// under reduced motion (FRN-SRS-052) and on the handoff.
    ///
    /// Implements: FRN-SRS-013
    pub fn advance(&mut self, target: f32, now: Duration, immediate: bool) -> f32 {
        let dt = self
            .last
            .map_or(Duration::ZERO, |last| now.saturating_sub(last));
        self.last = Some(now);
        if !target.is_finite() || target <= self.shown {
            return self.shown;
        }
        let target = target.min(1.0);
        let gap = target - self.shown;
        if immediate || target >= 1.0 || gap <= SNAP {
            self.shown = target;
            return self.shown;
        }
        let step = gap * ease_fraction(dt);
        // Never past the target, never backwards, even for a huge or zero dt.
        self.shown = (self.shown + step).clamp(self.shown, target);
        if target - self.shown <= SNAP {
            self.shown = target;
        }
        self.shown
    }
}

/// Fraction of the remaining gap closed after `dt`: `1 - e^(-dt/τ)`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the result lies in 0.0..=1.0 and only scales a progress step"
)]
fn ease_fraction(dt: Duration) -> f32 {
    let tau = EASE_TIME_CONSTANT.as_secs_f64();
    (1.0 - (-dt.as_secs_f64() / tau).exp()).clamp(0.0, 1.0) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(s: f64) -> Duration {
        Duration::from_secs_f64(s)
    }

    fn initrd(elapsed: f64, cached: Option<f64>) -> f32 {
        target(
            Stage::Initrd,
            &Inputs {
                elapsed: secs(elapsed),
                cached: cached.map(secs),
                ..Inputs::default()
            },
        )
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn cached_initrd_duration_scales_the_first_thirty_percent() {
        // Verifies: FRN-SRS-010
        assert!(close(initrd(0.0, Some(4.0)), 0.0));
        assert!(close(initrd(2.0, Some(4.0)), 0.15));
        assert!(close(initrd(4.0, Some(4.0)), 0.30));
        assert!(close(initrd(60.0, Some(4.0)), 0.30), "clamped to 0.30");
    }

    #[test]
    fn first_boot_curve_reaches_thirty_percent_at_eight_seconds() {
        // Verifies: FRN-SRS-011
        assert!(close(initrd(0.0, None), 0.0));
        assert!(close(initrd(4.0, None), 0.15));
        assert!(close(initrd(8.0, None), 0.30));
        assert!(close(initrd(30.0, None), 0.30));
        assert!(
            close(initrd(8.0, Some(0.0)), 0.30),
            "a zero cache is no cache"
        );
    }

    #[test]
    fn stage_two_maps_progress_onto_the_upper_seventy_percent() {
        // Verifies: FRN-SRS-012
        let at = |p: f64| {
            target(
                Stage::System,
                &Inputs {
                    systemd_progress: Some(p),
                    elapsed: secs(100.0),
                    ..Inputs::default()
                },
            )
        };
        assert!(close(at(0.0), 0.30));
        assert!(close(at(0.5), 0.65));
        assert!(close(at(0.9), 0.93));
        assert!(close(at(-3.0), 0.30));
        assert!(close(at(1.0), BEFORE_HANDOFF), "1.0 waits for greetd");
        assert!(
            close(at(f64::NAN), BEFORE_HANDOFF),
            "NaN falls back to time"
        );
    }

    #[test]
    fn missing_dbus_drives_stage_two_from_the_cached_duration() {
        // Verifies: FRN-SRS-016
        let at = |elapsed: f64, cached: Option<f64>| {
            target(
                Stage::System,
                &Inputs {
                    elapsed: secs(elapsed),
                    cached: cached.map(secs),
                    ..Inputs::default()
                },
            )
        };
        assert!(close(at(0.0, Some(10.0)), 0.30));
        assert!(close(at(5.0, Some(10.0)), 0.65));
        assert!(
            close(at(7.5, None), 0.65),
            "first boot uses FIRST_BOOT_STAGE2"
        );
    }

    #[test]
    fn the_bar_reaches_one_only_on_handoff() {
        // Verifies: FRN-SRS-017
        let mut bar = Bar::new(0.0);
        let mut t = Duration::ZERO;
        for progress in [0.0, 0.5, 1.0, 1.0, 1.0] {
            for _ in 0..60 {
                t += Duration::from_millis(33);
                let goal = target(
                    Stage::System,
                    &Inputs {
                        systemd_progress: Some(progress),
                        ..Inputs::default()
                    },
                );
                assert!(bar.advance(goal, t, false) < 1.0);
            }
        }
        assert!(close(bar.shown(), BEFORE_HANDOFF));
        let handoff = target(
            Stage::System,
            &Inputs {
                handoff: true,
                ..Inputs::default()
            },
        );
        assert!(close(bar.advance(handoff, t, false), 1.0));
        assert!(
            close(Bar::new(1.0).shown(), BEFORE_HANDOFF),
            "carry-over is capped"
        );
    }

    #[test]
    fn the_bar_never_decreases_under_adversarial_inputs() {
        // Verifies: FRN-SRS-013
        // A deterministic xorshift walk over targets, gaps and reduced motion.
        let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let mut bar = Bar::new(0.2);
        let mut now = Duration::ZERO;
        let mut previous = bar.shown();
        for _ in 0..100_000 {
            let draw = next();
            now += Duration::from_millis(draw % 200);
            let goal = match draw % 7 {
                0 => f32::NAN,
                1 => -1.0,
                2 => 2.0,
                _ => f32::from(u16::try_from(draw % 1_000).unwrap_or(0)) / 1_000.0,
            };
            let shown = bar.advance(goal, now, draw % 5 == 0);
            assert!(shown >= previous, "{shown} < {previous} for target {goal}");
            assert!((0.0..=1.0).contains(&shown));
            previous = shown;
        }
    }

    #[test]
    fn a_jump_is_eased_over_about_four_hundred_milliseconds() {
        let mut bar = Bar::new(0.3);
        bar.advance(0.3, Duration::ZERO, false);
        let after_one_frame = bar.advance(0.99, Duration::from_millis(33), false);
        assert!(
            after_one_frame > 0.3 && after_one_frame < 0.6,
            "{after_one_frame}"
        );
        let mut t = Duration::from_millis(33);
        while t < Duration::from_millis(400) {
            t += Duration::from_millis(33);
            bar.advance(0.99, t, false);
        }
        assert!(bar.shown() > 0.3 + 0.95 * 0.69, "{}", bar.shown());
        let mut reduced = Bar::new(0.3);
        assert!(close(reduced.advance(0.99, Duration::ZERO, true), 0.99));
    }

    #[test]
    fn stage_names_match_the_cli() {
        assert_eq!(Stage::Initrd.as_str(), "initrd");
        assert_eq!(Stage::System.as_str(), "system");
    }
}
