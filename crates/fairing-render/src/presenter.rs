// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Renders and presents scenes at a declared rate, re-acquiring a lost output.

use std::fmt;
use std::time::{Duration, Instant};

use crate::backend::{Backend, BackendKind, Choice, Surface, open};
use crate::cadence::Cadence;
use crate::compositor::Compositor;
use crate::fault::{RenderError, RenderErrorKind};
use crate::frame::Frame;
use crate::scene::Scene;

/// Default time to keep trying to re-acquire a lost output (FRN-SRS-006).
///
/// The splash raises this: between the firmware device vanishing and the
/// native driver registering its node there is commonly a gap of hundreds of
/// milliseconds to seconds, during which the last frame stays on screen.
pub const DEFAULT_REACQUIRE_BUDGET: Duration = Duration::from_millis(500);
/// Pause between re-acquire attempts.
const REACQUIRE_PAUSE: Duration = Duration::from_millis(25);

/// What the presenter has done so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stats {
    frames: u64,
    first_frame: Option<Duration>,
    dropped: u64,
    reacquired: u32,
}

impl Stats {
    /// Frames presented.
    #[must_use]
    pub const fn frames(&self) -> u64 {
        self.frames
    }

    /// Time from the origin instant to the first presented frame (FRN-SRS-004).
    #[must_use]
    pub const fn first_frame(&self) -> Option<Duration> {
        self.first_frame
    }

    /// Frame periods skipped because the loop fell behind.
    #[must_use]
    pub const fn dropped(&self) -> u64 {
        self.dropped
    }

    /// How many times the output was lost and re-acquired.
    #[must_use]
    pub const fn reacquired(&self) -> u32 {
        self.reacquired
    }
}

/// How a [`Presenter`] runs: rate, timing origin, re-acquire chain and budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresenterConfig {
    /// Frames per second (FRN-SRS-005 declares 30).
    pub hz: u32,
    /// The instant first-frame latency is measured from: process start (FRN-SRS-004).
    pub origin: Instant,
    /// The chain used to re-acquire a lost output (FRN-SRS-006).
    pub choice: Choice,
    /// How long a lost output is waited for before giving up.
    pub reacquire_budget: Duration,
}

impl Default for PresenterConfig {
    /// 30 Hz, measured from now, re-acquiring through the automatic chain.
    fn default() -> Self {
        Self {
            hz: 30,
            origin: Instant::now(),
            choice: Choice::Auto,
            reacquire_budget: DEFAULT_REACQUIRE_BUDGET,
        }
    }
}

/// Owns a surface, a compositor and a frame, and paces presentation.
pub struct Presenter {
    surface: Surface,
    compositor: Compositor,
    frame: Frame,
    cadence: Cadence,
    config: PresenterConfig,
    stats: Stats,
}

impl Presenter {
    /// A presenter over `surface` drawing with `compositor`.
    ///
    /// # Errors
    ///
    /// A frame allocation failure for the surface's size.
    pub fn new(
        surface: Surface,
        compositor: Compositor,
        config: PresenterConfig,
    ) -> Result<Self, RenderError> {
        let frame = Frame::new(surface.size(), surface.format())?;
        Ok(Self {
            surface,
            compositor,
            frame,
            cadence: Cadence::new(config.hz),
            config,
            stats: Stats::default(),
        })
    }

    /// The configuration in force.
    #[must_use]
    pub const fn config(&self) -> &PresenterConfig {
        &self.config
    }

    /// Draws and presents `scene`; on a lost output, re-acquires once and retries.
    ///
    /// Implements: FRN-SRS-006
    ///
    /// # Errors
    ///
    /// The backend's error when presentation fails for any reason other than a lost
    /// output, or when the output could not be re-acquired within 500 ms.
    pub fn present(&mut self, scene: &Scene) -> Result<(), RenderError> {
        self.compositor.render(&mut self.frame, scene);
        match self.surface.present(&self.frame) {
            Ok(()) => {}
            Err(error) if error.is_lost() => {
                self.reacquire()?;
                self.compositor.render(&mut self.frame, scene);
                self.surface.present(&self.frame)?;
            }
            Err(error) => return Err(error),
        }
        self.stats.frames += 1;
        if self.stats.first_frame.is_none() {
            self.stats.first_frame = Some(self.config.origin.elapsed());
        }
        Ok(())
    }

