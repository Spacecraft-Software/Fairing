// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! The text backend: the splash as plain console lines, for accessible mode.
//!
//! A screen reader, a braille display through brltty and a serial console
//! all read text, not pixels, and re-read whatever is redrawn. So the text
//! backend draws nothing: it writes one line, `Fairing <NN>% <status>`, when
//! the progress or the status changes, at most once a second, and a password
//! question as the plain text of its message. Every line is plain text with
//! every control character, escape included, turned into a space, so no
//! colour or cursor sequence reaches the console, whatever a unit description
//! or a question's message carries (FRN-SRS-071, FRN-SRS-073).
//!
//! Nothing is written while a question is open but the question itself: a
//! progress line read out while someone types a passphrase is noise. What is
//! typed is never written, not even as a count.

use std::io::{self, Write};
use std::time::Duration;

use crate::scene::Scene;

/// The shortest time between two progress lines (FRN-SRS-071).
pub const TEXT_INTERVAL: Duration = Duration::from_secs(1);

/// Writes scenes to a console as lines of plain text.
#[derive(Debug)]
pub struct TextConsole<W> {
    out: W,
    /// The last progress line written, and when.
    last: Option<(String, Duration)>,
    /// The message and notice of the open question, as written.
    prompt: Option<(String, Option<String>)>,
    /// The part of the last line the console has not taken yet. A
    /// non-blocking write can take part of a line; the rest goes out before
    /// anything else, so no line is ever broken or written twice.
    pending: Vec<u8>,
    lines: u64,
    /// Bytes the console has taken.
    written: u64,
}

impl<W: Write> TextConsole<W> {
    /// A console writing to `out`; nothing is written until the first scene.
    #[must_use]
    pub const fn new(out: W) -> Self {
        Self {
            out,
            last: None,
            prompt: None,
            pending: Vec::new(),
            lines: 0,
            written: 0,
        }
    }

    /// Writes what `scene` changes at `now` (any monotonic time): a question
    /// that opened, or a notice that appeared beside it, at once; otherwise
    /// the progress line, if it changed and a second has passed since the
    /// last one. Returns how many lines were written.
    ///
    /// A console that is not ready (`WouldBlock`) loses nothing: the line is
    /// written on a later call.
    ///
    /// # Errors
    ///
    /// A write error other than `WouldBlock`.
    ///
    /// Implements: FRN-SRS-071, FRN-SRS-073
    pub fn present(&mut self, scene: &Scene, now: Duration) -> io::Result<u32> {
        if !self.flush_pending()? {
            return Ok(0);
        }
        let Some(prompt) = scene.prompt() else {
            self.prompt = None;
            return self.progress(scene, now);
        };
        let message = plain(&prompt.message);
        let notice = prompt.notice.as_deref().map(plain);
        let mut written = 0;
        if self.prompt.as_ref().is_none_or(|(m, _)| *m != message) {
            if !self.line(&message)? {
                return Ok(0);
            }
            written += 1;
            self.prompt = Some((message, None));
        }
        if let Some(notice) = notice {
            let shown = self.prompt.as_ref().and_then(|(_, n)| n.as_deref());
            if shown != Some(notice.as_str()) {
                if !self.line(&notice)? {
                    return Ok(written);
                }
                written += 1;
                if let Some((_, shown)) = self.prompt.as_mut() {
                    *shown = Some(notice);
                }
            }
        }
        Ok(written)
    }

    /// Whether the open question's message has reached the console whole.
    /// A console that takes nothing (a tty stopped with Scroll Lock) leaves
    /// the question unseen, which the splash must not wait on for ever.
    #[must_use]
    pub fn prompt_written(&self) -> bool {
        self.prompt.is_some() && self.pending.is_empty()
    }

    /// Bytes the console has taken so far.
    #[must_use]
    pub const fn bytes_written(&self) -> u64 {
        self.written
    }

    /// Lines written so far, a line the console has taken only part of
    /// included.
    #[must_use]
    pub const fn lines(&self) -> u64 {
        self.lines
    }

    /// The writer.
    pub fn into_inner(self) -> W {
        self.out
    }

    /// The progress line, if it changed and the interval has passed.
    fn progress(&mut self, scene: &Scene, now: Duration) -> io::Result<u32> {
        let line = progress_line(scene);
        if let Some((last, at)) = &self.last
            && (*last == line || now.saturating_sub(*at) < TEXT_INTERVAL)
        {
            return Ok(0);
        }
        if !self.line(&line)? {
            return Ok(0);
        }
        self.last = Some((line, now));
        Ok(1)
    }

