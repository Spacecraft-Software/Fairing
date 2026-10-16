// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! The journal, as the overlay reads it: `journalctl -o json --follow`.
//!
//! A child `journalctl` does the reading, in the initrd as in stage 2, so
//! Fairing links no journal library and parses no journal file. It starts
//! with the boot's last 40 entries at priority notice or higher and then
//! follows the journal; its standard output is a pipe the splash loop reads
//! without blocking once per frame, so a new entry is on screen a frame after
//! journalctl writes it (FRN-SRS-062). Lines are bounded: one longer than
//! [`MAX_LINE`] is dropped whole, and at most [`READ_BUDGET`] bytes are read
//! per frame.

use std::io::{self, Read as _};
use std::os::fd::AsFd as _;
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Stdio};

use rustix::fs::OFlags;
use serde_json::Value;

/// The `journalctl` the splash runs.
pub const JOURNALCTL: &str = "journalctl";

/// Entries the overlay shows (FRN-SRS-060).
pub const ENTRIES: usize = 40;

/// The longest journal line kept, in bytes; longer ones are dropped.
pub const MAX_LINE: usize = 64 * 1024;

/// The most bytes read from journalctl in one frame.
pub const READ_BUDGET: usize = 64 * 1024;

/// One journal entry, as the overlay shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Syslog priority, 0 (emerg) to 7 (debug).
    pub priority: u8,
    /// Microseconds since boot (`__MONOTONIC_TIMESTAMP`).
    pub monotonic_us: Option<u64>,
    /// `SYSLOG_IDENTIFIER`, else `_COMM`.
    pub identifier: Option<String>,
    /// `MESSAGE`.
    pub message: String,
}

impl Entry {
    /// Parses one line of `journalctl -o json`; `None` for anything that is
    /// not an entry with a message.
    #[must_use]
    pub fn parse(line: &[u8]) -> Option<Self> {
        let value: Value = serde_json::from_slice(line).ok()?;
        let field = |name: &str| value.get(name);
        let text = |name: &str| field(name).and_then(Value::as_str).map(str::to_owned);
        let message = match field("MESSAGE")? {
            Value::String(text) => text.clone(),
            // A field given twice is an array of its values: the first.
            Value::Array(values) if values.first().is_some_and(Value::is_string) => {
                values.first().and_then(Value::as_str)?.to_owned()
            }
            // A message that is not UTF-8 is an array of bytes.
            Value::Array(bytes) => {
                let bytes: Vec<u8> = bytes
                    .iter()
                    .filter_map(|b| b.as_u64().and_then(|b| u8::try_from(b).ok()))
                    .collect();
                String::from_utf8_lossy(&bytes).into_owned()
            }
            _ => return None,
        };
        Some(Self {
            priority: text("PRIORITY").and_then(|p| p.parse().ok()).unwrap_or(6),
            monotonic_us: text("__MONOTONIC_TIMESTAMP").and_then(|t| t.parse().ok()),
            identifier: text("SYSLOG_IDENTIFIER").or_else(|| text("_COMM")),
            message,
        })
    }

    /// One line of plain text: seconds since boot, the severity tag, the
    /// identifier and the message, e.g. `   3.214 [WARN] systemd-fsck: ...`.
    /// The tag carries the severity, so it never depends on colour.
    #[must_use]
    pub fn line(&self) -> String {
        let tag = match self.priority {
            0..=3 => "[ERROR]",
            4 => "[WARN]",
            _ => "[INFO]",
        };
        let seconds = self.monotonic_us.map_or_else(
            || "       -".to_owned(),
            |us| {
                format!(
                    "{:>8}",
                    format!("{}.{:03}", us / 1_000_000, us / 1000 % 1000)
                )
            },
        );
        let message = fairing_render::plain(self.message.trim_end());
        match &self.identifier {
            Some(identifier) => format!(
                "{seconds} {tag} {}: {message}",
                fairing_render::plain(identifier)
            ),
            None => format!("{seconds} {tag} {message}"),
        }
    }
}

/// A running `journalctl --follow`.
#[derive(Debug)]
pub struct Journal {
    child: Child,
    stdout: ChildStdout,
    pending: Vec<u8>,
    /// Inside a line longer than [`MAX_LINE`], dropped up to its newline.
    skipping: bool,
}

