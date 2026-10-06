// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Rendering for Fairing: the frame compositor and the output backends.
//!
//! A [`Scene`] (progress and status) is drawn by the [`Compositor`] into a
//! [`Frame`] — a heap pixel buffer in the backend's byte order — which a
//! backend then presents: DRM/KMS with dumb buffers and page flips
//! (FRN-SRS-001, FRN-SRS-006), `/dev/fb0` through `write_at` (FRN-SRS-002),
//! or memory for tests and snapshots. [`open`] runs the fallback chain and
//! names every backend that failed (FRN-SRS-003). The [`Presenter`] paces
//! presentation at a declared rate (FRN-SRS-005) and records when the first
//! frame reached the screen (FRN-SRS-004).
//!
//! The layout is authored in a 1920×1080 reference frame and scaled uniformly
//! into the active mode, letterboxed, so one theme serves every panel
//! (FRN-SRS-007). Colours are §11.1 role tokens of a `fairing-theme`
//! selection; this crate holds no colour of its own except the Linux console's
//! default palette, which stands in for ANSI slots when the mono theme is
//! drawn on a framebuffer.
//!
//! All unsafe lives in qualified dependencies (`drm`, `rustix`, `tiny-skia`,
//! `fontdue`); this crate forbids it. Assurance Category B.

#![forbid(unsafe_code)]

mod backend;
mod cadence;
mod compositor;
mod fault;
mod frame;
mod geometry;
mod layout;
mod logo;
mod palette;
mod presenter;
mod scene;
mod shapes;
mod text;

#[doc(inline)]
pub use backend::drm::DrmBackend;
#[doc(inline)]
pub use backend::fbdev::{FbInfo, FbdevBackend};
#[doc(inline)]
pub use backend::{
    Attempt, Backend, BackendKind, Choice, MemoryBackend, NoBackend, Opened, Surface, open,
};
#[doc(inline)]
pub use cadence::{Cadence, Clock, SystemClock};
#[doc(inline)]
pub use compositor::Compositor;
#[doc(inline)]
pub use fault::{RenderError, RenderErrorKind};
#[doc(inline)]
pub use frame::{Frame, PixelFormat};
#[doc(inline)]
pub use geometry::{Rect, Size, Viewport};
#[doc(inline)]
pub use layout::{Layout, REFERENCE_SIZE};
#[doc(inline)]
pub use palette::Palette;
#[doc(inline)]
pub use presenter::{DEFAULT_REACQUIRE_BUDGET, Presenter, PresenterConfig, Reopen, Stats};
#[doc(inline)]
pub use scene::Scene;