    /// Writes `text` and a newline; `false` when the console is not ready
    /// for a new line. A line the console takes only part of counts as
    /// written: its tail goes out first on later calls.
    fn line(&mut self, text: &str) -> io::Result<bool> {
        if !self.flush_pending()? {
            return Ok(false);
        }
        self.pending.extend_from_slice(text.as_bytes());
        self.pending.push(b'\n');
        self.lines += 1;
        self.flush_pending()?;
        Ok(true)
    }

    /// Writes what is left of the last line; `true` once nothing is left.
    fn flush_pending(&mut self) -> io::Result<bool> {
        while !self.pending.is_empty() {
            match self.out.write(&self.pending) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(n) => {
                    self.pending.drain(..n);
                    self.written += u64::try_from(n).unwrap_or(u64::MAX);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(false),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
        match self.out.flush() {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(false),
            Err(error) => Err(error),
        }
    }
}

/// `Fairing <NN>% <status>`, or `Fairing <NN>%` with no status; a scene
/// without progress, the shutdown's, is `Fairing <status>`.
///
/// Implements: FRN-SRS-071
#[must_use]
pub fn progress_line(scene: &Scene) -> String {
    let status = scene.status().map(str::trim);
    match (scene.shows_progress(), status) {
        (true, Some(status)) => plain(&format!("Fairing {}% {status}", scene.percent())),
        (true, None) => format!("Fairing {}%", scene.percent()),
        (false, Some(status)) => plain(&format!("Fairing {status}")),
        (false, None) => "Fairing".to_owned(),
    }
}

/// `text` with every control character (C0, DEL and C1, escape and CSI
/// among them) turned into a space, and runs of spaces kept as they are.
///
/// Implements: FRN-SRS-073
#[must_use]
pub fn plain(text: &str) -> String {
    text.chars()
        .map(|ch| {
            if ch.is_control() || reorders(ch) {
                ' '
            } else {
                ch
            }
        })
        .collect()
}

/// Line and paragraph separators and the bidirectional controls: not
/// escape sequences, but they break a line or reorder it, and a screen
/// reader reads the result in an order nobody wrote.
const fn reorders(ch: char) -> bool {
    matches!(
        ch,
        '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{2028}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Field, PromptView};

    fn secs(s: f64) -> Duration {
        Duration::from_secs_f64(s)
    }

    fn written(console: &TextConsole<Vec<u8>>) -> String {
        String::from_utf8_lossy(&console.out).into_owned()
    }

    #[test]
    fn progress_is_one_line_a_second_at_most() {
        // Verifies: FRN-SRS-071
        let mut console = TextConsole::new(Vec::new());
        let scene = |p: u8| Scene::new(p).with_status("Mounting /home");
        assert_eq!(console.present(&scene(3), secs(0.0)).ok(), Some(1));
        // Changed, but within the second.
        assert_eq!(console.present(&scene(4), secs(0.5)).ok(), Some(0));
        assert_eq!(console.present(&scene(5), secs(1.0)).ok(), Some(1));
        // Unchanged, however long after.
        assert_eq!(console.present(&scene(5), secs(9.0)).ok(), Some(0));
        assert_eq!(console.present(&Scene::new(6), secs(9.5)).ok(), Some(1));
        assert_eq!(
            written(&console),
            "Fairing 3% Mounting /home\nFairing 5% Mounting /home\nFairing 6%\n"
        );
        assert_eq!(console.lines(), 3);
    }

    #[test]
    fn the_shutdown_line_has_no_percentage() {
        let scene = Scene::new(0)
            .without_progress()
            .with_status("Shutting down");
        assert_eq!(progress_line(&scene), "Fairing Shutting down");
        assert_eq!(progress_line(&Scene::new(0).without_progress()), "Fairing");
    }

    #[test]
    fn nothing_but_plain_text_reaches_the_console() {
        // Verifies: FRN-SRS-073
        let mut console = TextConsole::new(Vec::new());
        let hostile = "\u{1b}[31mRed\u{1b}[0m \u{9b}2J\u{7}done\r\u{202e}gro\u{2028}";
        let scene = Scene::new(10).with_status(hostile);
        console
            .present(&scene, secs(0.0))
            .unwrap_or_else(|e| panic!("{e}"));
        let prompt = PromptView {
            message: format!("{hostile} passphrase"),
            field: Field::Masked(3),
            notice: Some("\u{1b}[2KNot accepted".to_owned()),
        };
        console
            .present(&Scene::new(10).with_prompt(Some(prompt)), secs(2.0))
            .unwrap_or_else(|e| panic!("{e}"));
        let out = written(&console);
        assert!(
            out.chars()
                .all(|ch| ch == '\n' || !(ch.is_control() || reorders(ch))),
            "{out:?}"
        );
        assert!(out.contains("Fairing 10%"), "{out:?}");
        assert!(out.contains("Not accepted"), "{out:?}");
    }

    #[test]
    fn a_question_is_written_once_and_its_input_never() {
        // Verifies: FRN-SRS-071
        let mut console = TextConsole::new(Vec::new());
        let ask = |typed: usize, notice: Option<&str>| {
            Scene::new(20).with_prompt(Some(PromptView {
                message: "Please enter passphrase for disk root:".to_owned(),
                field: Field::Masked(typed),
                notice: notice.map(str::to_owned),
            }))
        };
        for (i, typed) in (0..5).enumerate() {
            console
                .present(
                    &ask(typed, None),
                    secs(0.1 * f64::from(u8::try_from(i).unwrap_or(0))),
                )
                .unwrap_or_else(|e| panic!("{e}"));
        }
        // The same question again, rejected: only the notice is new.
        console
            .present(&ask(0, Some("Not accepted. Please try again.")), secs(1.0))
            .unwrap_or_else(|e| panic!("{e}"));
        console
            .present(&ask(2, Some("Not accepted. Please try again.")), secs(1.1))
            .unwrap_or_else(|e| panic!("{e}"));
        // Closed: progress resumes.
        console
            .present(&Scene::new(25), secs(3.0))
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            written(&console),
            "Please enter passphrase for disk root:\nNot accepted. Please try again.\nFairing 25%\n"
        );
    }

    /// A console that is busy for the first `busy` writes.
    struct Busy {
        busy: u32,
        out: Vec<u8>,
    }

    impl Write for Busy {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if self.busy > 0 {
                self.busy -= 1;
                return Err(io::ErrorKind::WouldBlock.into());
            }
            self.out.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// A console that takes `take` bytes, then refuses `busy` writes.
    struct Short {
        take: usize,
        busy: u32,
        out: Vec<u8>,
    }

    impl Write for Short {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if self.take == 0 && self.busy > 0 {
                self.busy -= 1;
                return Err(io::ErrorKind::WouldBlock.into());
            }
            let n = if self.take > 0 {
                buf.len().min(std::mem::take(&mut self.take))
            } else {
                buf.len()
            };
            self.out.extend_from_slice(&buf[..n]);
            Ok(n)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_line_cut_short_is_finished_never_repeated() {
        // Verifies: FRN-SRS-071
        let mut console = TextConsole::new(Short {
            take: 5,
            busy: 2,
            out: Vec::new(),
        });
        let scene = |p: u8| Scene::new(p).with_status("Mounting /home");
        assert_eq!(console.present(&scene(3), secs(0.0)).ok(), Some(1));
        // Still busy: neither the tail nor a new line goes out.
        assert_eq!(console.present(&scene(5), secs(1.0)).ok(), Some(0));
        assert_eq!(console.present(&scene(6), secs(2.0)).ok(), Some(1));
        assert_eq!(
            String::from_utf8_lossy(&console.into_inner().out),
            "Fairing 3% Mounting /home\nFairing 6% Mounting /home\n"
        );
    }

    #[test]
    fn an_unwritten_question_is_known_as_such() {
        let mut console = TextConsole::new(Busy {
            busy: 3,
            out: Vec::new(),
        });
        let ask = Scene::new(1).with_prompt(Some(PromptView {
            message: "Passphrase:".to_owned(),
            field: Field::Masked(0),
            notice: None,
        }));
        console
            .present(&ask, secs(0.0))
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(!console.prompt_written());
        for _ in 0..3 {
            console
                .present(&ask, secs(0.1))
                .unwrap_or_else(|e| panic!("{e}"));
        }
        assert!(console.prompt_written());
    }

    #[test]
    fn a_busy_console_gets_the_line_later() {
        let mut console = TextConsole::new(Busy {
            busy: 1,
            out: Vec::new(),
        });
        // The line is taken on, and goes out once the console is ready.
        assert_eq!(console.present(&Scene::new(1), secs(0.0)).ok(), Some(1));
        assert_eq!(console.present(&Scene::new(1), secs(0.1)).ok(), Some(0));
        assert_eq!(console.into_inner().out, b"Fairing 1%\n");
    }
}