impl Journal {
    /// Starts `program` (normally [`JOURNALCTL`]) on this boot's entries at
    /// priority notice or higher, the last [`ENTRIES`] first.
    ///
    /// # Errors
    ///
    /// The program cannot be started, or its output made non-blocking.
    pub fn spawn(program: &Path) -> io::Result<Self> {
        let mut child = Command::new(program)
            .args([
                "--output=json",
                "--follow",
                "--boot",
                "--priority=notice",
                "--lines=40",
                "--no-pager",
                "--quiet",
                // Without it, journalctl writes a field of 4 KiB or more as
                // null; MAX_LINE bounds the line instead.
                "--all",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let Some(stdout) = child.stdout.take() else {
            stop(&mut child);
            return Err(io::Error::other("journalctl has no standard output"));
        };
        let nonblocking = rustix::fs::fcntl_getfl(stdout.as_fd())
            .and_then(|flags| rustix::fs::fcntl_setfl(stdout.as_fd(), flags | OFlags::NONBLOCK));
        if let Err(error) = nonblocking {
            stop(&mut child);
            return Err(error.into());
        }
        Ok(Self {
            child,
            stdout,
            pending: Vec::new(),
            skipping: false,
        })
    }

    /// Passes every whole entry written since the last call to `f`, reading
    /// at most [`READ_BUDGET`] bytes. `Ok(false)` once journalctl has exited.
    ///
    /// # Errors
    ///
    /// A read error other than "nothing to read".
    ///
    /// Implements: FRN-SRS-062
    pub fn read(&mut self, mut f: impl FnMut(Entry)) -> io::Result<bool> {
        let mut buf = [0u8; 8192];
        let mut budget = READ_BUDGET;
        let alive = loop {
            if budget == 0 {
                break true;
            }
            let want = buf.len().min(budget);
            match self.stdout.read(&mut buf[..want]) {
                Ok(0) => break false,
                Ok(n) => {
                    budget -= n;
                    self.take(&buf[..n], &mut f);
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break true,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        };
        Ok(alive)
    }

    /// Splits `bytes` into lines, keeping a partial last line for later.
    fn take(&mut self, bytes: &[u8], f: &mut impl FnMut(Entry)) {
        for part in bytes.split_inclusive(|&b| b == b'\n') {
            let ends = part.last() == Some(&b'\n');
            if self.skipping {
                self.skipping = !ends;
                continue;
            }
            if self.pending.len() + part.len() > MAX_LINE {
                self.pending.clear();
                self.skipping = !ends;
                continue;
            }
            self.pending.extend_from_slice(part);
            if ends {
                if let Some(entry) = Entry::parse(&self.pending) {
                    f(entry);
                }
                self.pending.clear();
            }
        }
    }

    /// Stops journalctl and reaps it.
    pub fn stop(mut self) {
        stop(&mut self.child);
    }
}

/// Kills and reaps `child`; it may have exited already.
fn stop(child: &mut Child) {
    let _killed = child.kill();
    let _reaped = child.wait();
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    const ENTRY: &str = r#"{"__MONOTONIC_TIMESTAMP":"3214567","PRIORITY":"4","SYSLOG_IDENTIFIER":"systemd-fsck","MESSAGE":"/dev/vda1: clean"}"#;

    #[test]
    fn entries_become_one_plain_line_with_a_tag() {
        let entry = Entry::parse(ENTRY.as_bytes()).unwrap_or_else(|| panic!("no entry"));
        assert_eq!(
            entry.line(),
            "   3.214 [WARN] systemd-fsck: /dev/vda1: clean"
        );
        let raw = r#"{"PRIORITY":"2","_COMM":"x","MESSAGE":[104,105,27,91,49,109]}"#;
        let entry = Entry::parse(raw.as_bytes()).unwrap_or_else(|| panic!("no entry"));
        assert_eq!(
            entry.line(),
            "       - [ERROR] x: hi [1m",
            "no escape survives"
        );
        let notice = r#"{"PRIORITY":"5","MESSAGE":"Reached target"}"#;
        let entry = Entry::parse(notice.as_bytes()).unwrap_or_else(|| panic!("no entry"));
        assert_eq!(entry.line(), "       - [INFO] Reached target");
        assert_eq!(Entry::parse(b"{\"MESSAGE\":null}"), None);
        let twice = r#"{"PRIORITY":"5","MESSAGE":["first","second"]}"#;
        let entry = Entry::parse(twice.as_bytes()).unwrap_or_else(|| panic!("no entry"));
        assert_eq!(entry.line(), "       - [INFO] first");
        assert_eq!(Entry::parse(b"not json"), None);
    }

    /// A journalctl stand-in: a script that prints `lines`, then sleeps.
    fn fake(dir: &Path, script: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt as _;
        let path = dir.join("journalctl");
        std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap_or_else(|e| panic!("{e}"));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .unwrap_or_else(|e| panic!("{e}"));
        path
    }

    #[test]
    fn a_new_entry_is_read_within_250_ms_of_its_arrival() {
        // Verifies: FRN-SRS-062
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let marker = dir.path().join("written");
        let program = fake(
            dir.path(),
            &format!(
                "sleep 0.3\necho '{ENTRY}'\ntouch {}\nsleep 5",
                marker.display()
            ),
        );
        let mut journal = Journal::spawn(&program).unwrap_or_else(|e| panic!("{e}"));
        let mut entries = Vec::new();
        let mut written = None;
        let deadline = Instant::now() + Duration::from_secs(5);
        // The splash's frame period.
        while entries.is_empty() && Instant::now() < deadline {
            if written.is_none() && marker.exists() {
                written = Some(Instant::now());
            }
            assert!(journal.read(|e| entries.push(e)).unwrap_or(false));
            std::thread::sleep(Duration::from_millis(33));
        }
        let seen = Instant::now();
        journal.stop();
        assert_eq!(entries.len(), 1);
        // The marker is touched after the line is written, so this bounds
        // the delay from above.
        let delay = written.map_or(Duration::ZERO, |w| seen.saturating_duration_since(w));
        assert!(delay < Duration::from_millis(250), "{delay:?}");
    }

    const HEAD: &str = r#"{"PRIORITY":"5","MESSAGE":""#;
    const TAIL: &str = "\"}\n";

    /// An entry line of exactly `len` bytes, newline included.
    fn entry_of(len: usize) -> Vec<u8> {
        let pad = len - HEAD.len() - TAIL.len();
        format!("{HEAD}{}{TAIL}", "m".repeat(pad)).into_bytes()
    }

    /// A journal whose journalctl prints nothing; lines are fed with `take`.
    fn idle(dir: &Path) -> Journal {
        Journal::spawn(&fake(dir, "exec sleep 30")).unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn a_line_of_the_limit_is_kept_and_one_byte_more_is_dropped() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let mut journal = idle(dir.path());
        let mut entries = Vec::new();
        let mut f = |e: Entry| entries.push(e.message.len());
        // Well over a kilobyte, well under the limit, in one piece.
        journal.take(&entry_of(4096), &mut f);
        // Exactly the limit, in two pieces.
        let exact = entry_of(MAX_LINE);
        let (first, second) = exact.split_at(MAX_LINE / 2);
        journal.take(first, &mut f);
        journal.take(second, &mut f);
        // One byte over, in one piece and in two.
        let over = entry_of(MAX_LINE + 1);
        journal.take(&over, &mut f);
        let (first, second) = over.split_at(100);
        journal.take(first, &mut f);
        journal.take(second, &mut f);
        // And one after it, to show nothing was left pending.
        journal.take(ENTRY.as_bytes(), &mut f);
        journal.take(b"\n", &mut f);
        journal.stop();
        let overhead = HEAD.len() + TAIL.len();
        assert_eq!(
            entries,
            [
                4096 - overhead,
                MAX_LINE - overhead,
                "/dev/vda1: clean".len()
            ]
        );
    }

    #[test]
    fn an_oversized_line_is_skipped_to_its_newline_however_it_arrives() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let mut journal = idle(dir.path());
        let mut entries = Vec::new();
        let mut f = |e: Entry| entries.push(e);
        journal.take("x".repeat(MAX_LINE + 1).as_bytes(), &mut f);
        // The rest of the long line, in pieces, one of which is an entry.
        journal.take(b"still the long line ", &mut f);
        journal.take(ENTRY.as_bytes(), &mut f);
        journal.take(b"\n", &mut f);
        // The next line is read again.
        journal.take(format!("{ENTRY}\n").as_bytes(), &mut f);
        journal.stop();
        assert_eq!(
            entries.len(),
            1,
            "only the line after the long one: {entries:?}"
        );
    }

    #[test]
    fn one_read_takes_exactly_its_budget() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let marker = dir.path().join("written");
        // Exactly the budget, which a pipe holds, then the end of output.
        let program = fake(
            dir.path(),
            &format!(
                "printf '%s' '{}'\ntouch {}",
                // The budget, written out: 64 KiB.
                "x".repeat(64 * 1024),
                marker.display()
            ),
        );
        let mut journal = Journal::spawn(&program).unwrap_or_else(|e| panic!("{e}"));
        let deadline = Instant::now() + Duration::from_secs(5);
        while journal.child.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(marker.exists(), "all written");
        let mut ignore = |_e: Entry| {};
        assert!(
            journal.read(&mut ignore).unwrap_or(false),
            "the budget is spent before the end is seen"
        );
        assert!(!journal.read(&mut ignore).unwrap_or(true), "then the end");
        journal.stop();
    }

    #[test]
    fn stopping_kills_and_reaps_journalctl() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let program = fake(dir.path(), "exec sleep 30");
        let journal = Journal::spawn(&program).unwrap_or_else(|e| panic!("{e}"));
        let pid = journal.child.id();
        let proc = std::path::PathBuf::from(format!("/proc/{pid}"));
        assert!(proc.exists());
        journal.stop();
        assert!(!proc.exists(), "killed and reaped");
    }

    #[test]
    fn partial_and_oversized_lines_are_handled_and_exit_is_seen() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let long = "x".repeat(MAX_LINE + 10);
        let program = fake(
            dir.path(),
            &format!("printf '%s\\n' '{long}'\nprintf '%s' '{ENTRY}'\nsleep 0.2\necho"),
        );
        let mut journal = Journal::spawn(&program).unwrap_or_else(|e| panic!("{e}"));
        let mut entries = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut alive = true;
        while alive && Instant::now() < deadline {
            alive = journal.read(|e| entries.push(e)).unwrap_or(false);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!alive, "the exit is seen");
        assert_eq!(
            entries.len(),
            1,
            "the long line is dropped, the split one kept"
        );
        journal.stop();
        assert!(Journal::spawn(&dir.path().join("missing")).is_err());
    }
}
