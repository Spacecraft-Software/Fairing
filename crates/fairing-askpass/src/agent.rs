// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! The password agent: questions in, keys in, one datagram out.
//!
//! [`Agent::tick`] is called once per frame by the splash loop and never
//! blocks. It drains the question directory's events, rescans when anything
//! changed (and four times a second regardless, so an expiry or a missed event
//! is still noticed), grabs the keyboards while a question is open and
//! releases them the tick it closes, and applies the keys typed since the last
//! tick: characters into the [`Secret`], Backspace, Enter to answer and Escape
//! to cancel. What the splash may draw comes out of [`Agent::view`], which
//! carries the number of characters typed and never the characters
//! themselves, except for a question asked with `Echo=1`.
//!
//! The secret is cleared on every path that ends a question (answer, cancel,
//! expiry, a question withdrawn or replaced, [`Agent::close`]), eagerly, not
//! through `Drop` (FRN-SRS-024).

use std::io;
use std::path::Path;

use crate::ask::{ID_CAPACITY, Text};
use crate::input::{Key, Keyboards};
use crate::reply;
use crate::secret::Secret;
use crate::watch::{NAME_CAPACITY, Question, Watcher};

/// How often the directory is rescanned with no event, in µs.
const RESCAN_US: u64 = 250_000;

/// How often keyboards are looked for again while a question is open, in µs:
/// a USB keyboard can enumerate after the question was asked.
const KEYBOARD_RESCAN_US: u64 = 1_000_000;

/// A repeated `Id=` this soon after a reply was a rejected answer, in µs
/// (FRN-SRS-029).
const REJECTION_WINDOW_US: u64 = 1_000_000;

/// Where keys come from: keyboards in the splash, a script in tests.
pub trait KeySource {
    /// Grabs every keyboard, or picks up new ones while grabbed; returns how
    /// many are held.
    fn grab(&mut self) -> usize;
    /// Releases every grab.
    fn release(&mut self);
    /// Whether a grab is held.
    fn grabbed(&self) -> bool;
    /// Passes every key typed since the last call to `f`.
    fn read(&mut self, f: &mut dyn FnMut(Key));
}

impl KeySource for Keyboards {
    fn grab(&mut self) -> usize {
        Self::grab(self)
    }

    fn release(&mut self) {
        Self::release(self);
    }

    fn grabbed(&self) -> bool {
        Self::grabbed(self)
    }

    fn read(&mut self, f: &mut dyn FnMut(Key)) {
        Self::read(self, f);
    }
}

/// What a tick did that is worth a journal line. Never carries the secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activity {
    /// Nothing worth reporting.
    Idle,
    /// A question opened; the number of keyboards grabbed for it.
    Opened(usize),
    /// The question was answered.
    Answered,
    /// The question was cancelled with Escape.
    Cancelled,
    /// The question's `NotAfter=` passed, or it was withdrawn.
    Closed,
    /// The reply could not be sent; the question is left to another agent.
    ReplyFailed(io::ErrorKind),
}

/// What the splash draws for the open question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct View<'a> {
    /// The question's `Message=`.
    pub message: &'a str,
    /// Characters typed.
    pub typed: usize,
    /// The input as typed, only for a question asked with `Echo=1`.
    pub echo: Option<&'a str>,
    /// Whether this question repeats one just answered: the answer was
    /// rejected (FRN-SRS-029).
    pub rejected: bool,
}

/// The agent's state between ticks.
///
/// Implements: FRN-SRS-020, FRN-SRS-022, FRN-SRS-023, FRN-SRS-024, FRN-SRS-028, FRN-SRS-029
pub struct Agent<K: KeySource> {
    watcher: Watcher,
    keys: K,
    secret: Secret,
    current: Option<Question>,
    rejected: bool,
    /// The file last answered or cancelled: systemd removes it once it has
    /// the reply, and until then it must not be asked again.
    done: Option<Text<NAME_CAPACITY>>,
    /// The `Id=` and time of the last answer.
    last_answer: Option<(Text<ID_CAPACITY>, u64)>,
    next_scan_us: u64,
    next_keyboard_scan_us: u64,
}

impl<K: KeySource> core::fmt::Debug for Agent<K> {
    /// The question's name and the count typed; never the secret.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Agent")
            .field("watcher", &self.watcher)
            .field("current", &self.current.map(|q| q.name))
            .field("secret", &self.secret)
            .finish_non_exhaustive()
    }
}

