// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Output backends and the fallback chain.
//!
//! [`Backend`] is the one seam hardware-free code can stop at: the compositor
//! and presenter are exercised against [`MemoryBackend`]; the DRM and fbdev
//! implementations are driven only on a machine with the device. [`open`] runs
//! the chain DRM → fbdev, records how long each attempt took and why it
//! failed, and hands the caller either a [`Surface`] or the full list of
//! failures to put in the journal (FRN-SRS-002, FRN-SRS-003).

pub mod drm;
pub mod fbdev;
mod memory;

use std::fmt;
use std::time::{Duration, Instant};

pub use memory::MemoryBackend;

use crate::fault::RenderError;
use crate::frame::{Frame, PixelFormat};
use crate::geometry::Size;

/// Which output implementation a surface is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackendKind {
    /// DRM/KMS with dumb buffers and page flips.
    Drm,
    /// The legacy `/dev/fb0` framebuffer.
    Fbdev,
    /// A heap buffer: tests, agents, snapshots.
    Memory,
}

impl BackendKind {
    /// Stable lowercase name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Drm => "drm",
            Self::Fbdev => "fbdev",
            Self::Memory => "memory",
        }
    }
}

impl fmt::Display for BackendKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Something frames can be presented on. Backends are `Send` so a render
/// thread can own one (M-TYPES-SEND).
pub trait Backend: Send {
    /// Which implementation this is.
    fn kind(&self) -> BackendKind;
    /// The output size; frames must match it exactly.
    fn size(&self) -> Size;
    /// The byte order frames must be drawn in.
    fn format(&self) -> PixelFormat;
    /// Shows `frame`. Returns when the frame is on its way to the screen.
    ///
    /// # Errors
    ///
    /// A frame of the wrong size or format is [`RenderErrorKind::InvalidGeometry`](crate::RenderErrorKind::InvalidGeometry);
    /// device failures carry their classified kind, [`RenderErrorKind::Lost`](crate::RenderErrorKind::Lost)
    /// meaning the output must be re-acquired.
    fn present(&mut self, frame: &Frame) -> Result<(), RenderError>;
    /// Releases the output and restores what was on it, where that is possible.
    ///
    /// # Errors
    ///
    /// The first teardown step that failed; the rest are still attempted.
    fn close(self) -> Result<(), RenderError>
    where
        Self: Sized;
}

/// The backend chosen by the chain.
#[derive(Debug)]
pub enum Surface {
    /// DRM/KMS.
    Drm(drm::DrmBackend),
    /// `/dev/fb0`.
    Fbdev(fbdev::FbdevBackend),
    /// Memory.
    Memory(MemoryBackend),
}

impl Surface {
    /// The memory backend, if that is what this is.
    #[must_use]
    pub const fn as_memory(&self) -> Option<&MemoryBackend> {
        match self {
            Self::Memory(memory) => Some(memory),
            Self::Drm(_) | Self::Fbdev(_) => None,
        }
    }
}

impl Backend for Surface {
    fn kind(&self) -> BackendKind {
        match self {
            Self::Drm(b) => b.kind(),
            Self::Fbdev(b) => b.kind(),
            Self::Memory(b) => b.kind(),
        }
    }

    fn size(&self) -> Size {
        match self {
            Self::Drm(b) => b.size(),
            Self::Fbdev(b) => b.size(),
            Self::Memory(b) => b.size(),
        }
    }

    fn format(&self) -> PixelFormat {
        match self {
            Self::Drm(b) => b.format(),
            Self::Fbdev(b) => b.format(),
            Self::Memory(b) => b.format(),
        }
    }

    fn present(&mut self, frame: &Frame) -> Result<(), RenderError> {
        match self {
            Self::Drm(b) => b.present(frame),
            Self::Fbdev(b) => b.present(frame),
            Self::Memory(b) => b.present(frame),
        }
    }

    fn close(self) -> Result<(), RenderError> {
        match self {
            Self::Drm(b) => b.close(),
            Self::Fbdev(b) => b.close(),
            Self::Memory(b) => b.close(),
        }
    }
}

/// Which backends to try, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Choice {
    /// DRM, then fbdev — the splash's chain.
    Auto,
    /// DRM only.
    Drm,
    /// fbdev only.
    Fbdev,
    /// Memory only.
    Memory,
}

impl Choice {
    /// The backends this choice tries, in order.
    #[must_use]
    pub const fn chain(self) -> &'static [BackendKind] {
        match self {
            Self::Auto => &[BackendKind::Drm, BackendKind::Fbdev],
            Self::Drm => &[BackendKind::Drm],
            Self::Fbdev => &[BackendKind::Fbdev],
            Self::Memory => &[BackendKind::Memory],
        }
    }

    /// Stable lowercase name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Drm => "drm",
            Self::Fbdev => "fbdev",
            Self::Memory => "memory",
        }
    }
}

impl fmt::Display for Choice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One backend that was tried and failed.
#[derive(Debug)]
pub struct Attempt {
    /// Which backend.
    pub backend: BackendKind,
    /// Why it failed.
    pub error: RenderError,
    /// How long the attempt took.
    pub elapsed: Duration,
}

