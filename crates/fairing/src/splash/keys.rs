// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! The journal overlay's keys, read from every keyboard without a grab.
//!
//! The overlay listens; it never takes the keyboard. Its devices are opened
//! read-only and never grabbed, so every key also reaches the kernel: a
//! VT-switch chord, a screen reader's chord or `SysRq` works as it would with
//! no splash at all (FRN-SRS-063). While a password question is open the
//! agent grabs the keyboards, and an exclusive grab stops the events reaching
//! any other reader: the overlay is deaf then, which is what the prompt needs.
//!
//! Keyboards are found as the password agent finds them, from the sysfs
//! capability masks, and looked for again once a second so one plugged in
//! during the boot works too.

use std::os::fd::{AsFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::time::Duration;

use fairing_askpass::ask::Text;
use fairing_askpass::input::{NODE_CAPACITY, key_events, keyboards};
use rustix::fs::{Mode, OFlags};

/// How often keyboards are looked for again.
pub const RESCAN: Duration = Duration::from_secs(1);

// Key codes from <linux/input-event-codes.h>.
const KEY_ESC: u16 = 1;
const KEY_J: u16 = 36;
const KEY_K: u16 = 37;
const KEY_UP: u16 = 103;
const KEY_PAGEUP: u16 = 104;
const KEY_DOWN: u16 = 108;
const KEY_PAGEDOWN: u16 = 109;
const KEY_LEFTCTRL: u16 = 29;
const KEY_LEFTALT: u16 = 56;
const KEY_RIGHTCTRL: u16 = 97;
const KEY_RIGHTALT: u16 = 100;
const KEY_LEFTMETA: u16 = 125;
const KEY_RIGHTMETA: u16 = 126;

/// A key the overlay acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    /// Escape: show or hide the overlay (FRN-SRS-060).
    Toggle,
    /// Up or `k`: one line back (FRN-SRS-061).
    Up,
    /// Down or `j`: one line forward.
    Down,
    /// Page Up: a page back.
    PageUp,
    /// Page Down: a page forward.
    PageDown,
}

/// What one key event means to the overlay, given the chord modifiers held.
///
/// A key pressed with Ctrl, Alt or Meta held is somebody else's chord
/// (Ctrl+Alt+Del, Alt+F2, a screen reader's) and means nothing here.
/// Scrolling repeats while a key is held; Escape toggles once per press.
fn nav(code: u16, value: i32, chord: bool) -> Option<Nav> {
    if chord || value == 0 {
        return None;
    }
    match code {
        KEY_ESC if value == 1 => Some(Nav::Toggle),
        KEY_UP | KEY_K => Some(Nav::Up),
        KEY_DOWN | KEY_J => Some(Nav::Down),
        KEY_PAGEUP => Some(Nav::PageUp),
        KEY_PAGEDOWN => Some(Nav::PageDown),
        _ => None,
    }
}

/// Every keyboard, open for reading and never grabbed.
#[derive(Debug)]
pub struct Navigation {
    sysfs: PathBuf,
    dev: PathBuf,
    open: Vec<(Text<NODE_CAPACITY>, OwnedFd)>,
    next_scan: Duration,
    /// Chord modifiers held, one bit per key.
    held: u8,
}

impl Navigation {
    /// Keyboards listed under `sysfs` (`/sys/class/input`) with nodes under
    /// `dev` (`/dev/input`); nothing is opened until the first read.
    #[must_use]
    pub fn new(sysfs: &Path, dev: &Path) -> Self {
        Self {
            sysfs: sysfs.to_path_buf(),
            dev: dev.to_path_buf(),
            open: Vec::new(),
            next_scan: Duration::ZERO,
            held: 0,
        }
    }

    /// How many keyboards are open.
    #[cfg(test)]
    #[must_use]
    pub fn keyboards(&self) -> usize {
        self.open.len()
    }

    /// Opens keyboards that appeared, at most once per [`RESCAN`] of `now`.
    ///
    /// Implements: FRN-SRS-063
    pub fn scan(&mut self, now: Duration) {
        if now < self.next_scan {
            return;
        }
        self.next_scan = now + RESCAN;
        for name in keyboards(&self.sysfs) {
            if self.open.iter().any(|(open, _)| *open == name) {
                continue;
            }
            // Read-only and never grabbed: the kernel keeps every key.
            if let Ok(fd) = rustix::fs::open(
                self.dev.join(name.as_str()),
                OFlags::RDONLY | OFlags::NONBLOCK | OFlags::CLOEXEC | OFlags::NOCTTY,
                Mode::empty(),
            ) {
                self.open.push((name, fd));
            }
        }
    }

