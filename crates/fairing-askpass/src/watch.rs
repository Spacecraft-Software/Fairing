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
//!
//! A question whose `Socket=` is not directly inside the watched directory is
//! passed over: systemd always creates the socket there, so any other path,
//! one through `..` included, means a forged or garbled file, and the secret
//! must never be sent to it (maintainer decision T-061).

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
    /// How long ago the file was last written when it was read, in µs of
    /// `CLOCK_REALTIME`, the clock of its modification time: how long the
    /// question waited to be seen (FRN-SRS-020). `None` when the file's time
    /// is later than the clock's, as after the clock was set back.
    pub age_us: Option<u64>,
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

    /// The question to show. `keep`, the question already open, stays while
    /// its file is still there and valid, so a new question never takes
    /// over a prompt mid-typing. Otherwise it is the lowest-named `ask.*`
    /// file that parses, has not expired at `now_us` (`CLOCK_MONOTONIC`, µs),
    /// is not `skip` (one already answered) and whose asker is still alive,
    /// as systemd's own agents decide. `None` when there is none.
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
        keep: Option<&Text<NAME_CAPACITY>>,
    ) -> io::Result<Option<Question>> {
        let mut best: Option<Question> = None;
        for entry in fs::read_dir(&self.dir)? {
            let Ok(entry) = entry else { continue };
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if !name.starts_with("ask.") || skip.is_some_and(|s| s.as_str() == name) {
                continue;
            }
            let kept = keep.is_some_and(|k| k.as_str() == name);
            if !kept && best.is_some_and(|b| b.name.as_str() <= name) {
                continue;
            }
            let Some(text) = Text::exact(name) else {
                continue;
            };
            let Some((ask, age_us)) = self.read(&entry.path()) else {
                continue;
            };
            if !socket_inside(&self.dir, ask.socket.as_str())
                || ask.expired(now_us)
                || asker_gone(ask.pid)
            {
                continue;
            }
            if kept {
                return Ok(Some(Question {
                    name: text,
                    ask,
                    age_us,
                }));
            }
            best = Some(Question {
                name: text,
                ask,
                age_us,
            });
        }
        Ok(best)
    }

    /// Reads and parses one file into the fixed buffer, with how long ago it
    /// was written; `None` on any failure.
    fn read(&mut self, path: &Path) -> Option<(Ask, Option<u64>)> {
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
        let age_us = age_us(
            &stat,
            rustix::time::clock_gettime(rustix::time::ClockId::Realtime),
        );
        Ask::parse(&self.file[..len]).ok().map(|ask| (ask, age_us))
    }
}

/// Microseconds from `stat`'s modification time to `now`, both
/// `CLOCK_REALTIME`; `None` when the file is newer than the clock.
fn age_us(stat: &rustix::fs::Stat, now: rustix::time::Timespec) -> Option<u64> {
    let micros = |secs: i128, nanos: i128| secs * 1_000_000 + nanos / 1_000;
    let written = micros(stat.st_mtime.into(), stat.st_mtime_nsec.into());
    let now = micros(now.tv_sec.into(), now.tv_nsec.into());
    u64::try_from(now - written).ok()
}

/// Whether `socket` names an entry directly inside `dir` (T-061). A last
/// component of `..` has no file name; one earlier makes the parent differ.
fn socket_inside(dir: &Path, socket: &str) -> bool {
    let socket = Path::new(socket);
    socket.file_name().is_some() && socket.parent() == Some(dir)
}