    /// Waits for the next frame deadline; returns how late the previous frame finished.
    pub fn pace(&mut self) -> Duration {
        let late = self.cadence.wait();
        self.stats.dropped = self.cadence.dropped();
        late
    }

    /// The last rendered frame.
    #[must_use]
    pub const fn frame(&self) -> &Frame {
        &self.frame
    }

    /// The surface in use.
    #[must_use]
    pub const fn surface(&self) -> &Surface {
        &self.surface
    }

    /// Which backend is in use.
    #[must_use]
    pub fn backend(&self) -> BackendKind {
        self.surface.kind()
    }

    /// The compositor.
    #[must_use]
    pub const fn compositor(&self) -> &Compositor {
        &self.compositor
    }

    /// Progress so far.
    #[must_use]
    pub const fn stats(&self) -> &Stats {
        &self.stats
    }

    /// Releases the output and returns the final statistics.
    ///
    /// # Errors
    ///
    /// The backend's teardown error.
    pub fn close(self) -> Result<Stats, RenderError> {
        self.surface.close()?;
        Ok(self.stats)
    }

    /// Re-opens the chain after the output vanished, within the re-acquire budget.
    fn reacquire(&mut self) -> Result<(), RenderError> {
        let deadline = Instant::now() + self.config.reacquire_budget;
        let memory_size = self.frame.size();
        loop {
            let failure = match open(self.config.choice, memory_size) {
                Ok(opened) => {
                    let old = std::mem::replace(&mut self.surface, opened.surface);
                    // The old device is gone; its teardown ioctls fail harmlessly.
                    let _ = old.close();
                    if self.surface.size() != self.frame.size()
                        || self.surface.format() != self.frame.format()
                    {
                        self.frame = Frame::new(self.surface.size(), self.surface.format())?;
                    }
                    self.stats.reacquired += 1;
                    return Ok(());
                }
                Err(failure) => failure,
            };
            if Instant::now() >= deadline {
                return Err(RenderError::new(
                    RenderErrorKind::Lost,
                    format!(
                        "output lost and not re-acquired within {:?}: {failure}",
                        self.config.reacquire_budget
                    ),
                ));
            }
            std::thread::sleep(REACQUIRE_PAUSE);
        }
    }
}

impl fmt::Debug for Presenter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Presenter")
            .field("backend", &self.surface.kind())
            .field("size", &self.frame.size())
            .field("cadence", &self.cadence.to_string())
            .field("stats", &self.stats)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use fairing_theme::{Selection, Theme};

    use super::*;
    use crate::backend::MemoryBackend;
    use crate::frame::PixelFormat;
    use crate::geometry::Size;

    #[test]
    fn presents_into_memory_and_records_stats() {
        // Verifies: FRN-SRS-004
        let size = Size::new(320, 200).unwrap_or_else(|e| panic!("{e}"));
        let surface = Surface::Memory(
            MemoryBackend::new(size, PixelFormat::Rgba8888).unwrap_or_else(|e| panic!("{e}")),
        );
        let compositor = Compositor::new(Selection::Colour(Theme::family_default()))
            .unwrap_or_else(|e| panic!("{e}"));
        let config = PresenterConfig {
            choice: Choice::Memory,
            ..PresenterConfig::default()
        };
        let mut presenter =
            Presenter::new(surface, compositor, config).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(presenter.config().hz, 30);
        assert_eq!(presenter.backend(), BackendKind::Memory);
        for percent in [0, 50, 100] {
            presenter.pace();
            presenter
                .present(&Scene::new(percent).with_status("test"))
                .unwrap_or_else(|e| panic!("{e}"));
        }
        let stats = *presenter.stats();
        assert_eq!(stats.frames(), 3);
        assert!(stats.first_frame().is_some_and(|d| d >= Duration::ZERO));
        assert_eq!(stats.reacquired(), 0);
        let memory = presenter
            .surface()
            .as_memory()
            .unwrap_or_else(|| panic!("memory"));
        assert_eq!(memory.presented(), 3);
        assert_eq!(presenter.frame().size(), size);
        let closed = presenter.close().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(closed, stats);
    }
}
