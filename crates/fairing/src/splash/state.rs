// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Files that outlive one Fairing process.
//!
//! - `/run/fairing/state` carries the initrd's last bar value and the boot
//!   time at which the initrd ended across switch-root, so the stage-2 instance
//!   starts where the initrd stopped (FRN-SRS-014). `/run` is a tmpfs that
//!   systemd moves into the new root, so the file survives the pivot.
//! - `/var/lib/fairing/boot-duration` holds the initrd and stage-2 durations
//!   measured at the greetd handoff, read by the next boot's time estimates
//!   (FRN-SRS-010, FRN-SRS-015, FRN-SRS-016).
//!
//! Both are `key=value` lines. Reads are bounded and forgiving: a missing,
//! oversized, non-UTF-8 or malformed file is the same as no file, because a
//! splash must never fail a boot over its own cache. Writes go to a sibling
//! `.part` file that is renamed into place, so a reader never sees half a file.

use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The runtime directory systemd gives both units (`RuntimeDirectory=fairing`).
pub const RUNTIME_DIR: &str = "/run/fairing";
/// The state directory of `fairing.service` (`StateDirectory=fairing`).
pub const STATE_DIR: &str = "/var/lib/fairing";
/// Where the initrd can see stage 2's state directory once the root is mounted.
pub const SYSROOT_STATE_DIR: &str = "/sysroot/var/lib/fairing";
/// File name of the switch-root handoff, under [`RUNTIME_DIR`].
pub const HANDOFF_FILE: &str = "state";
/// File name of the greetd handoff marker, under [`RUNTIME_DIR`].
///
/// greetd's start writes it just before it stops the splash (the NixOS
/// module's `ExecStartPre`), so the SIGTERM that follows is known to be the
/// handoff (FRN-SRS-017). Any other SIGTERM in stage 2 (a password prompt, a
/// stop with no shutdown in view) is not, and neither fills the bar nor counts
/// as a measurement.
pub const HANDOFF_MARKER: &str = "handoff";
/// File name of the duration cache, under [`STATE_DIR`].
pub const DURATIONS_FILE: &str = "boot-duration";

/// Largest file either reader accepts.
///
/// Both files hold two or three short lines; anything larger was not written
/// by Fairing and is ignored rather than read into memory.
const MAX_FILE_BYTES: u64 = 4096;

/// Longest duration a cache entry may claim.
///
/// A boot that takes an hour is a boot that needs attention, not an estimate;
/// a larger value means a corrupt or hand-edited file, and is dropped so the
/// bar falls back to the first-boot curve instead of standing still.
const MAX_DURATION: Duration = Duration::from_hours(1);

/// Bar resolution in the handoff file: ten-thousandths, an integer, so the
/// file never depends on float formatting.
const BAR_STEPS: u32 = 10_000;

/// The initrd's last word, read by the stage-2 instance (FRN-SRS-014).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Handoff {
    /// The bar value the initrd last showed, `0.0..=1.0`.
    pub bar: f32,
    /// Boot time (`CLOCK_BOOTTIME`) at which the initrd instance stopped.
    pub initrd_end: Duration,
}

/// Durations measured on a previous boot (FRN-SRS-015).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Durations {
    /// Kernel start to switch-root.
    pub initrd: Option<Duration>,
    /// Switch-root to the greetd handoff.
    pub stage2: Option<Duration>,
}

/// Reads the handoff file; `None` when it is absent or not Fairing's.
#[must_use]
pub fn read_handoff(path: &Path) -> Option<Handoff> {
    let text = read_bounded(path)?;
    let steps = value(&text, "bar")?.parse::<u32>().ok()?;
    // A boot-clock reading, not a duration: no bound here. Whether it comes
    // before this process started is the reader's check.
    let initrd_end = Duration::from_millis(value(&text, "initrd_end_ms")?.parse::<u64>().ok()?);
    if steps > BAR_STEPS {
        return None;
    }
    Some(Handoff {
        bar: steps_to_fraction(steps),
        initrd_end,
    })
}