impl Agent<Keyboards> {
    /// The agent on `dir` (normally [`crate::watch::ASK_DIR`]) with the
    /// keyboards under `sysfs` and `dev`.
    ///
    /// Marks the process non-dumpable first, so no core dump or ptrace
    /// attach by an unprivileged process can read a secret (FRN-SRS-024).
    ///
    /// # Errors
    ///
    /// The error from `prctl` or from opening the watcher. The splash then
    /// leaves the question to systemd's console agent (FRN-SRS-027).
    pub fn open(dir: &Path, sysfs: &Path, dev: &Path) -> io::Result<Self> {
        rustix::process::set_dumpable_behavior(rustix::process::DumpableBehavior::NotDumpable)?;
        Ok(Self::with_keys(
            Watcher::open(dir)?,
            Keyboards::new(sysfs, dev),
        ))
    }
}

impl<K: KeySource> Agent<K> {
    /// The agent with an explicit watcher and key source.
    pub fn with_keys(watcher: Watcher, keys: K) -> Self {
        Self {
            watcher,
            keys,
            secret: Secret::new(),
            current: None,
            rejected: false,
            done: None,
            last_answer: None,
            next_scan_us: 0,
            next_keyboard_scan_us: 0,
        }
    }

    /// The key source.
    pub const fn keys(&self) -> &K {
        &self.keys
    }

    /// One frame at `now_us` (`CLOCK_MONOTONIC`, µs).
    ///
    /// # Errors
    ///
    /// The error from reading the question directory. The agent can no
    /// longer see questions; the caller closes it and steps aside.
    pub fn tick(&mut self, now_us: u64) -> io::Result<Activity> {
        let mut activity = Activity::Idle;
        if self.watcher.changed()? || now_us >= self.next_scan_us {
            self.next_scan_us = now_us.saturating_add(RESCAN_US);
            let found = self.watcher.first(now_us, self.done.as_ref())?;
            activity = self.show(found.as_ref(), now_us);
        }
        if self.current.is_some_and(|q| q.ask.expired(now_us)) {
            self.show(None, now_us);
            activity = Activity::Closed;
        }
        let Some(question) = self.current else {
            return Ok(activity);
        };
        if now_us >= self.next_keyboard_scan_us {
            self.next_keyboard_scan_us = now_us.saturating_add(KEYBOARD_RESCAN_US);
            let held = self.keys.grab();
            if let Activity::Opened(_) = activity {
                activity = Activity::Opened(held);
            }
        }
        let mut end = None;
        let secret = &mut self.secret;
        self.keys.read(&mut |key| {
            if end.is_some() {
                return;
            }
            match key {
                Key::Char(c) => {
                    secret.push(c);
                }
                Key::Backspace => secret.pop(),
                Key::Enter | Key::Escape => end = Some(key),
            }
        });
        let Some(key) = end else {
            return Ok(activity);
        };
        let socket = Path::new(question.ask.socket.as_str());
        let sent = if key == Key::Enter {
            reply::answer(socket, &mut self.secret)
        } else {
            reply::cancel(socket, &mut self.secret)
        };
        self.done = Some(question.name);
        self.show(None, now_us);
        Ok(match (sent, key) {
            (Err(e), _) => Activity::ReplyFailed(e.kind()),
            (Ok(()), Key::Enter) => {
                self.last_answer = Some((question.ask.id, now_us));
                Activity::Answered
            }
            (Ok(()), _) => Activity::Cancelled,
        })
    }

    /// Makes `found` the open question; clears the secret and grabs or
    /// releases the keyboards when that changes which question is open.
    fn show(&mut self, found: Option<&Question>, now_us: u64) -> Activity {
        if found.map(|q| q.name) == self.current.map(|q| q.name) {
            return Activity::Idle;
        }
        self.secret.clear();
        let was_open = self.current.is_some();
        self.current = found.copied();
        if let Some(question) = found {
            self.rejected = self.last_answer.is_some_and(|(id, at)| {
                id == question.ask.id && now_us.saturating_sub(at) <= REJECTION_WINDOW_US
            });
            self.next_keyboard_scan_us = now_us.saturating_add(KEYBOARD_RESCAN_US);
            return Activity::Opened(self.keys.grab());
        }
        self.rejected = false;
        self.keys.release();
        if was_open {
            Activity::Closed
        } else {
            Activity::Idle
        }
    }

