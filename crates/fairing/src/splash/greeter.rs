// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! The greeter's console, readied at the greetd handoff.
//!
//! When the splash lets the display go, the kernel puts the text console back
//! on the screen. Left alone, that console shows black (the kernel's palette)
//! and whatever boot text it holds until the greeter draws its first frame: a
//! visible flash between the splash and the login screen.
//!
//! At the handoff, before the display is released, the splash sets the
//! console's default background and foreground (palette entries 0 and 7) to
//! the theme's canvas and text colours, hides the cursor and clears the
//! screen. The kernel then brings back a console that is already the
//! splash's canvas, and a greeter such as Gantry paints on it. A greeter
//! sets its own palette when it starts and resets it when it exits.
//!
//! The console is opened write-only, without becoming the controlling
//! terminal and without blocking, so a stuck console costs the colours,
//! never the handoff's release bound.

use std::fs::File;
use std::io::{self, Write as _};
use std::os::unix::fs::FileTypeExt as _;
use std::path::PathBuf;

use fairing_theme::Rgb;
use rustix::fs::{Mode, OFlags};

/// The Linux console sequence that readies it: palette entry 0 is `canvas`,
/// entry 7 is `text`, attributes reset to that default, cursor hidden,
/// cursor home, screen cleared.
///
/// Implements: FRN-SRS-106
#[must_use]
pub fn sequence(canvas: Rgb, text: Rgb) -> String {
    format!(
        "\x1b]P0{:02x}{:02x}{:02x}\x1b]P7{:02x}{:02x}{:02x}\x1b[0m\x1b[?25l\x1b[H\x1b[2J",
        canvas.r, canvas.g, canvas.b, text.r, text.g, text.b
    )
}

/// The console to ready, and whether it may be a device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// The console, normally greetd's VT (`/dev/tty1`).
    pub path: PathBuf,
    /// False under an agent or CI harness: whatever the path names when it
    /// is opened, a character device is refused before anything is written.
    pub devices: bool,
}

/// Writes [`sequence`] to the console `target` names.
///
/// The device check is made on the opened descriptor, so a path swapped for
/// a link to a VT after it was checked still cannot reach one.
///
/// # Errors
///
/// The console cannot be opened (it is never created), is a character
/// device when `target.devices` is false, or does not take the whole
/// sequence at once.
///
/// Implements: FRN-SRS-106
pub fn prepare(target: &Target, canvas: Rgb, text: Rgb) -> io::Result<()> {
    let fd = rustix::fs::open(
        &target.path,
        OFlags::WRONLY | OFlags::NOCTTY | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    let mut console = File::from(fd);
    if !target.devices && console.metadata()?.file_type().is_char_device() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "a device under an agent or CI harness",
        ));
    }
    console.write_all(sequence(canvas, text).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sequence_sets_both_entries_then_clears() {
        // Verifies: FRN-SRS-106
        let text = sequence(Rgb::new(0x00, 0x0b, 0x1e), Rgb::new(0xd0, 0xd8, 0xe8));
        assert_eq!(
            text, "\x1b]P0000b1e\x1b]P7d0d8e8\x1b[0m\x1b[?25l\x1b[H\x1b[2J",
            "palette first, so the clear paints the canvas"
        );
    }

    #[test]
    fn a_missing_console_is_an_error_and_is_never_created() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let path = dir.path().join("tty1");
        let target = Target {
            path: path.clone(),
            devices: true,
        };
        assert!(prepare(&target, Rgb::new(0, 0, 0), Rgb::new(1, 1, 1)).is_err());
        assert!(!path.exists());
    }

    #[test]
    fn without_devices_a_device_is_refused_on_the_open_descriptor() {
        // Verifies: FRN-SRS-106
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let link = dir.path().join("console");
        std::os::unix::fs::symlink("/dev/null", &link).unwrap_or_else(|e| panic!("{e}"));
        let refused = prepare(
            &Target {
                path: link,
                devices: false,
            },
            Rgb::new(0, 0, 0),
            Rgb::new(1, 1, 1),
        );
        assert_eq!(
            refused.map_err(|e| e.kind()),
            Err(io::ErrorKind::PermissionDenied)
        );
        let file = dir.path().join("plain");
        std::fs::write(&file, b"").unwrap_or_else(|e| panic!("{e}"));
        let written = prepare(
            &Target {
                path: file.clone(),
                devices: false,
            },
            Rgb::new(0, 0, 0),
            Rgb::new(1, 1, 1),
        );
        assert!(written.is_ok(), "{written:?}");
        assert!(!std::fs::read(&file).unwrap_or_default().is_empty());
    }
}
