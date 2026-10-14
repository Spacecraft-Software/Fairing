// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! The question directory, watched with inotify and read without blocking.
//!
//! systemd writes each question to a temporary name and renames it to
//! `ask.<random>`, so a new question arrives as `IN_MOVED_TO`; a question
//! withdrawn arrives as `IN_DELETE` or `IN_MOVED_FROM`, and a file written in
//! place (as a test or another agent may) as `IN_CLOSE_WRITE`. The watcher
//! never blocks: [`Watcher::changed`] drains the queue and returns, so the
//! splash calls it once per frame and a new file is seen within one frame
//! (FRN-SRS-020).
//!
//! On a change the directory is scanned again and the question shown is the
//! lowest-named valid, unexpired `ask.*` file, so the choice does not depend
//! on the order events arrive in. Each file is opened without following
//! links and without blocking, must be a regular file, and is read into one
//! buffer of `MAX_FILE + 1` bytes allocated when the watcher opens: what a
//! file contains never decides how much memory is allocated (FRN-SRS-104).

use std::fmt;
use std::fs;
use std::io;
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, OwnedFd};
use std::path::{Path, PathBuf};

use rustix::fs::inotify::{self, CreateFlags, ReadFlags, WatchFlags};
use rustix::fs::{FileType, Mode, OFlags};
use rustix::io::Errno;

use crate::ask::{Ask, MAX_FILE, Text};

/// Where systemd puts its questions.
pub const ASK_DIR: &str = "/run/systemd/ask-password";

/// The longest file name the directory can hold (`NAME_MAX`).
pub const NAME_CAPACITY: usize = 255;

/// Bytes of inotify events read per call: many events with names of
/// `NAME_MAX`, and so always at least one.
const EVENT_BUFFER: usize = 4096;

/// A question found in the directory: its file name and what it asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Question {
    /// The file's name, `ask.<something>`.
    pub name: Text<NAME_CAPACITY>,
    /// The parsed question.
    pub ask: Ask,
}

/// The directory systemd asks its questions in, under inotify.
///
/// Implements: FRN-SRS-020, FRN-SRS-102, FRN-SRS-104
pub struct Watcher {
    dir: PathBuf,
    inotify: OwnedFd,
    events: Box<[MaybeUninit<u8>]>,
    file: Box<[u8]>,
}

impl fmt::Debug for Watcher {
    /// The directory only: the file buffer may hold the last question read.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Watcher")
            .field("dir", &self.dir)
            .finish_non_exhaustive()
    }
}

impl Watcher {
    /// Watches `dir`, creating it first when systemd has not yet.
    ///
    /// The directory is created as systemd's own console agent creates it,
    /// so a question asked before systemd made it is still seen.
    ///
    /// # Errors
    ///
    /// The error from creating the directory, from `inotify_init1`, or from
    /// adding the watch. The agent then cannot run, and the splash leaves the
    /// question to systemd's console agent (FRN-SRS-027).
    pub fn open(dir: &Path) -> io::Result<Self> {
        fs::create_dir_all(dir)?;
        let inotify = inotify::init(CreateFlags::NONBLOCK | CreateFlags::CLOEXEC)?;
        inotify::add_watch(
            &inotify,
            dir,
            WatchFlags::CLOSE_WRITE
                | WatchFlags::MOVED_TO
                | WatchFlags::MOVED_FROM
                | WatchFlags::DELETE
                | WatchFlags::ONLYDIR,
        )?;
        Ok(Self {
            dir: dir.to_path_buf(),
            inotify,
            events: Box::new_uninit_slice(EVENT_BUFFER),
            file: vec![0; MAX_FILE + 1].into_boxed_slice(),
        })
    }

    /// The directory being watched.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Drains the event queue without blocking; `true` when an `ask.*` file
    /// may have come, gone or changed since the last call.
    ///
    /// A queue that overflowed, or a watch the kernel removed, counts as a
    /// change, so the caller rescans rather than missing a question.
    ///
    /// # Errors
    ///
    /// A read error other than "nothing to read".
    pub fn changed(&mut self) -> io::Result<bool> {
        let mut changed = false;
        let mut reader = inotify::Reader::new(self.inotify.as_fd(), &mut self.events);
        loop {
            match reader.next() {
                Ok(event) => {
                    let flags = event.events();
                    if flags.intersects(ReadFlags::QUEUE_OVERFLOW | ReadFlags::IGNORED) {
                        changed = true;
                    }
                    if event
                        .file_name()
                        .is_some_and(|name| name.to_bytes().starts_with(b"ask."))
                    {
                        changed = true;
                    }
                }
                Err(Errno::AGAIN) => return Ok(changed),
                Err(Errno::INTR) => {}
                Err(e) => return Err(e.into()),
            }
        }
    }