/// Writes the handoff file atomically, creating its directory.
///
/// # Errors
///
/// The I/O error of the first step that failed; the part file is removed.
pub fn write_handoff(path: &Path, handoff: &Handoff) -> io::Result<()> {
    let text = format!(
        "bar={}\ninitrd_end_ms={}\n",
        fraction_to_steps(handoff.bar),
        handoff.initrd_end.as_millis()
    );
    write_atomically(path, &text)
}

/// Reads the duration cache; absent or unreadable entries are `None`.
#[must_use]
pub fn read_durations(path: &Path) -> Durations {
    let Some(text) = read_bounded(path) else {
        return Durations::default();
    };
    Durations {
        initrd: value(&text, "initrd_ms").and_then(milliseconds),
        stage2: value(&text, "stage2_ms").and_then(milliseconds),
    }
}

/// Writes the duration cache atomically, creating its directory.
///
/// An unknown duration is written as absent, so the next boot uses its
/// fallback for that stage rather than a zero.
///
/// # Errors
///
/// The I/O error of the first step that failed; the part file is removed.
pub fn write_durations(path: &Path, durations: &Durations) -> io::Result<()> {
    let mut text = String::new();
    if let Some(initrd) = durations.initrd {
        let _ = writeln!(text, "initrd_ms={}", initrd.as_millis());
    }
    if let Some(stage2) = durations.stage2 {
        let _ = writeln!(text, "stage2_ms={}", stage2.as_millis());
    }
    write_atomically(path, &text)
}

/// The file's text if it exists, is at most [`MAX_FILE_BYTES`] and is UTF-8.
fn read_bounded(path: &Path) -> Option<String> {
    let file = File::open(path).ok()?;
    let mut bytes = Vec::new();
    // Read one byte past the limit so an oversized file is detected, not truncated.
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes).ok()?;
    if u64::try_from(bytes.len()).ok()? > MAX_FILE_BYTES {
        return None;
    }
    String::from_utf8(bytes).ok()
}

/// The value of the last `key=` line, trimmed.
fn value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines()
        .filter_map(|line| line.split_once('='))
        .filter(|(k, _)| k.trim() == key)
        .map(|(_, v)| v.trim())
        .next_back()
}

/// A millisecond count within [`MAX_DURATION`].
fn milliseconds(text: &str) -> Option<Duration> {
    let ms = text.parse::<u64>().ok()?;
    let duration = Duration::from_millis(ms);
    (duration <= MAX_DURATION).then_some(duration)
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is clamped to 0.0..=10000.0 and rounded before the cast"
)]
/// Rounded up, never to nearest: the carried value must not be below the
/// value the initrd last drew, or the bar would step back across switch-root
/// (FRN-SRS-013).
fn fraction_to_steps(fraction: f32) -> u32 {
    let clamped = if fraction.is_finite() {
        fraction.clamp(0.0, 1.0)
    } else {
        0.0
    };
    (clamped * 10_000.0).ceil().min(10_000.0) as u32
}

#[expect(
    clippy::cast_precision_loss,
    reason = "steps are at most 10 000, exactly representable in f32"
)]
fn steps_to_fraction(steps: u32) -> f32 {
    steps as f32 / BAR_STEPS as f32
}

/// `path` with `.part` appended to its file name.
fn part_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    path.with_file_name(name)
}

