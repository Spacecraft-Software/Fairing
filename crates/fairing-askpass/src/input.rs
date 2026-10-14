// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Keyboard input from evdev, grabbed only while a question is open.
//!
//! Keyboards are found the way udev finds them, from the capability bitmasks
//! in sysfs: a device that reports `EV_KEY` and every key from Esc to D is a
//! keyboard. While a prompt is open each one is grabbed exclusively, so no
//! keystroke reaches the console's line discipline and none of the secret
//! can be read back from the tty; when it closes every grab is released at
//! once, so VT-switch and assistive chords reach the kernel again
//! (FRN-SRS-028, FRN-SRS-063).
//!
//! The `evdev` crate is used for two things only: wrapping a descriptor
//! Fairing opened itself, and the `EVIOCGRAB` ioctl. Events are read with a
//! plain `read(2)` into a buffer on the stack that is zeroised after every
//! read, because the key codes in it are the secret (FRN-SRS-024).
//!
//! Key codes become characters through a fixed US layout: the initrd has no
//! xkb, and the console's own keymap is not readable without `unsafe`.

use std::fs;
use std::io;
use std::os::fd::{AsFd, BorrowedFd};
use std::path::{Path, PathBuf};

use evdev::raw_stream::RawDevice;
use rustix::fs::{Mode, OFlags};
use rustix::io::Errno;
use zeroize::Zeroize;

use crate::ask::Text;

/// Where the kernel lists input devices.
pub const SYSFS_INPUT: &str = "/sys/class/input";

/// Where their device nodes are.
pub const DEV_INPUT: &str = "/dev/input";

/// `EV_KEY`, the event type of a key.
const EV_KEY: u16 = 1;

/// Bytes of one `struct input_event`: a `struct timeval` of two longs, then
/// `type: u16`, `code: u16` and `value: i32`.
const EVENT_SIZE: usize = 2 * size_of::<usize>() + 8;

/// Events read per `read(2)`.
const EVENTS_PER_READ: usize = 64;

/// A key that means something to a prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// A character to type.
    Char(char),
    /// Remove the last character.
    Backspace,
    /// Answer the question.
    Enter,
    /// Cancel the question.
    Escape,
}

/// The modifier state and the fixed US layout.
#[derive(Debug, Default, Clone, Copy)]
pub struct Keymap {
    shift_left: bool,
    shift_right: bool,
    ctrl: u8,
    alt: u8,
    caps_lock: bool,
}

// Key codes from <linux/input-event-codes.h>.
const KEY_ESC: u16 = 1;
const KEY_BACKSPACE: u16 = 14;
const KEY_ENTER: u16 = 28;
const KEY_LEFTCTRL: u16 = 29;
const KEY_LEFTSHIFT: u16 = 42;
const KEY_RIGHTSHIFT: u16 = 54;
const KEY_LEFTALT: u16 = 56;
const KEY_CAPSLOCK: u16 = 58;
const KEY_KPENTER: u16 = 96;
const KEY_RIGHTCTRL: u16 = 97;
const KEY_RIGHTALT: u16 = 100;

/// The printable keys of the US layout: code, then the character without and
/// with Shift. The keypad gives its digits whatever the Num Lock state.
const US: &[(u16, char, char)] = &[
    (2, '1', '!'),
    (3, '2', '@'),
    (4, '3', '#'),
    (5, '4', '$'),
    (6, '5', '%'),
    (7, '6', '^'),
    (8, '7', '&'),
    (9, '8', '*'),
    (10, '9', '('),
    (11, '0', ')'),
    (12, '-', '_'),
    (13, '=', '+'),
    (16, 'q', 'Q'),
    (17, 'w', 'W'),
    (18, 'e', 'E'),
    (19, 'r', 'R'),
    (20, 't', 'T'),
    (21, 'y', 'Y'),
    (22, 'u', 'U'),
    (23, 'i', 'I'),
    (24, 'o', 'O'),
    (25, 'p', 'P'),
    (26, '[', '{'),
    (27, ']', '}'),
    (30, 'a', 'A'),
    (31, 's', 'S'),
    (32, 'd', 'D'),
    (33, 'f', 'F'),
    (34, 'g', 'G'),
    (35, 'h', 'H'),
    (36, 'j', 'J'),
    (37, 'k', 'K'),
    (38, 'l', 'L'),
    (39, ';', ':'),
    (40, '\'', '"'),
    (41, '`', '~'),
    (43, '\\', '|'),
    (44, 'z', 'Z'),
    (45, 'x', 'X'),
    (46, 'c', 'C'),
    (47, 'v', 'V'),
    (48, 'b', 'B'),
    (49, 'n', 'N'),
    (50, 'm', 'M'),
    (51, ',', '<'),
    (52, '.', '>'),
    (53, '/', '?'),
    (55, '*', '*'),
    (57, ' ', ' '),
    (71, '7', '7'),
    (72, '8', '8'),
    (73, '9', '9'),
    (74, '-', '-'),
    (75, '4', '4'),
    (76, '5', '5'),
    (77, '6', '6'),
    (78, '+', '+'),
    (79, '1', '1'),
    (80, '2', '2'),
    (81, '3', '3'),
    (82, '0', '0'),
    (83, '.', '.'),
    (98, '/', '/'),
];

