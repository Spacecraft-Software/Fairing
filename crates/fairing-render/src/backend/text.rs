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
    lines: u64,
}

impl<W: Write> TextConsole<W> {
    /// A console writing to `out`; nothing is written until the first scene.
    #[must_use]
    pub const fn new(out: W) -> Self {
        Self {
            out,
            last: None,
            prompt: None,
            lines: 0,
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

    /// Lines written so far.
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

    /// Writes `text` and a newline; `false` when the console is not ready.
    fn line(&mut self, text: &str) -> io::Result<bool> {
        let mut bytes = Vec::with_capacity(text.len() + 1);
        bytes.extend_from_slice(text.as_bytes());
        bytes.push(b'\n');
        match self.out.write_all(&bytes).and_then(|()| self.out.flush()) {
            Ok(()) => {
                self.lines += 1;
                Ok(true)
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(false),
            Err(error) => Err(error),
        }
    }
}

/// `Fairing <NN>% <status>`, or `Fairing <NN>%` with no status.
///
/// Implements: FRN-SRS-071
#[must_use]
pub fn progress_line(scene: &Scene) -> String {
    match scene.status() {
        Some(status) => plain(&format!("Fairing {}% {}", scene.percent(), status.trim())),
        None => format!("Fairing {}%", scene.percent()),
    }
}

/// `text` with every control character (C0, DEL and C1, escape and CSI
/// among them) turned into a space, and runs of spaces kept as they are.
///
/// Implements: FRN-SRS-073
#[must_use]
pub fn plain(text: &str) -> String {
    text.chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect()
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
    fn nothing_but_plain_text_reaches_the_console() {
        // Verifies: FRN-SRS-073
        let mut console = TextConsole::new(Vec::new());
        let hostile = "\u{1b}[31mRed\u{1b}[0m \u{9b}2J\u{7}done\r";
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
            out.chars().all(|ch| ch == '\n' || !ch.is_control()),
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

    #[test]
    fn a_busy_console_gets_the_line_later() {
        let mut console = TextConsole::new(Busy {
            busy: 1,
            out: Vec::new(),
        });
        assert_eq!(console.present(&Scene::new(1), secs(0.0)).ok(), Some(0));
        assert_eq!(console.present(&Scene::new(1), secs(0.1)).ok(), Some(1));
        assert_eq!(console.into_inner().out, b"Fairing 1%\n");
    }
}
