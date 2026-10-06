// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! What the splash learns from systemd, as messages.
//!
//! The D-Bus client runs on its own thread and sends these over a channel;
//! the render loop drains the channel once per frame and never waits on it,
//! which is how every wait on D-Bus stays bounded (FRN-SRS-037). Tests feed
//! the loop through the same channel without a bus.

/// The manager's state at one poll.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    /// `Manager.Progress`, `0.0..=1.0` (FRN-SRS-012).
    pub progress: f64,
    /// `Manager.NFailedUnits` (FRN-SRS-034).
    pub failed_units: u32,
    /// `Manager.SystemState`: `starting`, `running`, `maintenance` (rescue or
    /// emergency, FRN-SRS-034), `stopping` (a shutdown, not a handoff), ...
    pub system_state: String,
    /// Description of the most recently started job's unit (FRN-SRS-050).
    pub job: Option<String>,
}

impl Snapshot {
    /// Rescue or emergency mode is active.
    #[must_use]
    pub fn is_maintenance(&self) -> bool {
        self.system_state == "maintenance"
    }

    /// The system is shutting down.
    #[must_use]
    pub fn is_stopping(&self) -> bool {
        self.system_state == "stopping"
    }
}

/// One message from the D-Bus thread.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(
    not(feature = "dbus"),
    expect(dead_code, reason = "only the D-Bus client sends events")
)]
pub enum Event {
    /// The connection to the bus is up.
    Connected,
    /// A fresh reading.
    Snapshot(Snapshot),
    /// The connection failed or dropped; the thread keeps retrying.
    Lost(String),
}
