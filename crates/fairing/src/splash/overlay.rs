// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! The journal overlay: Escape shows the boot's log over the splash.
//!
//! The overlay keeps the last [`ENTRIES`] journal entries at priority notice
//! or higher, newest at the bottom (FRN-SRS-060). Up and `k` scroll back a
//! line, Down and `j` forward, Page Up and Page Down a page (FRN-SRS-061).
//! journalctl is started the first time the overlay is shown and kept
//! following from then on, so a boot nobody looks at runs no extra process.
//! While a password question is open the overlay is neither drawn nor
//! toggled: the prompt is what the screen is for then.

use std::collections::VecDeque;
use std::path::PathBuf;

use fairing_render::OverlayView;

use crate::splash::journal::{ENTRIES, Journal};
use crate::splash::keys::Nav;

/// Lines Page Up and Page Down move.
pub const PAGE: usize = 10;

/// The overlay's state.
#[derive(Debug)]
pub struct Overlay {
    journalctl: PathBuf,
    journal: Option<Journal>,
    lines: VecDeque<String>,
    shown: bool,
    scroll: usize,
    /// journalctl could not be started, or exited: said once, in the overlay.
    unavailable: bool,
    /// When the overlay first read the running journalctl (`CLOCK_MONOTONIC`,
    /// µs): entries stamped earlier are the backlog, not new ones.
    following_since_us: Option<u64>,
    /// The longest a new entry took from the journal to the overlay while it
    /// was shown, in µs (FRN-SRS-062).
    worst_entry_us: Option<u64>,
}

impl Overlay {
    /// A hidden overlay that will run `journalctl` when first shown.
    #[must_use]
    pub fn new(journalctl: PathBuf) -> Self {
        Self {
            journalctl,
            journal: None,
            lines: VecDeque::with_capacity(ENTRIES),
            shown: false,
            scroll: 0,
            unavailable: false,
            following_since_us: None,
            worst_entry_us: None,
        }
    }

    /// The longest a new journal entry took to reach the overlay while it was
    /// shown, in µs: from the entry's `__MONOTONIC_TIMESTAMP` to the frame
    /// that took it (FRN-SRS-062). `None` when no new entry arrived.
    #[must_use]
    pub const fn worst_entry_us(&self) -> Option<u64> {
        self.worst_entry_us
    }

    /// Whether the overlay is up.
    #[cfg(test)]
    #[must_use]
    pub const fn shown(&self) -> bool {
        self.shown
    }

    /// Acts on one key.
    ///
    /// Implements: FRN-SRS-060, FRN-SRS-061
    pub fn key(&mut self, key: Nav) {
        match key {
            Nav::Toggle => {
                self.shown = !self.shown;
                self.scroll = 0;
                if self.shown && self.journal.is_none() && !self.unavailable {
                    match Journal::spawn(&self.journalctl) {
                        Ok(journal) => self.journal = Some(journal),
                        Err(error) => {
                            self.unavailable(&format!("the journal cannot be read: {error}"));
                        }
                    }
                }
            }
            _ if !self.shown => {}
            Nav::Up => self.scroll_by(1, true),
            Nav::Down => self.scroll_by(1, false),
            Nav::PageUp => self.scroll_by(PAGE, true),
            Nav::PageDown => self.scroll_by(PAGE, false),
        }
    }

    /// One frame at `now_us` (`CLOCK_MONOTONIC`, µs): `keys` act unless a
    /// password prompt is open, journalctl's new entries are taken, and the
    /// view is returned while shown and no prompt covers the screen
    /// (FRN-SRS-060: "with no prompt active").
    ///
    /// Implements: FRN-SRS-060
    pub fn frame(&mut self, keys: &[Nav], prompt_open: bool, now_us: u64) -> Option<OverlayView> {
        if !prompt_open {
            for &key in keys {
                self.key(key);
            }
        }
        self.take(now_us, self.shown && !prompt_open);
        if prompt_open { None } else { self.view() }
    }

    /// [`Self::take`] as a frame with no prompt open does.
    #[cfg(test)]
    fn tick(&mut self, now_us: u64) {
        self.take(now_us, self.shown);
    }

    /// Takes what journalctl wrote since the last frame, at `now_us`
    /// (`CLOCK_MONOTONIC`, µs, the clock of the journal's monotonic stamps),
    /// timing the new entries only while they are `on_screen`.
    ///
    /// Implements: FRN-SRS-062
    fn take(&mut self, now_us: u64, on_screen: bool) {
        let Some(journal) = self.journal.as_mut() else {
            return;
        };
        let since = *self.following_since_us.get_or_insert(now_us);
        let mut worst = self.worst_entry_us;
        let mut lines = Vec::new();
        let alive = journal.read(|entry| {
            if let Some(stamp) = entry.monotonic_us.filter(|&t| on_screen && t >= since) {
                let waited = now_us.saturating_sub(stamp);
                worst = Some(worst.map_or(waited, |w| w.max(waited)));
            }
            lines.push(entry.line());
        });
        self.worst_entry_us = worst;
        for line in lines {
            self.push(line);
        }
        match alive {
            Ok(true) => {}
            Ok(false) => self.stop("journalctl exited; no new entries will appear"),
            Err(error) => self.stop(&format!("reading the journal failed: {error}")),
        }
    }