fn write_atomically(path: &Path, text: &str) -> io::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let part = part_path(path);
    // No fsync: this runs on the way out, after the display is released and
    // while greetd waits for the process to exit, and an fsync has no bound
    // (FRN-SRS-037). The rename keeps a reader from seeing half a file; a crash
    // can at worst lose the file, which reads as "no previous boot".
    let written = File::create(&part)
        .and_then(|mut file| file.write_all(text.as_bytes()))
        .and_then(|()| fs::rename(&part, path));
    if written.is_err() {
        let _ = fs::remove_file(&part);
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn handoff_round_trips_through_a_new_directory() {
        // Verifies: FRN-SRS-014
        let dir = dir();
        let path = dir.path().join("run/fairing/state");
        let handoff = Handoff {
            bar: 0.2734,
            initrd_end: Duration::from_millis(4210),
        };
        write_handoff(&path, &handoff).unwrap_or_else(|e| panic!("{e}"));
        let read = read_handoff(&path).unwrap_or_else(|| panic!("handoff missing"));
        assert!(
            read.bar >= 0.2734 && read.bar - 0.2734 < 2e-4,
            "{}",
            read.bar
        );
        assert_eq!(read.initrd_end, Duration::from_millis(4210));
        assert!(!part_path(&path).exists());
        // A machine up for longer than any plausible boot still hands over.
        let late = Handoff {
            bar: 0.3,
            initrd_end: Duration::from_hours(5),
        };
        write_handoff(&path, &late).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            read_handoff(&path).map(|h| h.initrd_end),
            Some(Duration::from_hours(5))
        );
    }

    #[test]
    fn durations_round_trip_and_unknowns_stay_unknown() {
        // Verifies: FRN-SRS-015
        let dir = dir();
        let path = dir.path().join("boot-duration");
        let durations = Durations {
            initrd: Some(Duration::from_millis(4210)),
            stage2: None,
        };
        write_durations(&path, &durations).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(read_durations(&path), durations);
        assert_eq!(
            fs::read_to_string(&path).unwrap_or_default(),
            "initrd_ms=4210\n"
        );
    }

    #[test]
    fn foreign_or_damaged_files_read_as_absent() {
        let dir = dir();
        let path = dir.path().join("state");
        assert_eq!(read_handoff(&path), None, "missing");
        fs::write(&path, "bar=20000\ninitrd_end_ms=10\n").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(read_handoff(&path), None, "bar beyond 1.0");
        fs::write(&path, "bar=12\n").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(read_handoff(&path), None, "no initrd end");
        fs::write(&path, [0xFF, 0xFE, b'\n']).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(read_handoff(&path), None, "not UTF-8");
        let big = "x".repeat(5000);
        fs::write(&path, format!("bar=1\ninitrd_end_ms=1\n{big}"))
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(read_handoff(&path), None, "oversized");
        fs::write(&path, "initrd_ms=999999999\nstage2_ms=abc\n").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(read_durations(&path), Durations::default());
        fs::write(
            &path,
            "# comment\ninitrd_ms = 1500 \nstage2_ms=1\nstage2_ms=2500\n",
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            read_durations(&path),
            Durations {
                initrd: Some(Duration::from_millis(1500)),
                stage2: Some(Duration::from_millis(2500)),
            },
            "last value wins, whitespace is trimmed"
        );
    }

    #[test]
    fn bar_steps_are_exact_and_clamped() {
        assert_eq!(fraction_to_steps(0.0), 0);
        assert_eq!(fraction_to_steps(1.0), 10_000);
        assert_eq!(fraction_to_steps(0.5), 5_000);
        assert_eq!(fraction_to_steps(7.0), 10_000);
        assert_eq!(fraction_to_steps(f32::NAN), 0);
        assert_eq!(fraction_to_steps(-1.0), 0);
        // Never below the drawn value.
        for drawn in [0.273_44_f32, 0.1, 0.299_99, 0.000_01] {
            assert!(
                steps_to_fraction(fraction_to_steps(drawn)) >= drawn,
                "{drawn}"
            );
        }
    }

    #[test]
    fn a_failed_write_leaves_no_part_file() {
        let dir = dir();
        // A directory where the file should be makes the rename fail.
        let path = dir.path().join("state");
        fs::create_dir(&path).unwrap_or_else(|e| panic!("{e}"));
        fs::write(path.join("occupant"), "x").unwrap_or_else(|e| panic!("{e}"));
        let error = write_handoff(
            &path,
            &Handoff {
                bar: 0.1,
                initrd_end: Duration::ZERO,
            },
        );
        assert!(error.is_err());
        assert!(!part_path(&path).exists());
    }
}
