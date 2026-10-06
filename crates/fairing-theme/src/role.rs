// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! The eleven §11.1 role tokens.

use std::fmt;
use std::str::FromStr;

use crate::fault::{ThemeError, ThemeErrorKind};

/// A §11.1 theme role. Application code names roles, never colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Role {
    /// The canvas under every surface.
    Background,
    /// Elevated panels and cards; never a text colour.
    Surface,
    /// Code blocks and terminal wells; never a text colour.
    SurfaceAlt,
    /// Body text and the default readout.
    Foreground,
    /// Primary accent and the active readout.
    Accent,
    /// Structure, links and borders; also the informational colour.
    Structure,
    /// Success and safe status.
    Success,
    /// Error status.
    Error,
    /// Warning and attention.
    Warning,
    /// The visible focus indicator.
    Focus,
    /// Surface boundaries.
    Border,
}

impl Role {
    /// Every role, in palette-file order.
    pub const ALL: [Self; 11] = [
        Self::Background,
        Self::Surface,
        Self::SurfaceAlt,
        Self::Foreground,
        Self::Accent,
        Self::Structure,
        Self::Success,
        Self::Error,
        Self::Warning,
        Self::Focus,
        Self::Border,
    ];

    /// The palette file's spelling of the role.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Background => "background",
            Self::Surface => "surface",
            Self::SurfaceAlt => "surface-alt",
            Self::Foreground => "foreground",
            Self::Accent => "accent",
            Self::Structure => "structure",
            Self::Success => "success",
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Focus => "focus",
            Self::Border => "border",
        }
    }

    /// Whether the role is a surface fill, which §11.0.1 forbids as a text colour.
    #[must_use]
    pub const fn is_surface(self) -> bool {
        matches!(self, Self::Surface | Self::SurfaceAlt)
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Role {
    type Err = ThemeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|role| role.as_str() == s)
            .ok_or_else(|| ThemeError::new(ThemeErrorKind::UnknownRole, s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for role in Role::ALL {
            assert_eq!(role.as_str().parse::<Role>(), Ok(role));
            assert_eq!(role.to_string(), role.as_str());
        }
        assert!("surface_alt".parse::<Role>().is_err());
    }

    #[test]
    fn surfaces_are_the_two_fills() {
        let surfaces: Vec<Role> = Role::ALL.into_iter().filter(|r| r.is_surface()).collect();
        assert_eq!(surfaces, [Role::Surface, Role::SurfaceAlt]);
    }
}