    /// What to draw, while shown.
    #[must_use]
    pub fn view(&self) -> Option<OverlayView> {
        self.shown.then(|| OverlayView {
            lines: self.lines.iter().cloned().collect(),
            scroll: self.scroll,
        })
    }

    /// Stops journalctl.
    pub fn close(&mut self) {
        if let Some(journal) = self.journal.take() {
            journal.stop();
        }
    }

    /// Adds a line, dropping the oldest beyond [`ENTRIES`]. A view scrolled
    /// back stays on the lines it shows.
    fn push(&mut self, line: String) {
        if self.lines.len() == ENTRIES {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
        if self.scroll > 0 {
            self.scroll = (self.scroll + 1).min(self.lines.len() - 1);
        }
    }

    fn scroll_by(&mut self, lines: usize, back: bool) {
        let most = self.lines.len().saturating_sub(1);
        self.scroll = if back {
            self.scroll.saturating_add(lines).min(most)
        } else {
            self.scroll.saturating_sub(lines)
        };
    }

    fn stop(&mut self, why: &str) {
        self.close();
        self.unavailable(why);
    }

    fn unavailable(&mut self, why: &str) {
        self.unavailable = true;
        self.push(format!("       - [WARN] fairing: {why}"));
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::{Duration, Instant};

    use super::*;

    fn fake(dir: &Path, script: &str) -> PathBuf {
        crate::splash::journal::fake_journalctl(dir, script)
    }

    fn entries(n: usize) -> String {
        let mut script = String::new();
        for i in 1..=n {
            script.push_str("echo '{\"PRIORITY\":\"5\",\"MESSAGE\":\"entry ");
            script.push_str(&i.to_string());
            script.push_str("\"}'\n");
        }
        script
    }

    fn settle(overlay: &mut Overlay, want: usize) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while overlay.lines.len() < want && Instant::now() < deadline {
            overlay.tick(fairing_askpass::watch::monotonic_us());
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn escape_shows_the_last_forty_entries_newest_last() {
        // Verifies: FRN-SRS-060
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let program = fake(dir.path(), &format!("{}sleep 5", entries(45)));
        let mut overlay = Overlay::new(program);
        assert_eq!(overlay.view(), None);
        overlay.tick(fairing_askpass::watch::monotonic_us());
        assert!(overlay.journal.is_none(), "nothing runs until shown");
        overlay.key(Nav::Toggle);
        settle(&mut overlay, ENTRIES);
        std::thread::sleep(Duration::from_millis(100));
        overlay.tick(fairing_askpass::watch::monotonic_us());
        let view = overlay.view().unwrap_or_else(|| panic!("hidden"));
        assert_eq!(view.lines.len(), ENTRIES);
        assert!(view.lines[0].ends_with("entry 6"), "{:?}", view.lines[0]);
        assert!(view.lines[ENTRIES - 1].ends_with("entry 45"));
        assert_eq!(view.scroll, 0);
        overlay.key(Nav::Toggle);
        assert_eq!(overlay.view(), None);
        overlay.close();
    }

    #[test]
    fn a_new_entry_is_timed_from_the_journal_to_the_overlay() {
        // Verifies: FRN-SRS-062
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let entry = |stamp: u64, text: &str| {
            format!(
                "echo '{{\"PRIORITY\":\"5\",\"__MONOTONIC_TIMESTAMP\":\"{stamp}\",\"MESSAGE\":\"{text}\"}}'\n"
            )
        };
        let script = format!(
            "{}sleep 0.3\n{}sleep 5",
            entry(500_000, "backlog"),
            entry(1_002_000, "new")
        );
        let mut overlay = Overlay::new(fake(dir.path(), &script));
        overlay.key(Nav::Toggle);
        // The first frame marks where following begins; the backlog is older.
        let deadline = Instant::now() + Duration::from_secs(5);
        while overlay.lines.is_empty() && Instant::now() < deadline {
            overlay.tick(1_000_000);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(overlay.worst_entry_us(), None, "the backlog is not timed");
        while overlay.lines.len() < 2 && Instant::now() < deadline {
            overlay.tick(1_010_000);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(overlay.worst_entry_us(), Some(8_000));
        overlay.close();

        // An entry that arrives under a prompt is not on screen: not timed.
        let script = format!("sleep 0.3\n{}sleep 5", entry(1_002_000, "under the prompt"));
        let mut overlay = Overlay::new(fake(dir.path(), &script));
        overlay.key(Nav::Toggle);
        overlay.frame(&[], true, 1_000_000);
        let deadline = Instant::now() + Duration::from_secs(5);
        while overlay.lines.is_empty() && Instant::now() < deadline {
            overlay.frame(&[], true, 1_010_000);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(overlay.lines.len(), 1, "taken");
        assert_eq!(overlay.worst_entry_us(), None, "but not timed");
        overlay.close();
    }

    #[test]
    fn a_prompt_hides_the_overlay_and_escape_is_the_prompts() {
        // Verifies: FRN-SRS-060
        let mut overlay = Overlay::new(PathBuf::from("/nonexistent/journalctl"));
        assert_eq!(overlay.frame(&[Nav::Toggle], true, 0), None);
        assert!(!overlay.shown(), "Escape during a prompt is the prompt's");
        assert!(overlay.frame(&[Nav::Toggle], false, 0).is_some());
        assert_eq!(overlay.frame(&[], true, 0), None, "the prompt covers it");
        assert!(
            overlay.frame(&[], false, 0).is_some(),
            "and it is back after"
        );
    }

    #[test]
    fn journalctl_runs_once_and_never_after_it_is_closed() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let mut overlay = Overlay::new(fake(dir.path(), "exec sleep 30"));
        overlay.key(Nav::Toggle);
        assert!(overlay.journal.is_some(), "started when first shown");
        overlay.close();
        assert!(overlay.journal.is_none(), "stopped");
        // Hiding the overlay, or showing it again, starts nothing.
        overlay.key(Nav::Toggle);
        assert!(overlay.journal.is_none());
        assert!(!overlay.shown());

        // A journalctl that cannot be started is said once, however often
        // the overlay is shown.
        let mut overlay = Overlay::new(PathBuf::from("/nonexistent/journalctl"));
        for _ in 0..4 {
            overlay.key(Nav::Toggle);
        }
        assert_eq!(overlay.lines.len(), 1, "{:?}", overlay.lines);
    }

    #[test]
    fn a_journalctl_that_exits_is_said_and_let_go() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let mut overlay = Overlay::new(fake(dir.path(), "exit 0"));
        overlay.key(Nav::Toggle);
        let deadline = Instant::now() + Duration::from_secs(5);
        while overlay.journal.is_some() && Instant::now() < deadline {
            overlay.tick(fairing_askpass::watch::monotonic_us());
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(overlay.journal.is_none(), "reaped");
        assert!(
            overlay
                .lines
                .back()
                .is_some_and(|l| l.contains("journalctl exited")),
            "{:?}",
            overlay.lines
        );
    }

    #[test]
    fn a_full_overlay_scrolled_to_the_oldest_line_stays_on_it() {
        let mut overlay = Overlay::new(PathBuf::from("/nonexistent/journalctl"));
        overlay.key(Nav::Toggle);
        for i in 0..ENTRIES {
            overlay.push(format!("line {i}"));
        }
        for _ in 0..=ENTRIES / PAGE {
            overlay.key(Nav::PageUp);
        }
        assert_eq!(overlay.scroll, ENTRIES - 1);
        overlay.push("newer".to_owned());
        assert_eq!(overlay.lines.len(), ENTRIES);
        assert_eq!(overlay.scroll, ENTRIES - 1, "never past the oldest line");
    }

    #[test]
    fn the_keys_scroll_within_the_entries() {
        // Verifies: FRN-SRS-061
        let mut overlay = Overlay::new(PathBuf::from("/nonexistent/journalctl"));
        for i in 0..30 {
            overlay.push(format!("line {i}"));
        }
        overlay.key(Nav::Up);
        assert_eq!(overlay.scroll, 0, "nothing scrolls while hidden");
        overlay.key(Nav::Toggle);
        // The missing journalctl is said in the overlay itself.
        assert!(
            overlay
                .lines
                .back()
                .is_some_and(|l| l.contains("cannot be read"))
        );
        overlay.key(Nav::Up);
        overlay.key(Nav::Up);
        assert_eq!(overlay.scroll, 2);
        overlay.key(Nav::Down);
        assert_eq!(overlay.scroll, 1);
        overlay.key(Nav::PageUp);
        assert_eq!(overlay.scroll, 1 + PAGE);
        for _ in 0..10 {
            overlay.key(Nav::PageUp);
        }
        assert_eq!(
            overlay.scroll,
            overlay.lines.len() - 1,
            "the oldest stays on screen"
        );
        for _ in 0..10 {
            overlay.key(Nav::PageDown);
        }
        assert_eq!(overlay.scroll, 0);
        // A new line while scrolled back keeps the view where it was.
        overlay.key(Nav::Up);
        overlay.push("newer".to_owned());
        assert_eq!(overlay.scroll, 2);
    }
}