impl Keymap {
    /// A layout with no modifier held and Caps Lock off.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            shift_left: false,
            shift_right: false,
            ctrl: 0,
            alt: 0,
            caps_lock: false,
        }
    }

    /// Forgets every held modifier, as when a grab starts: keys held before
    /// it were never seen going down.
    pub const fn reset(&mut self) {
        *self = Self::new();
    }

    /// One `EV_KEY` event: `value` is 1 for a press, 2 for auto-repeat and 0
    /// for a release. Returns the key it means to a prompt, if any.
    ///
    /// With Ctrl or Alt held a key types nothing, so a chord never leaves a
    /// stray character in the secret.
    pub fn key(&mut self, code: u16, value: i32) -> Option<Key> {
        let down = value != 0;
        match code {
            KEY_LEFTSHIFT => self.shift_left = down,
            KEY_RIGHTSHIFT => self.shift_right = down,
            KEY_LEFTCTRL | KEY_RIGHTCTRL => self.ctrl = held(self.ctrl, code, down),
            KEY_LEFTALT | KEY_RIGHTALT => self.alt = held(self.alt, code, down),
            KEY_CAPSLOCK if value == 1 => self.caps_lock = !self.caps_lock,
            _ => {}
        }
        if !down {
            return None;
        }
        match code {
            KEY_ENTER | KEY_KPENTER if value == 1 => return Some(Key::Enter),
            KEY_ESC if value == 1 => return Some(Key::Escape),
            KEY_BACKSPACE => return Some(Key::Backspace),
            _ => {}
        }
        if self.ctrl != 0 || self.alt != 0 {
            return None;
        }
        let &(_, plain, shifted) = US.iter().find(|&&(c, _, _)| c == code)?;
        let mut shift = self.shift_left || self.shift_right;
        if self.caps_lock && plain.is_ascii_lowercase() {
            shift = !shift;
        }
        Some(Key::Char(if shift { shifted } else { plain }))
    }
}

/// Tracks the left and right key of one modifier as two bits.
const fn held(state: u8, code: u16, down: bool) -> u8 {
    let bit = if code == KEY_LEFTCTRL || code == KEY_LEFTALT {
        1
    } else {
        2
    };
    if down { state | bit } else { state & !bit }
}

/// Whether bit `n` is set in a sysfs capability mask: hexadecimal words of a
/// `long` each, most significant first.
fn bit(mask: &str, n: usize) -> bool {
    let width = usize::try_from(usize::BITS).unwrap_or(64);
    mask.split_whitespace()
        .rev()
        .nth(n / width)
        .and_then(|word| usize::from_str_radix(word, 16).ok())
        .is_some_and(|word| word & (1 << (n % width)) != 0)
}

/// The longest node name kept, `event` and a number.
pub const NODE_CAPACITY: usize = 16;

/// A sysfs attribute is at most a page.
const ATTRIBUTE_BYTES: usize = 4096;

/// Reads a sysfs attribute into `buf`, a page at most; `None` when it cannot
/// be read or is not text.
fn attribute<'b>(path: &Path, buf: &'b mut [u8; ATTRIBUTE_BYTES]) -> Option<&'b str> {
    let mut file = fs::File::open(path).ok()?;
    let mut len = 0;
    while len < buf.len() {
        match io::Read::read(&mut file, &mut buf[len..]) {
            Ok(0) => break,
            Ok(n) => len += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_e) => return None,
        }
    }
    str::from_utf8(&buf[..len]).ok()
}

