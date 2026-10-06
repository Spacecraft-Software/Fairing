// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! The crate's one error type (M-ERRORS-CANONICAL-STRUCTS).
//!
//! Operating-system failures are classified once, here, from their errno, so
//! backends and callers branch on [`RenderErrorKind`] and never on numbers.

use std::fmt;
use std::io;

use rustix::io::Errno;

use crate::backend::BackendKind;

/// What went wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RenderErrorKind {
    /// No device, connector or file to draw on (`ENOENT`, no connected output).
    NotFound,
    /// The device exists but refused us (`EACCES`/`EPERM`: not DRM master, no group membership).
    PermissionDenied,
    /// Another client holds the resource (`EBUSY`).
    Busy,
    /// The device exists but we cannot drive it (no dumb buffers, unusual depth).
    Unsupported,
    /// The device went away under us (`ENODEV`/`ENXIO`: simpledrm replaced by the native driver).
    Lost,
    /// A wait exceeded its bound.
    Timeout,
    /// A size or rectangle was degenerate.
    InvalidGeometry,
    /// The bundled font failed to parse (a build defect, never a runtime input).
    Font,
    /// Any other I/O failure.
    Io,
}

impl RenderErrorKind {
    /// Stable lowercase name for diagnostics.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotFound => "not-found",
            Self::PermissionDenied => "permission-denied",
            Self::Busy => "busy",
            Self::Unsupported => "unsupported",
            Self::Lost => "lost",
            Self::Timeout => "timeout",
            Self::InvalidGeometry => "invalid-geometry",
            Self::Font => "font",
            Self::Io => "io",
        }
    }

    /// Classifies an operating-system error by its errno.
    #[must_use]
    pub fn classify(error: &io::Error) -> Self {
        let raw = error.raw_os_error();
        if raw == Some(Errno::NODEV.raw_os_error()) || raw == Some(Errno::NXIO.raw_os_error()) {
            Self::Lost
        } else if raw == Some(Errno::ACCESS.raw_os_error())
            || raw == Some(Errno::PERM.raw_os_error())
        {
            Self::PermissionDenied
        } else if raw == Some(Errno::BUSY.raw_os_error()) {
            Self::Busy
        } else if raw == Some(Errno::NOENT.raw_os_error()) {
            Self::NotFound
        } else if error.kind() == io::ErrorKind::TimedOut {
            Self::Timeout
        } else {
            Self::Io
        }
    }
}

/// A rendering or output failure: what, on which backend, and the OS error if any.
#[derive(Debug)]
pub struct RenderError {
    kind: RenderErrorKind,
    backend: Option<BackendKind>,
    detail: String,
    source: Option<io::Error>,
}

impl RenderError {
    /// A failure with no operating-system cause.
    pub fn new(kind: RenderErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            backend: None,
            detail: detail.into(),
            source: None,
        }
    }

    /// An operating-system failure, classified from its errno.
    pub fn from_io(detail: impl Into<String>, source: io::Error) -> Self {
        Self {
            kind: RenderErrorKind::classify(&source),
            backend: None,
            detail: detail.into(),
            source: Some(source),
        }
    }

    /// Attributes the failure to a backend.
    #[must_use]
    pub fn on(mut self, backend: BackendKind) -> Self {
        self.backend = Some(backend);
        self
    }

    /// The category.
    #[must_use]
    pub fn kind(&self) -> RenderErrorKind {
        self.kind
    }

    /// The backend that failed, if the failure belongs to one.
    #[must_use]
    pub fn backend(&self) -> Option<BackendKind> {
        self.backend
    }

    /// What was being attempted.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }

    /// Whether the output vanished and must be re-acquired (FRN-SRS-006).
    #[must_use]
    pub fn is_lost(&self) -> bool {
        self.kind == RenderErrorKind::Lost
    }
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(backend) = self.backend {
            write!(f, "{backend}: ")?;
        }
        write!(f, "{}", self.detail)?;
        if let Some(source) = &self.source {
            write!(f, ": {source}")?;
        }
        Ok(())
    }
}

impl std::error::Error for RenderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|e| e as &(dyn std::error::Error + 'static))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(errno: Errno) -> io::Error {
        io::Error::from_raw_os_error(errno.raw_os_error())
    }

    #[test]
    fn errnos_classify_into_kinds() {
        assert_eq!(
            RenderErrorKind::classify(&os(Errno::NODEV)),
            RenderErrorKind::Lost
        );
        assert_eq!(
            RenderErrorKind::classify(&os(Errno::NXIO)),
            RenderErrorKind::Lost
        );
        assert_eq!(
            RenderErrorKind::classify(&os(Errno::ACCESS)),
            RenderErrorKind::PermissionDenied
        );
        assert_eq!(
            RenderErrorKind::classify(&os(Errno::PERM)),
            RenderErrorKind::PermissionDenied
        );
        assert_eq!(
            RenderErrorKind::classify(&os(Errno::BUSY)),
            RenderErrorKind::Busy
        );
        assert_eq!(
            RenderErrorKind::classify(&os(Errno::NOENT)),
            RenderErrorKind::NotFound
        );
        assert_eq!(
            RenderErrorKind::classify(&os(Errno::IO)),
            RenderErrorKind::Io
        );
        assert_eq!(
            RenderErrorKind::classify(&io::Error::new(io::ErrorKind::TimedOut, "slow")),
            RenderErrorKind::Timeout
        );
    }

    #[test]
    fn display_names_backend_detail_and_source() {
        let error =
            RenderError::from_io("open `/dev/dri/card0`", os(Errno::ACCESS)).on(BackendKind::Drm);
        let text = error.to_string();
        assert!(text.starts_with("drm: open `/dev/dri/card0`: "), "{text}");
        assert!(!error.is_lost() && error.kind() == RenderErrorKind::PermissionDenied);
        assert_eq!(error.backend(), Some(BackendKind::Drm));
        assert!(std::error::Error::source(&error).is_some());
    }
}
