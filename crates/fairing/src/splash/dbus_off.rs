// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! The D-Bus client's stand-in for a build without the `dbus` feature: the
//! initrd copy, where no bus runs.
//!
//! `--stage system` still works: the channel stays silent, so the loop
//! drives the bar from the cached duration and reports the missing bus after
//! its deadline, exactly as when the bus never answers (FRN-SRS-016).

use std::sync::mpsc::{self, Receiver};

use crate::splash::events::Event;

/// Checks a `--bus` value: only `system` and `none` mean anything here.
///
/// # Errors
///
/// A bus address, which this build cannot connect to.
pub fn check_bus(bus: &str) -> Result<(), String> {
    if bus == "system" || bus == "none" {
        Ok(())
    } else {
        Err(format!(
            "`{bus}` needs D-Bus, and this build of fairing has none (built without `dbus`)"
        ))
    }
}

/// A channel that never delivers: its sender is dropped at once.
#[must_use]
pub fn spawn(_bus: &str) -> Receiver<Event> {
    mpsc::channel().1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_named_buses_are_accepted() {
        assert_eq!(check_bus("system"), Ok(()));
        assert_eq!(check_bus("none"), Ok(()));
        assert!(check_bus("unix:path=/tmp/manager").is_err());
        assert!(spawn("system").try_recv().is_err());
    }
}