/// Whether the capabilities describe a keyboard, as udev decides it: `EV_KEY`
/// and every key from Esc (1) to D (31).
fn is_keyboard(ev: &str, key: &str) -> bool {
    bit(ev, usize::from(EV_KEY)) && (1..32).all(|n| bit(key, n))
}

/// The `eventN` names of every keyboard under `sysfs` (normally
/// [`SYSFS_INPUT`]), sorted.
///
/// Implements: FRN-SRS-028
#[must_use]
pub fn keyboards(sysfs: &Path) -> Vec<Text<NODE_CAPACITY>> {
    let Ok(entries) = fs::read_dir(sysfs) else {
        return Vec::new();
    };
    let mut ev = [0u8; ATTRIBUTE_BYTES];
    let mut key = [0u8; ATTRIBUTE_BYTES];
    let mut found: Vec<Text<NODE_CAPACITY>> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let os_name = entry.file_name();
            let name = os_name.to_str()?;
            if !name.starts_with("event") {
                return None;
            }
            let name = Text::exact(name)?;
            let caps = entry.path().join("device/capabilities");
            let ev = attribute(&caps.join("ev"), &mut ev)?;
            let key = attribute(&caps.join("key"), &mut key)?;
            is_keyboard(ev, key).then_some(name)
        })
        .collect();
    found.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    found
}

/// Reads every pending event from `fd` without blocking and passes each key
/// that means something through `map` to `f`. `Ok(false)` when the device
/// has gone.
///
/// The read buffer is zeroised before this returns, on every path.
///
/// # Errors
///
/// A read error other than "nothing to read" or "device gone".
pub fn drain(fd: BorrowedFd<'_>, map: &mut Keymap, f: &mut impl FnMut(Key)) -> io::Result<bool> {
    let mut buf = [0u8; EVENT_SIZE * EVENTS_PER_READ];
    let result = loop {
        match rustix::io::read(fd, &mut buf) {
            // End of file, or ENODEV: the device has gone.
            Ok(0) | Err(Errno::NODEV) => break Ok(false),
            Ok(n) => {
                for event in buf[..n].chunks_exact(EVENT_SIZE) {
                    let at = 2 * size_of::<usize>();
                    let kind = u16::from_ne_bytes([event[at], event[at + 1]]);
                    let code = u16::from_ne_bytes([event[at + 2], event[at + 3]]);
                    let value = i32::from_ne_bytes([
                        event[at + 4],
                        event[at + 5],
                        event[at + 6],
                        event[at + 7],
                    ]);
                    if kind == EV_KEY
                        && let Some(key) = map.key(code, value)
                    {
                        f(key);
                    }
                }
            }
            Err(Errno::AGAIN) => break Ok(true),
            Err(Errno::INTR) => {}
            Err(e) => break Err(e.into()),
        }
    };
    buf.zeroize();
    result
}

/// One keyboard Fairing has open.
struct Keyboard {
    name: Text<NODE_CAPACITY>,
    device: RawDevice,
}

/// Every keyboard, grabbed while a prompt is open.
///
/// Implements: FRN-SRS-028, FRN-SRS-063
pub struct Keyboards {
    sysfs: PathBuf,
    dev: PathBuf,
    open: Vec<Keyboard>,
    grabbed: bool,
    map: Keymap,
}

impl core::fmt::Debug for Keyboards {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Keyboards")
            .field(
                "open",
                &self.open.iter().map(|k| &k.name).collect::<Vec<_>>(),
            )
            .field("grabbed", &self.grabbed)
            .finish_non_exhaustive()
    }
}

impl Keyboards {
    /// Keyboards listed under `sysfs` with nodes under `dev`; nothing is
    /// opened until [`Keyboards::grab`].
    #[must_use]
    pub fn new(sysfs: &Path, dev: &Path) -> Self {
        Self {
            sysfs: sysfs.to_path_buf(),
            dev: dev.to_path_buf(),
            open: Vec::new(),
            grabbed: false,
            map: Keymap::new(),
        }
    }

    /// Whether a grab is held.
    #[must_use]
    pub const fn grabbed(&self) -> bool {
        self.grabbed
    }