    /// The question to show: the lowest-named `ask.*` file that parses, has
    /// not expired at `now_us` (`CLOCK_MONOTONIC`, µs) and is not `skip` (one
    /// already answered). `None` when there is none.
    ///
    /// A file that cannot be read or parsed is passed over, never fatal:
    /// another agent may still answer it.
    ///
    /// # Errors
    ///
    /// The error from reading the directory itself.
    pub fn first(
        &mut self,
        now_us: u64,
        skip: Option<&Text<NAME_CAPACITY>>,
    ) -> io::Result<Option<Question>> {
        let mut best: Option<Question> = None;
        for entry in fs::read_dir(&self.dir)? {
            let Ok(entry) = entry else { continue };
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if !name.starts_with("ask.") || skip.is_some_and(|s| s.as_str() == name) {
                continue;
            }
            if best.is_some_and(|b| b.name.as_str() <= name) {
                continue;
            }
            let Some(text) = Text::exact(name) else {
                continue;
            };
            let Some(ask) = self.read(&entry.path()) else {
                continue;
            };
            if ask.expired(now_us) {
                continue;
            }
            best = Some(Question { name: text, ask });
        }
        Ok(best)
    }

    /// Reads and parses one file into the fixed buffer; `None` on any
    /// failure.
    fn read(&mut self, path: &Path) -> Option<Ask> {
        let fd = rustix::fs::open(
            path,
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::NOCTTY,
            Mode::empty(),
        )
        .ok()?;
        let stat = rustix::fs::fstat(&fd).ok()?;
        if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile {
            return None;
        }
        // Up to MAX_FILE + 1 bytes: one more than allowed is enough for the
        // parser to refuse the file as too large.
        let mut len = 0;
        while len < self.file.len() {
            match rustix::io::read(&fd, &mut self.file[len..]) {
                Ok(0) => break,
                Ok(n) => len += n,
                Err(Errno::INTR) => {}
                Err(_e) => return None,
            }
        }
        Ask::parse(&self.file[..len]).ok()
    }
}

/// `CLOCK_MONOTONIC` in microseconds, the clock of `NotAfter=`.
#[must_use]
pub fn monotonic_us() -> u64 {
    let now = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
    let secs = u64::try_from(now.tv_sec).unwrap_or(0);
    let micros = u64::try_from(now.tv_nsec / 1_000).unwrap_or(0);
    secs.saturating_mul(1_000_000).saturating_add(micros)
}