    /// The open question as the splash draws it.
    #[must_use]
    pub fn view(&self) -> Option<View<'_>> {
        let question = self.current.as_ref()?;
        Some(View {
            message: question.ask.message.as_str(),
            typed: self.secret.chars(),
            echo: question.ask.echo.then(|| self.secret.as_str()),
            rejected: self.rejected,
        })
    }

    /// Ends the agent: clears the secret and releases every keyboard.
    pub fn close(&mut self) {
        self.secret.clear();
        self.current = None;
        self.keys.release();
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::fs;
    use std::os::unix::net::UnixDatagram;

    use super::*;

    /// Keys from a script, with the grab state recorded.
    #[derive(Default)]
    struct Script {
        pending: VecDeque<Key>,
        grabbed: bool,
        grabs: usize,
    }

    impl Script {
        fn typed(&mut self, text: &str) {
            self.pending.extend(text.chars().map(Key::Char));
        }
    }

    impl KeySource for Script {
        fn grab(&mut self) -> usize {
            if !self.grabbed {
                self.grabs += 1;
            }
            self.grabbed = true;
            1
        }

        fn release(&mut self) {
            self.grabbed = false;
        }

        fn grabbed(&self) -> bool {
            self.grabbed
        }

        fn read(&mut self, f: &mut dyn FnMut(Key)) {
            // Keys only arrive while grabbed, as with a real grab.
            if self.grabbed {
                self.pending.drain(..).for_each(f);
            }
        }
    }

    struct Fixture {
        dir: tempfile::TempDir,
        socket: UnixDatagram,
        socket_path: std::path::PathBuf,
        agent: Agent<Script>,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let socket_path = dir.path().join("sck.1");
        let socket = UnixDatagram::bind(&socket_path).unwrap_or_else(|e| panic!("{e}"));
        socket
            .set_nonblocking(true)
            .unwrap_or_else(|e| panic!("{e}"));
        let ask_dir = dir.path().join("ask-password");
        let watcher = Watcher::open(&ask_dir).unwrap_or_else(|e| panic!("{e}"));
        Fixture {
            dir,
            socket,
            socket_path,
            agent: Agent::with_keys(watcher, Script::default()),
        }
    }

    impl Fixture {
        fn ask(&self, name: &str, extra: &str) {
            let body = format!(
                "[Ask]\nSocket={}\nMessage=Passphrase for root\nId=cryptsetup:/dev/vda2\n{extra}",
                self.socket_path.display()
            );
            let dir = self.dir.path().join("ask-password");
            let tmp = dir.join(".tmp");
            fs::write(&tmp, body).unwrap_or_else(|e| panic!("{e}"));
            fs::rename(&tmp, dir.join(name)).unwrap_or_else(|e| panic!("{e}"));
        }

        fn withdraw(&self, name: &str) {
            fs::remove_file(self.dir.path().join("ask-password").join(name))
                .unwrap_or_else(|e| panic!("{e}"));
        }

        fn received(&self) -> Option<Vec<u8>> {
            let mut buf = [0u8; 1024];
            let n = self.socket.recv(&mut buf).ok()?;
            Some(buf[..n].to_vec())
        }

        fn tick(&mut self, now_us: u64) -> Activity {
            self.agent.tick(now_us).unwrap_or_else(|e| panic!("{e}"))
        }
    }

    #[test]
    fn a_question_is_shown_grabbed_typed_and_answered() {
        // Verifies: FRN-SRS-021, FRN-SRS-022, FRN-SRS-028
        let mut f = fixture();
        assert_eq!(f.tick(1), Activity::Idle);
        assert!(!f.agent.keys().grabbed(), "no grab without a question");
        f.ask("ask.1", "");
        assert_eq!(f.tick(2), Activity::Opened(1));
        assert!(f.agent.keys().grabbed());
        let view = f.agent.view().unwrap_or_else(|| panic!("no view"));
        assert_eq!(view.message, "Passphrase for root");
        assert_eq!((view.typed, view.echo, view.rejected), (0, None, false));

        f.agent.keys.typed("hunter22");
        f.agent.keys.pending.push_back(Key::Backspace);
        f.tick(3);
        let view = f.agent.view().unwrap_or_else(|| panic!("no view"));
        assert_eq!(view.typed, 7);
        assert_eq!(view.echo, None, "a masked field never carries the text");

        f.agent.keys.pending.push_back(Key::Enter);
        assert_eq!(f.tick(4), Activity::Answered);
        assert_eq!(f.received().as_deref(), Some(&b"+hunter2"[..]));
        // Released in the same tick the prompt closed, well within 50 ms.
        assert!(!f.agent.keys().grabbed());
        assert!(f.agent.view().is_none());
        // systemd has not removed the file yet: it is not asked again.
        assert_eq!(f.tick(1_000_000), Activity::Idle);
        assert!(f.agent.view().is_none());
    }

    #[test]
    fn escape_cancels() {
        // Verifies: FRN-SRS-022
        let mut f = fixture();
        f.ask("ask.1", "");
        f.tick(1);
        f.agent.keys.typed("abc");
        f.agent.keys.pending.push_back(Key::Escape);
        assert_eq!(f.tick(2), Activity::Cancelled);
        assert_eq!(f.received().as_deref(), Some(&b"-"[..]));
        assert!(!f.agent.keys().grabbed());
    }

    #[test]
    fn echo_shows_the_text() {
        // Verifies: FRN-SRS-026
        let mut f = fixture();
        f.ask("ask.1", "Echo=1\n");
        f.tick(1);
        f.agent.keys.typed("root");
        f.tick(2);
        let view = f.agent.view().unwrap_or_else(|| panic!("no view"));
        assert_eq!(view.echo, Some("root"));
    }

    #[test]
    fn the_prompt_closes_at_not_after_and_the_secret_is_cleared() {
        // Verifies: FRN-SRS-023, FRN-SRS-024
        let mut f = fixture();
        f.ask("ask.1", "NotAfter=5000\n");
        f.tick(1_000);
        f.agent.keys.typed("partial");
        f.tick(2_000);
        assert_eq!(f.agent.view().map(|v| v.typed), Some(7));
        assert_eq!(f.tick(5_000), Activity::Closed);
        assert!(f.agent.view().is_none());
        assert!(f.agent.secret.is_empty(), "cleared at expiry");
        assert!(!f.agent.keys().grabbed());
        assert_eq!(f.received(), None, "an expired question gets no reply");
    }

    #[test]
    fn a_withdrawn_question_closes_the_prompt() {
        let mut f = fixture();
        f.ask("ask.1", "");
        f.tick(1);
        f.agent.keys.typed("abc");
        f.tick(2);
        f.withdraw("ask.1");
        assert_eq!(f.tick(3), Activity::Closed);
        assert!(f.agent.secret.is_empty());
        assert!(!f.agent.keys().grabbed());
    }

    #[test]
    fn a_repeated_id_within_a_second_is_a_rejection() {
        // Verifies: FRN-SRS-029
        let mut f = fixture();
        f.ask("ask.1", "");
        f.tick(1_000);
        f.agent.keys.typed("wrong");
        f.agent.keys.pending.push_back(Key::Enter);
        assert_eq!(f.tick(2_000), Activity::Answered);
        f.withdraw("ask.1");
        f.ask("ask.2", "");
        assert_eq!(f.tick(500_000), Activity::Opened(1));
        assert!(f.agent.view().is_some_and(|v| v.rejected));
        assert_eq!(f.agent.keys().grabs, 2, "grabbed again for the new prompt");

        // The same Id= long after the answer is a new question, not a rejection.
        f.agent.keys.pending.push_back(Key::Enter);
        f.tick(600_000);
        f.withdraw("ask.2");
        f.ask("ask.3", "");
        f.tick(5_000_000);
        assert!(f.agent.view().is_some_and(|v| !v.rejected));
    }

    #[test]
    fn close_clears_and_releases() {
        // Verifies: FRN-SRS-024
        let mut f = fixture();
        f.ask("ask.1", "");
        f.tick(1);
        f.agent.keys.typed("abc");
        f.tick(2);
        f.agent.close();
        assert!(f.agent.secret.is_empty());
        assert!(!f.agent.keys().grabbed());
        assert!(!format!("{:?}", f.agent).contains("abc"));
    }

    #[test]
    fn a_reply_that_cannot_be_sent_is_reported_and_cleared() {
        let mut f = fixture();
        f.ask("ask.1", "");
        f.tick(1);
        fs::remove_file(&f.socket_path).unwrap_or_else(|e| panic!("{e}"));
        f.agent.keys.typed("abc");
        f.agent.keys.pending.push_back(Key::Enter);
        assert!(matches!(f.tick(2), Activity::ReplyFailed(_)));
        assert!(f.agent.secret.is_empty());
        assert!(!f.agent.keys().grabbed());
    }
}
