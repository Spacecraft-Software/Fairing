// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Fixed-rate pacing for the redraw loop (FRN-SRS-005).
//!
//! The clock is a trait so the pacing logic is tested with a virtual clock
//! (M-MOCKABLE-SYSCALLS); production uses [`SystemClock`].

use std::fmt;
use std::time::{Duration, Instant};

/// A source of monotonic time and sleep.
pub trait Clock {
    /// The current monotonic instant.
    fn now(&self) -> Instant;
    /// Blocks for `duration`.
    fn sleep(&self, duration: Duration);
}

/// The operating system's monotonic clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

impl<C: Clock + ?Sized> Clock for &C {
    fn now(&self) -> Instant {
        (**self).now()
    }

    fn sleep(&self, duration: Duration) {
        (**self).sleep(duration);
    }
}

/// Paces a loop at a declared frame rate.
///
/// Deadlines advance by a fixed period from the first call, so jitter in one
/// frame does not accumulate. When the loop falls more than one period behind,
/// the schedule re-anchors to now instead of bursting to catch up: a slow
/// machine drops frames, it never draws faster than the declared rate.
#[derive(Debug, Clone)]
pub struct Cadence<C: Clock = SystemClock> {
    clock: C,
    period: Duration,
    next: Option<Instant>,
    dropped: u64,
}

impl Cadence<SystemClock> {
    /// A cadence of `hz` frames per second on the system clock (`hz == 0` is treated as 1).
    #[must_use]
    pub fn new(hz: u32) -> Self {
        Self::with_clock(hz, SystemClock)
    }
}

impl<C: Clock> Cadence<C> {
    /// A cadence of `hz` frames per second on `clock`.
    #[must_use]
    pub fn with_clock(hz: u32, clock: C) -> Self {
        let period = Duration::from_secs(1) / hz.max(1);
        Self {
            clock,
            period,
            next: None,
            dropped: 0,
        }
    }

    /// The frame period.
    #[must_use]
    pub const fn period(&self) -> Duration {
        self.period
    }

    /// Frames skipped because the loop fell behind.
    #[must_use]
    pub const fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Blocks until the next frame deadline and returns how late the previous frame finished.
    ///
    /// The first call returns immediately and anchors the schedule.
    pub fn wait(&mut self) -> Duration {
        let now = self.clock.now();
        let Some(deadline) = self.next else {
            self.next = Some(now + self.period);
            return Duration::ZERO;
        };
        if now < deadline {
            self.clock.sleep(deadline - now);
            self.next = Some(deadline + self.period);
            return Duration::ZERO;
        }
        let late = now - deadline;
        if late >= self.period {
            // Re-anchor rather than burst; count the whole periods lost.
            let lost = late.as_nanos() / self.period.as_nanos().max(1);
            self.dropped += u64::try_from(lost).unwrap_or(u64::MAX);
            self.next = Some(now + self.period);
        } else {
            self.next = Some(deadline + self.period);
        }
        late
    }
}

impl<C: Clock> fmt::Display for Cadence<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.1} Hz", 1.0 / self.period.as_secs_f64())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    /// A clock that only moves when slept or nudged.
    #[derive(Debug)]
    struct Virtual {
        base: Instant,
        offset: Cell<Duration>,
    }

    impl Virtual {
        fn new() -> Self {
            Self {
                base: Instant::now(),
                offset: Cell::new(Duration::ZERO),
            }
        }

        fn advance(&self, by: Duration) {
            self.offset.set(self.offset.get() + by);
        }
    }

    impl Clock for Virtual {
        fn now(&self) -> Instant {
            self.base + self.offset.get()
        }

        fn sleep(&self, duration: Duration) {
            self.advance(duration);
        }
    }

    #[test]
    fn thirty_hertz_yields_thirty_frames_per_second() {
        // Verifies: FRN-SRS-005
        let clock = Virtual::new();
        let mut cadence = Cadence::with_clock(30, &clock);
        let start = clock.now();
        let mut frames = 0;
        while clock.now() - start < Duration::from_secs(5) {
            cadence.wait();
            // Each frame takes 4 ms of "work" before the next wait.
            clock.advance(Duration::from_millis(4));
            frames += 1;
        }
        assert!((149..=151).contains(&frames), "{frames} frames in 5 s");
        assert_eq!(cadence.dropped(), 0);
        assert_eq!(cadence.to_string(), "30.0 Hz");
    }

    #[test]
    fn a_slow_frame_drops_instead_of_bursting() {
        // Verifies: FRN-SRS-005
        let clock = Virtual::new();
        let mut cadence = Cadence::with_clock(30, &clock);
        cadence.wait();
        clock.advance(Duration::from_millis(100)); // three periods of work
        let late = cadence.wait();
        assert!(late >= Duration::from_millis(66), "{late:?}");
        assert_eq!(cadence.dropped(), 2);
        // The next deadline is one period from now, not three in the past.
        let before = clock.now();
        cadence.wait();
        assert!(clock.now() - before >= Duration::from_millis(33));
    }

    #[test]
    fn zero_hertz_is_clamped() {
        assert_eq!(Cadence::new(0).period(), Duration::from_secs(1));
    }
}