#[cfg(test)]
#[allow(
    clippy::disallowed_types,
    clippy::disallowed_macros,
    clippy::disallowed_methods,
    reason = "test fixtures are built with them; the agent is not"
)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    fn question(socket: &str, extra: &str) -> String {
        format!(
            "[Ask]\nPID=1\nSocket={socket}\nMessage=Passphrase for root\nId=cryptsetup:/dev/vda2\n{extra}"
        )
    }

    /// Writes `body` the way systemd does: a temporary name, then a rename.
    fn ask(dir: &Path, name: &str, body: &str) {
        let tmp = dir.join(format!(".tmp-{name}"));
        fs::write(&tmp, body).unwrap_or_else(|e| panic!("{e}"));
        fs::rename(&tmp, dir.join(name)).unwrap_or_else(|e| panic!("{e}"));
    }

    fn open() -> (tempfile::TempDir, Watcher) {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let watcher = Watcher::open(dir.path()).unwrap_or_else(|e| panic!("{e}"));
        (dir, watcher)
    }

    #[test]
    fn a_renamed_file_is_seen_within_50_ms() {
        // Verifies: FRN-SRS-020
        let (dir, mut watcher) = open();
        assert!(!watcher.changed().unwrap_or_else(|e| panic!("{e}")));
        ask(dir.path(), "ask.Xy12", &question("/run/sck.1", ""));
        let start = Instant::now();
        loop {
            if watcher.changed().unwrap_or_else(|e| panic!("{e}")) {
                break;
            }
            assert!(
                start.elapsed() < Duration::from_millis(50),
                "not seen in 50 ms"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        let found = watcher
            .first(monotonic_us(), None)
            .unwrap_or_else(|e| panic!("{e}"))
            .unwrap_or_else(|| panic!("no question"));
        assert!(
            start.elapsed() < Duration::from_millis(50),
            "not read in 50 ms"
        );
        assert_eq!(found.name.as_str(), "ask.Xy12");
        assert_eq!(found.ask.message.as_str(), "Passphrase for root");
    }

    #[test]
    fn other_names_are_not_a_change() {
        let (dir, mut watcher) = open();
        fs::write(dir.path().join("sck.1"), b"").unwrap_or_else(|e| panic!("{e}"));
        assert!(!watcher.changed().unwrap_or_else(|e| panic!("{e}")));
        assert!(
            watcher
                .first(0, None)
                .unwrap_or_else(|e| panic!("{e}"))
                .is_none()
        );
    }

    #[test]
    fn a_withdrawn_question_is_a_change() {
        let (dir, mut watcher) = open();
        ask(dir.path(), "ask.a", &question("/run/sck.1", ""));
        assert!(watcher.changed().unwrap_or_else(|e| panic!("{e}")));
        fs::remove_file(dir.path().join("ask.a")).unwrap_or_else(|e| panic!("{e}"));
        assert!(watcher.changed().unwrap_or_else(|e| panic!("{e}")));
        assert!(
            watcher
                .first(0, None)
                .unwrap_or_else(|e| panic!("{e}"))
                .is_none()
        );
    }

    #[test]
    fn an_answered_file_is_skipped() {
        let (dir, mut watcher) = open();
        ask(dir.path(), "ask.a", &question("/run/sck.a", ""));
        ask(dir.path(), "ask.b", &question("/run/sck.b", ""));
        let done = Text::exact("ask.a");
        let found = watcher
            .first(0, done.as_ref())
            .unwrap_or_else(|e| panic!("{e}"))
            .unwrap_or_else(|| panic!("no question"));
        assert_eq!(found.name.as_str(), "ask.b");
    }

    #[test]
    fn the_lowest_valid_unexpired_name_wins() {
        // Verifies: FRN-SRS-023, FRN-SRS-102
        let (dir, mut watcher) = open();
        ask(dir.path(), "ask.b", &question("/run/sck.b", ""));
        // Lower, but not a usable question.
        ask(dir.path(), "ask.a", "[Ask]\nMessage=no socket\n");
        // Lower still, but expired.
        ask(
            dir.path(),
            "ask.0",
            &question("/run/sck.0", "NotAfter=10\n"),
        );
        // Larger than 64 KiB.
        let mut big = question("/run/sck.00", "");
        big.push_str(&"#".repeat(MAX_FILE));
        ask(dir.path(), "ask.00", &big);
        let found = watcher
            .first(1_000, None)
            .unwrap_or_else(|e| panic!("{e}"))
            .unwrap_or_else(|| panic!("no question"));
        assert_eq!(found.name.as_str(), "ask.b");
        assert_eq!(found.ask.socket.as_str(), "/run/sck.b");
    }

    #[test]
    fn links_and_fifos_are_not_read() {
        let (dir, mut watcher) = open();
        let target = dir.path().join("elsewhere");
        fs::write(&target, question("/run/sck.1", "")).unwrap_or_else(|e| panic!("{e}"));
        std::os::unix::fs::symlink(&target, dir.path().join("ask.link"))
            .unwrap_or_else(|e| panic!("{e}"));
        rustix::fs::mknodat(
            rustix::fs::CWD,
            dir.path().join("ask.fifo"),
            FileType::Fifo,
            Mode::RUSR | Mode::WUSR,
            0,
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert!(
            watcher
                .first(0, None)
                .unwrap_or_else(|e| panic!("{e}"))
                .is_none()
        );
    }

    #[test]
    fn a_missing_directory_is_created() {
        let base = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let dir = base.path().join("ask-password");
        let mut watcher = Watcher::open(&dir).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(watcher.dir(), dir);
        ask(&dir, "ask.1", &question("/run/sck.1", ""));
        assert!(watcher.changed().unwrap_or_else(|e| panic!("{e}")));
    }

    #[test]
    fn the_clock_moves_forward() {
        let a = monotonic_us();
        std::thread::sleep(Duration::from_millis(2));
        assert!(monotonic_us() > a);
    }
}
