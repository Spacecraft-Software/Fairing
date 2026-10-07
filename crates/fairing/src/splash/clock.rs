// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Boot time: `CLOCK_BOOTTIME`, the time since the kernel started.
//!
//! The initrd's share of the bar is elapsed boot time over the initrd's
//! measured duration (FRN-SRS-010), and that duration is the boot time at
//! which the initrd ended, so both must come from the one clock that starts
//! with the kernel. `std::time::Instant` cannot say when the kernel started.
//! The trait keeps every consumer testable with a virtual clock
//! (M-MOCKABLE-SYSCALLS).

use std::time::Duration;

use rustix::time::{ClockId, clock_gettime};

/// A source of boot time.
pub trait BootClock {
    /// Time since the kernel started.
    fn now(&self) -> Duration;
}

/// The kernel's `CLOCK_BOOTTIME`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Boottime;

impl BootClock for Boottime {
    fn now(&self) -> Duration {
        let ts = clock_gettime(ClockId::Boottime);
        Duration::new(
            u64::try_from(ts.tv_sec).unwrap_or(0),
            u32::try_from(ts.tv_nsec).unwrap_or(0),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boot_time_is_positive_and_advances() {
        let clock = Boottime;
        let first = clock.now();
        assert!(first > Duration::ZERO);
        std::thread::sleep(Duration::from_millis(5));
        assert!(clock.now() > first);
    }
}
