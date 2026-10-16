// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Where the text backend writes: the console, or stderr under an agent.
//!
//! Accessible mode replaces the graphics with lines of text on
//! `/dev/console`, which a screen reader, brltty or a serial console reads
//! (FRN-SRS-070, FRN-SRS-071). The console is opened without becoming the
//! process's controlling terminal and without blocking: a serial console held
//! up by flow control costs a line, never a frame or the SIGTERM bound.

use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use rustix::fs::{Mode, OFlags};

/// The kernel's console.
pub const CONSOLE: &str = "/dev/console";

/// Where the text backend's lines go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextTarget {
    /// A console device (normally [`CONSOLE`]) or, in tests, a file.
    Console(PathBuf),
    /// stderr: an agent or CI harness, where no console is touched.
    Stderr,
}

impl TextTarget {
    /// The target for `console` given the harness: under an agent or CI
    /// harness the real console is never written, and stderr stands in.
    #[must_use]
    pub fn choose(console: &Path, agent_environment: bool) -> Self {
        if agent_environment && console == Path::new(CONSOLE) {
            Self::Stderr
        } else {
            Self::Console(console.to_path_buf())
        }
    }
}

/// An open text target.
#[derive(Debug)]
pub enum Sink {
    /// The console device or file.
    File(File),
    /// stderr.
    Stderr(io::Stderr),
}

impl Sink {
    /// Opens `target` for writing: append-only, no controlling terminal,
    /// non-blocking.
    ///
    /// # Errors
    ///
    /// The console cannot be opened.
    pub fn open(target: &TextTarget) -> io::Result<Self> {
        match target {
            TextTarget::Stderr => Ok(Self::Stderr(io::stderr())),
            TextTarget::Console(path) => {
                let fd = rustix::fs::open(
                    path,
                    OFlags::WRONLY
                        | OFlags::APPEND
                        | OFlags::NOCTTY
                        | OFlags::NONBLOCK
                        | OFlags::CLOEXEC,
                    Mode::empty(),
                )?;
                Ok(Self::File(File::from(fd)))
            }
        }
    }
}

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::File(file) => file.write(buf),
            Self::Stderr(stderr) => stderr.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::File(file) => file.flush(),
            Self::Stderr(stderr) => stderr.flush(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agents_never_write_the_real_console() {
        let console = Path::new(CONSOLE);
        assert_eq!(TextTarget::choose(console, true), TextTarget::Stderr);
        assert_eq!(
            TextTarget::choose(console, false),
            TextTarget::Console(console.to_path_buf())
        );
        let file = Path::new("/tmp/console");
        assert_eq!(
            TextTarget::choose(file, true),
            TextTarget::Console(file.to_path_buf())
        );
    }

    #[test]
    fn the_console_is_appended_to_and_never_created() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let path = dir.path().join("console");
        assert!(Sink::open(&TextTarget::Console(path.clone())).is_err());
        std::fs::write(&path, b"before\n").unwrap_or_else(|e| panic!("{e}"));
        let mut sink =
            Sink::open(&TextTarget::Console(path.clone())).unwrap_or_else(|e| panic!("{e}"));
        sink.write_all(b"after\n").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap_or_default(),
            "before\nafter\n"
        );
    }
}