    /// Passes every overlay key typed since the last call to `f`. A keyboard
    /// that has gone is closed.
    pub fn read(&mut self, mut f: impl FnMut(Nav)) {
        let held = &mut self.held;
        let before = self.open.len();
        self.open.retain(|(_, fd)| {
            key_events(fd.as_fd(), &mut |code, value| {
                if let Some(bit) = modifier_bit(code) {
                    if value == 0 {
                        *held &= !bit;
                    } else {
                        *held |= bit;
                    }
                } else if let Some(key) = nav(code, value, *held != 0) {
                    f(key);
                }
            })
            .unwrap_or(false)
        });
        if self.open.len() != before {
            // A keyboard unplugged with a modifier held never sends its release.
            self.held = 0;
        }
    }

    /// Closes every keyboard.
    pub fn close(&mut self) {
        self.open.clear();
        self.held = 0;
    }
}

/// The bit of a chord modifier, if `code` is one.
const fn modifier_bit(code: u16) -> Option<u8> {
    match code {
        KEY_LEFTCTRL => Some(1),
        KEY_RIGHTCTRL => Some(2),
        KEY_LEFTALT => Some(4),
        KEY_RIGHTALT => Some(8),
        KEY_LEFTMETA => Some(16),
        KEY_RIGHTMETA => Some(32),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Write as _;

    use super::*;

    fn event(code: u16, value: i32) -> Vec<u8> {
        let mut bytes = vec![0u8; 2 * size_of::<usize>()];
        bytes.extend_from_slice(&1_u16.to_ne_bytes());
        bytes.extend_from_slice(&code.to_ne_bytes());
        bytes.extend_from_slice(&value.to_ne_bytes());
        bytes
    }

    #[test]
    fn escape_toggles_and_the_cua_and_vim_keys_scroll() {
        // Verifies: FRN-SRS-060, FRN-SRS-061
        assert_eq!(nav(KEY_ESC, 1, false), Some(Nav::Toggle));
        assert_eq!(nav(KEY_ESC, 2, false), None, "a held Escape toggles once");
        assert_eq!(nav(KEY_UP, 1, false), Some(Nav::Up));
        assert_eq!(nav(KEY_K, 2, false), Some(Nav::Up), "scrolling repeats");
        assert_eq!(nav(KEY_DOWN, 1, false), Some(Nav::Down));
        assert_eq!(nav(KEY_J, 1, false), Some(Nav::Down));
        assert_eq!(nav(KEY_PAGEUP, 1, false), Some(Nav::PageUp));
        assert_eq!(nav(KEY_PAGEDOWN, 1, false), Some(Nav::PageDown));
        assert_eq!(nav(KEY_J, 0, false), None, "releases mean nothing");
        assert_eq!(nav(30, 1, false), None);
        assert_eq!(nav(KEY_ESC, 1, true), None, "another program's chord");
    }

    #[test]
    fn keyboards_are_read_without_a_grab_and_chords_are_left_alone() {
        // Verifies: FRN-SRS-063
        let root = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let sysfs = root.path().join("sys");
        let dev = root.path().join("dev");
        let caps = sysfs.join("event4/device/capabilities");
        fs::create_dir_all(&caps).unwrap_or_else(|e| panic!("{e}"));
        fs::create_dir_all(&dev).unwrap_or_else(|e| panic!("{e}"));
        fs::write(caps.join("ev"), "120013\n").unwrap_or_else(|e| panic!("{e}"));
        fs::write(caps.join("key"), "fffffffffffffffe\n").unwrap_or_else(|e| panic!("{e}"));
        // A FIFO stands in for the event node: it can be opened like one,
        // and a grab, which the overlay never attempts, would fail on it.
        let node = dev.join("event4");
        rustix::fs::mknodat(
            rustix::fs::CWD,
            &node,
            rustix::fs::FileType::Fifo,
            Mode::from_raw_mode(0o600),
            0,
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let mut keys = Navigation::new(&sysfs, &dev);
        keys.scan(Duration::ZERO);
        assert_eq!(keys.keyboards(), 1);
        let mut writer = fs::OpenOptions::new()
            .write(true)
            .open(&node)
            .unwrap_or_else(|e| panic!("{e}"));
        let mut bytes = Vec::new();
        for (code, value) in [
            (KEY_ESC, 1),
            (KEY_ESC, 0),
            (KEY_LEFTALT, 1),
            (KEY_DOWN, 1),
            (KEY_LEFTALT, 0),
            (KEY_K, 1),
        ] {
            bytes.extend(event(code, value));
        }
        writer.write_all(&bytes).unwrap_or_else(|e| panic!("{e}"));
        let mut seen = Vec::new();
        keys.read(|key| seen.push(key));
        assert_eq!(seen, [Nav::Toggle, Nav::Up], "Alt+Down was a chord");
        assert_eq!(keys.keyboards(), 1);
        // A rescan within the second opens nothing twice.
        keys.scan(Duration::from_millis(500));
        keys.scan(RESCAN);
        assert_eq!(keys.keyboards(), 1);
        drop(writer);
        // Another test's child may hold a copy of the writer for the moment
        // between its fork and its exec.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while keys.keyboards() > 0 && std::time::Instant::now() < deadline {
            keys.read(|_key| {});
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(keys.keyboards(), 0, "a device gone is closed");
    }
}