    /// Opens and grabs every keyboard, then discards what was typed before
    /// the prompt opened. Also call it while grabbed, about once a second,
    /// to take a keyboard plugged in after the prompt opened.
    ///
    /// Returns how many keyboards are grabbed. A keyboard that cannot be
    /// opened or grabbed is skipped.
    pub fn grab(&mut self) -> usize {
        if !self.grabbed {
            self.map.reset();
        }
        self.grabbed = true;
        for name in keyboards(&self.sysfs) {
            if self.open.iter().any(|k| k.name == name) {
                continue;
            }
            let Ok(fd) = rustix::fs::open(
                self.dev.join(name.as_str()),
                OFlags::RDONLY | OFlags::NONBLOCK | OFlags::CLOEXEC | OFlags::NOCTTY,
                Mode::empty(),
            ) else {
                continue;
            };
            let Ok(mut device) = RawDevice::from_fd(fd) else {
                continue;
            };
            if device.grab().is_err() {
                continue;
            }
            // Typed before the grab: not part of the answer.
            let _ignored = drain(device.as_fd(), &mut Keymap::new(), &mut |_key| {});
            self.open.push(Keyboard { name, device });
        }
        self.open.len()
    }

    /// Releases every grab and closes every keyboard, at once.
    ///
    /// Closing a descriptor releases its grab too, so a failed ungrab still
    /// ends with the keyboard free.
    pub fn release(&mut self) {
        for mut keyboard in self.open.drain(..) {
            let _ignored = keyboard.device.ungrab();
        }
        self.grabbed = false;
        self.map.reset();
    }

    /// Passes every key typed since the last call to `f`; nothing while no
    /// grab is held. A keyboard that has gone is closed.
    pub fn read(&mut self, mut f: impl FnMut(Key)) {
        if !self.grabbed {
            return;
        }
        let map = &mut self.map;
        self.open
            .retain(|keyboard| drain(keyboard.device.as_fd(), map, &mut f).unwrap_or(false));
    }
}

impl Drop for Keyboards {
    fn drop(&mut self) {
        self.release();
    }
}

#[cfg(test)]
#[allow(
    clippy::disallowed_types,
    clippy::disallowed_macros,
    clippy::disallowed_methods,
    reason = "test fixtures are built with them; the agent is not"
)]
mod tests {
    use std::os::unix::net::UnixStream;

    use super::*;

    fn press(map: &mut Keymap, code: u16) -> Option<Key> {
        let key = map.key(code, 1);
        map.key(code, 0);
        key
    }