/// A surface plus the record of what was tried before it.
#[derive(Debug)]
pub struct Opened {
    /// The working backend.
    pub surface: Surface,
    /// Backends that failed before it, in chain order.
    pub attempts: Vec<Attempt>,
    /// Time from the first attempt to the working backend.
    pub elapsed: Duration,
}

/// Every backend in the chain failed.
#[derive(Debug)]
pub struct NoBackend {
    /// Each failure, in chain order.
    pub attempts: Vec<Attempt>,
    /// Time spent on the whole chain.
    pub elapsed: Duration,
}

impl NoBackend {
    /// The failing backends, comma-separated, for the journal notice (FRN-SRS-003).
    #[must_use]
    pub fn names(&self) -> String {
        self.attempts
            .iter()
            .map(|a| a.backend.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl fmt::Display for NoBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "no backend could draw (")?;
        for (i, attempt) in self.attempts.iter().enumerate() {
            if i > 0 {
                write!(f, "; ")?;
            }
            write!(f, "{}", attempt.error)?;
        }
        write!(f, ")")
    }
}

impl std::error::Error for NoBackend {}

/// Opens the first backend in `choice`'s chain that works.
///
/// `memory_size` is the size of the memory backend when the chain reaches it.
///
/// Implements: FRN-SRS-002, FRN-SRS-003
///
/// # Errors
///
/// [`NoBackend`] with every attempt when nothing in the chain could be opened.
pub fn open(choice: Choice, memory_size: Size) -> Result<Opened, NoBackend> {
    open_chain(choice.chain(), |kind| open_one(kind, memory_size))
}

/// Runs `opener` over `kinds` in order, timing each attempt.
pub(crate) fn open_chain(
    kinds: &[BackendKind],
    mut opener: impl FnMut(BackendKind) -> Result<Surface, RenderError>,
) -> Result<Opened, NoBackend> {
    let start = Instant::now();
    let mut attempts = Vec::new();
    for &kind in kinds {
        let attempt_start = Instant::now();
        match opener(kind) {
            Ok(surface) => {
                return Ok(Opened {
                    surface,
                    attempts,
                    elapsed: start.elapsed(),
                });
            }
            Err(error) => attempts.push(Attempt {
                backend: kind,
                error: error.on(kind),
                elapsed: attempt_start.elapsed(),
            }),
        }
    }
    Err(NoBackend {
        attempts,
        elapsed: start.elapsed(),
    })
}

fn open_one(kind: BackendKind, memory_size: Size) -> Result<Surface, RenderError> {
    match kind {
        BackendKind::Drm => drm::DrmBackend::open().map(Surface::Drm),
        BackendKind::Fbdev => fbdev::FbdevBackend::open().map(Surface::Fbdev),
        BackendKind::Memory => {
            MemoryBackend::new(memory_size, PixelFormat::Rgba8888).map(Surface::Memory)
        }
    }
}

/// Compile-time proof that the types a render thread owns can move there.
const _: () = {
    const fn assert_send<T: Send>() {}
    assert_send::<Surface>();
    assert_send::<Opened>();
    assert_send::<NoBackend>();
    assert_send::<RenderError>();
    assert_send::<crate::compositor::Compositor>();
    assert_send::<crate::presenter::Presenter>();
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fault::RenderErrorKind;

    fn size() -> Size {
        Size::new(64, 32).unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn chain_falls_through_failures_and_records_them() {
        let opened = open_chain(Choice::Auto.chain(), |kind| match kind {
            BackendKind::Drm => Err(RenderError::new(RenderErrorKind::NotFound, "no card")),
            BackendKind::Fbdev => {
                MemoryBackend::new(size(), PixelFormat::Xrgb8888).map(Surface::Memory)
            }
            BackendKind::Memory => unreachable!("not in the auto chain"),
        })
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(opened.attempts.len(), 1);
        assert_eq!(opened.attempts[0].backend, BackendKind::Drm);
        assert_eq!(opened.attempts[0].error.backend(), Some(BackendKind::Drm));
        assert!(opened.elapsed >= opened.attempts[0].elapsed);
        assert_eq!(opened.surface.format(), PixelFormat::Xrgb8888);
    }

    #[test]
    fn exhausted_chain_names_every_backend() {
        let error = open_chain(Choice::Auto.chain(), |kind| {
            Err(RenderError::new(
                RenderErrorKind::NotFound,
                format!("{kind} missing"),
            ))
        })
        .err()
        .unwrap_or_else(|| panic!("expected failure"));
        assert_eq!(error.names(), "drm, fbdev");
        let text = error.to_string();
        assert!(
            text.contains("drm: drm missing") && text.contains("fbdev: fbdev missing"),
            "{text}"
        );
    }

    #[test]
    fn choices_expand_to_chains() {
        assert_eq!(
            Choice::Auto.chain(),
            &[BackendKind::Drm, BackendKind::Fbdev]
        );
        assert_eq!(Choice::Memory.chain(), &[BackendKind::Memory]);
        assert_eq!(Choice::Fbdev.to_string(), "fbdev");
        let opened = open(Choice::Memory, size()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(opened.surface.kind(), BackendKind::Memory);
        assert!(opened.surface.as_memory().is_some());
    }
}