/// Whether the process that asked has exited: a stale file systemd has
/// not yet removed. Only "no such process" counts; a process Fairing may
/// not signal is still alive.
fn asker_gone(pid: Option<u32>) -> bool {
    pid.and_then(|p| i32::try_from(p).ok())
        .and_then(rustix::process::Pid::from_raw)
        .is_some_and(|pid| rustix::process::test_kill_process(pid) == Err(Errno::SRCH))
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
            "[Ask]\nSocket={socket}\nMessage=Passphrase for root\nId=cryptsetup:/dev/vda2\n{extra}"
        )
    }

    /// Writes `body` the way systemd does: a temporary name, then a rename.
    /// A `Socket=/run/` path is moved into `dir`, where systemd puts sockets.
    fn ask(dir: &Path, name: &str, body: &str) {
        let body = body.replace("Socket=/run/", &format!("Socket={}/", dir.display()));
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
            .first(monotonic_us(), None, None)
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
                .first(0, None, None)
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
                .first(0, None, None)
                .unwrap_or_else(|e| panic!("{e}"))
                .is_none()
        );
    }

    #[test]
    fn the_open_question_stays_when_a_lower_one_arrives() {
        let (dir, mut watcher) = open();
        ask(dir.path(), "ask.b", &question("/run/sck.b", ""));
        let open = Text::exact("ask.b");
        ask(dir.path(), "ask.a", &question("/run/sck.a", ""));
        let found = watcher
            .first(0, None, open.as_ref())
            .unwrap_or_else(|e| panic!("{e}"))
            .unwrap_or_else(|| panic!("no question"));
        assert_eq!(found.name.as_str(), "ask.b", "not taken over mid-typing");
        let found = watcher
            .first(0, None, None)
            .unwrap_or_else(|e| panic!("{e}"))
            .unwrap_or_else(|| panic!("no question"));
        assert_eq!(found.name.as_str(), "ask.a");
    }

    #[test]
    fn a_question_whose_asker_has_exited_is_passed_over() {
        let (dir, mut watcher) = open();
        let mut child = std::process::Command::new("true")
            .spawn()
            .unwrap_or_else(|e| panic!("{e}"));
        let dead = child.id();
        child.wait().unwrap_or_else(|e| panic!("{e}"));
        ask(
            dir.path(),
            "ask.a",
            &question("/run/sck.a", &format!("PID={dead}\n")),
        );
        assert!(
            watcher
                .first(0, None, None)
                .unwrap_or_else(|e| panic!("{e}"))
                .is_none()
        );
        let alive = std::process::id();
        ask(
            dir.path(),
            "ask.b",
            &question("/run/sck.b", &format!("PID={alive}\n")),
        );
        let found = watcher
            .first(0, None, None)
            .unwrap_or_else(|e| panic!("{e}"))
            .unwrap_or_else(|| panic!("no question"));
        assert_eq!(found.name.as_str(), "ask.b");
    }

    #[test]
    fn an_answered_file_is_skipped() {
        let (dir, mut watcher) = open();
        ask(dir.path(), "ask.a", &question("/run/sck.a", ""));
        ask(dir.path(), "ask.b", &question("/run/sck.b", ""));
        let done = Text::exact("ask.a");
        let found = watcher
            .first(0, done.as_ref(), None)
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
            .first(1_000, None, None)
            .unwrap_or_else(|e| panic!("{e}"))
            .unwrap_or_else(|| panic!("no question"));
        assert_eq!(found.name.as_str(), "ask.b");
        assert!(found.ask.socket.as_str().ends_with("/sck.b"));
    }

    #[test]
    fn a_socket_outside_the_directory_is_passed_over() {
        // Maintainer decision T-061: reply only inside the question directory.
        let (dir, mut watcher) = open();
        let outside = format!("{}/../sck.a", dir.path().display());
        ask(dir.path(), "ask.a", &question(&outside, ""));
        ask(dir.path(), "ask.b", &question("/tmp/elsewhere/sck.b", ""));
        let parent = format!("{}/..", dir.path().display());
        ask(dir.path(), "ask.c", &question(&parent, ""));
        assert!(
            watcher
                .first(0, None, None)
                .unwrap_or_else(|e| panic!("{e}"))
                .is_none()
        );
        ask(dir.path(), "ask.d", &question("/run/sck.d", ""));
        let found = watcher
            .first(0, None, None)
            .unwrap_or_else(|e| panic!("{e}"))
            .unwrap_or_else(|| panic!("no question"));
        assert_eq!(found.name.as_str(), "ask.d");
    }

    #[test]
    fn links_and_fifos_are_not_read() {
        let (dir, mut watcher) = open();
        // A target that would be a usable question if it were read through
        // the link.
        let target = dir.path().join("elsewhere");
        let socket = format!("{}/sck.1", dir.path().display());
        fs::write(&target, question(&socket, "")).unwrap_or_else(|e| panic!("{e}"));
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
                .first(0, None, None)
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
    fn a_removed_watch_is_a_change() {
        let base = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let dir = base.path().join("ask-password");
        let mut watcher = Watcher::open(&dir).unwrap_or_else(|e| panic!("{e}"));
        // The kernel drops the watch with IN_IGNORED, an event with no name.
        fs::remove_dir(&dir).unwrap_or_else(|e| panic!("{e}"));
        assert!(watcher.changed().unwrap_or_else(|e| panic!("{e}")));
    }

    /// Sets `path`'s modification time `offset_secs` from now.
    fn written(path: &Path, offset_secs: i64) {
        let now = rustix::time::clock_gettime(rustix::time::ClockId::Realtime);
        let then = rustix::time::Timespec {
            tv_sec: now.tv_sec + offset_secs,
            tv_nsec: now.tv_nsec,
        };
        let times = rustix::fs::Timestamps {
            last_access: then,
            last_modification: then,
        };
        rustix::fs::utimensat(rustix::fs::CWD, path, &times, rustix::fs::AtFlags::empty())
            .unwrap_or_else(|e| panic!("{e}"));
    }

    #[test]
    fn a_question_carries_how_long_its_file_waited() {
        // Verifies: FRN-SRS-020
        let (dir, mut watcher) = open();
        ask(dir.path(), "ask.1", &question("/run/sck.1", ""));
        let fresh = watcher
            .first(0, None, None)
            .unwrap_or_else(|e| panic!("{e}"))
            .and_then(|q| q.age_us)
            .unwrap_or_else(|| panic!("no age"));
        assert!(fresh < 5_000_000, "{fresh} µs for a file just written");
        written(&dir.path().join("ask.1"), -5);
        let old = watcher
            .first(0, None, None)
            .unwrap_or_else(|e| panic!("{e}"))
            .and_then(|q| q.age_us)
            .unwrap_or_else(|| panic!("no age"));
        assert!((5_000_000..60_000_000).contains(&old), "{old} µs");
        written(&dir.path().join("ask.1"), 3600);
        let future = watcher
            .first(0, None, None)
            .unwrap_or_else(|e| panic!("{e}"))
            .unwrap_or_else(|| panic!("the question is still shown"));
        assert_eq!(
            future.age_us, None,
            "a file newer than the clock has no age"
        );
    }

    #[test]
    fn debug_shows_the_directory_only() {
        let (dir, watcher) = open();
        let shown = format!("{watcher:?}");
        assert!(shown.starts_with("Watcher {"), "{shown}");
        assert!(shown.contains(&format!("{:?}", dir.path())), "{shown}");
    }

    #[test]
    fn the_clock_is_monotonic_microseconds() {
        let us = |t: rustix::time::Timespec| {
            u64::try_from(t.tv_sec).unwrap_or(0) * 1_000_000
                + u64::try_from(t.tv_nsec).unwrap_or(0) / 1_000
        };
        for _ in 0..3 {
            let before = us(rustix::time::clock_gettime(
                rustix::time::ClockId::Monotonic,
            ));
            let now = monotonic_us();
            let after = us(rustix::time::clock_gettime(
                rustix::time::ClockId::Monotonic,
            ));
            assert!(before <= now && now <= after, "{before} {now} {after}");
            std::thread::sleep(Duration::from_millis(3));
        }
    }

    #[test]
    fn the_clock_moves_forward() {
        let a = monotonic_us();
        std::thread::sleep(Duration::from_millis(2));
        assert!(monotonic_us() > a);
    }
}