    fn typed(map: &mut Keymap, codes: &[u16]) -> String {
        codes
            .iter()
            .filter_map(|&c| match press(map, c) {
                Some(Key::Char(ch)) => Some(ch),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_us_layout_types_letters_digits_and_symbols() {
        let mut map = Keymap::new();
        // h e l l o space 1 2
        assert_eq!(typed(&mut map, &[35, 18, 38, 38, 24, 57, 2, 3]), "hello 12");
        map.key(KEY_LEFTSHIFT, 1);
        assert_eq!(typed(&mut map, &[35, 2, 53]), "H!?");
        map.key(KEY_LEFTSHIFT, 0);
        assert_eq!(typed(&mut map, &[35]), "h");
    }

    #[test]
    fn caps_lock_shifts_letters_only() {
        let mut map = Keymap::new();
        press(&mut map, KEY_CAPSLOCK);
        assert_eq!(typed(&mut map, &[30, 2]), "A1");
        map.key(KEY_RIGHTSHIFT, 1);
        assert_eq!(typed(&mut map, &[30, 2]), "a!");
        map.key(KEY_RIGHTSHIFT, 0);
        press(&mut map, KEY_CAPSLOCK);
        assert_eq!(typed(&mut map, &[30]), "a");
    }

    #[test]
    fn enter_escape_backspace_and_repeat() {
        let mut map = Keymap::new();
        assert_eq!(press(&mut map, KEY_ENTER), Some(Key::Enter));
        assert_eq!(press(&mut map, KEY_KPENTER), Some(Key::Enter));
        assert_eq!(press(&mut map, KEY_ESC), Some(Key::Escape));
        assert_eq!(map.key(KEY_BACKSPACE, 2), Some(Key::Backspace));
        assert_eq!(map.key(30, 2), Some(Key::Char('a')));
        // A held Enter does not answer twice.
        assert_eq!(map.key(KEY_ENTER, 2), None);
        assert_eq!(map.key(30, 0), None);
    }

    #[test]
    fn a_chord_types_nothing() {
        let mut map = Keymap::new();
        map.key(KEY_LEFTCTRL, 1);
        map.key(KEY_RIGHTCTRL, 1);
        map.key(KEY_LEFTCTRL, 0);
        assert_eq!(press(&mut map, 46), None, "Ctrl+C is not a 'c'");
        map.key(KEY_RIGHTCTRL, 0);
        map.key(KEY_LEFTALT, 1);
        assert_eq!(press(&mut map, 46), None);
        map.key(KEY_LEFTALT, 0);
        assert_eq!(press(&mut map, 46), Some(Key::Char('c')));
    }

    #[test]
    fn capability_masks_are_read_word_by_word() {
        let width = usize::try_from(usize::BITS).unwrap_or(64);
        assert!(bit("1", 0));
        assert!(!bit("1", 1));
        assert!(bit("120013", 1));
        let two_words = format!("1 {:x}", 0);
        assert!(bit(&two_words, width));
        assert!(!bit(&two_words, 0));
        assert!(!bit("", 3));
        assert!(!bit("zz", 0));
    }

    fn device(root: &Path, name: &str, ev: &str, key: &str) {
        let caps = root.join(name).join("device/capabilities");
        fs::create_dir_all(&caps).unwrap_or_else(|e| panic!("{e}"));
        fs::write(caps.join("ev"), ev).unwrap_or_else(|e| panic!("{e}"));
        fs::write(caps.join("key"), key).unwrap_or_else(|e| panic!("{e}"));
    }

    #[test]
    fn keyboards_are_found_from_sysfs_capabilities() {
        let root = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        // An AT keyboard as the kernel lists it.
        device(
            root.path(),
            "event3",
            "120013\n",
            "402000000 3803078f800d001 feffffdfffefffff fffffffffffffffe\n",
        );
        // A power button: EV_KEY, but one key.
        device(root.path(), "event0", "3\n", "4 0 0 0\n");
        // A mouse: no EV_KEY keys from Esc to D.
        device(root.path(), "event5", "17\n", "70000 0 0 0 0\n");
        // Not an event node.
        device(root.path(), "mouse0", "120013\n", "fffffffffffffffe\n");
        device(root.path(), "event1", "120013\n", "fffffffffffffffe\n");
        let found = keyboards(root.path());
        let names: Vec<&str> = found.iter().map(Text::as_str).collect();
        assert_eq!(names, ["event1", "event3"]);
        assert!(keyboards(&root.path().join("missing")).is_empty());
    }

    fn event(kind: u16, code: u16, value: i32) -> Vec<u8> {
        let mut bytes = vec![0u8; 2 * size_of::<usize>()];
        bytes.extend_from_slice(&kind.to_ne_bytes());
        bytes.extend_from_slice(&code.to_ne_bytes());
        bytes.extend_from_slice(&value.to_ne_bytes());
        bytes
    }

    #[test]
    fn drain_decodes_key_events_and_skips_the_rest() {
        let (mut tx, rx) = UnixStream::pair().unwrap_or_else(|e| panic!("{e}"));
        rx.set_nonblocking(true).unwrap_or_else(|e| panic!("{e}"));
        let mut bytes = Vec::new();
        for (kind, code, value) in [(4, 4, 30), (1, 30, 1), (0, 0, 0), (1, 30, 0), (1, 28, 1)] {
            bytes.extend(event(kind, code, value));
        }
        io::Write::write_all(&mut tx, &bytes).unwrap_or_else(|e| panic!("{e}"));
        let mut keys = Vec::new();
        let alive = drain(rx.as_fd(), &mut Keymap::new(), &mut |k| keys.push(k))
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(alive);
        assert_eq!(keys, [Key::Char('a'), Key::Enter]);
        drop(tx);
        let gone =
            drain(rx.as_fd(), &mut Keymap::new(), &mut |_k| {}).unwrap_or_else(|e| panic!("{e}"));
        assert!(!gone, "end of file is a device gone");
    }

    #[test]
    fn nothing_is_read_or_grabbed_without_a_prompt() {
        let root = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        device(root.path(), "event1", "120013\n", "fffffffffffffffe\n");
        let mut boards = Keyboards::new(root.path(), &root.path().join("dev"));
        assert!(!boards.grabbed());
        let mut called = false;
        boards.read(|_k| called = true);
        assert!(!called);
        // The node does not exist, so nothing opens, but the grab is
        // pending for a keyboard that appears.
        assert_eq!(boards.grab(), 0);
        assert!(boards.grabbed());
        boards.release();
        assert!(!boards.grabbed());
        assert!(format!("{boards:?}").contains("grabbed: false"));
    }
}
