// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! The mono theme: roles bound to 4-bit ANSI slots (§11.1.1).

use std::fmt;
use std::str::FromStr;

use crate::fault::{ThemeError, ThemeErrorKind};
use crate::role::Role;

/// A 4-bit ANSI slot, the terminal default, or reverse video.
///
/// Hue is deferred entirely to the terminal's own palette; on a framebuffer the
/// renderer substitutes the console's default palette for the same slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ansi {
    /// The terminal's default foreground or background.
    Default,
    /// SGR 30 / 40.
    Black,
    /// SGR 31 / 41.
    Red,
    /// SGR 32 / 42.
    Green,
    /// SGR 33 / 43.
    Yellow,
    /// SGR 34 / 44.
    Blue,
    /// SGR 35 / 45.
    Magenta,
    /// SGR 36 / 46.
    Cyan,
    /// SGR 37 / 47.
    White,
    /// SGR 90 / 100.
    BrightBlack,
    /// SGR 91 / 101.
    BrightRed,
    /// SGR 92 / 102.
    BrightGreen,
    /// SGR 93 / 103.
    BrightYellow,
    /// SGR 94 / 104.
    BrightBlue,
    /// SGR 95 / 105.
    BrightMagenta,
    /// SGR 96 / 106.
    BrightCyan,
    /// SGR 97 / 107.
    BrightWhite,
    /// SGR 7: swap foreground and background.
    ReverseVideo,
}

impl Ansi {
    /// Every slot, in SGR order.
    pub const ALL: [Self; 18] = [
        Self::Default,
        Self::Black,
        Self::Red,
        Self::Green,
        Self::Yellow,
        Self::Blue,
        Self::Magenta,
        Self::Cyan,
        Self::White,
        Self::BrightBlack,
        Self::BrightRed,
        Self::BrightGreen,
        Self::BrightYellow,
        Self::BrightBlue,
        Self::BrightMagenta,
        Self::BrightCyan,
        Self::BrightWhite,
        Self::ReverseVideo,
    ];

    /// The palette file's spelling of the slot.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Black => "black",
            Self::Red => "red",
            Self::Green => "green",
            Self::Yellow => "yellow",
            Self::Blue => "blue",
            Self::Magenta => "magenta",
            Self::Cyan => "cyan",
            Self::White => "white",
            Self::BrightBlack => "bright-black",
            Self::BrightRed => "bright-red",
            Self::BrightGreen => "bright-green",
            Self::BrightYellow => "bright-yellow",
            Self::BrightBlue => "bright-blue",
            Self::BrightMagenta => "bright-magenta",
            Self::BrightCyan => "bright-cyan",
            Self::BrightWhite => "bright-white",
            Self::ReverseVideo => "reverse-video",
        }
    }

    /// The 16-colour index (`0..=15`) of a colour slot; `None` for default and reverse video.
    #[must_use]
    pub const fn index(self) -> Option<u8> {
        Some(match self {
            Self::Default | Self::ReverseVideo => return None,
            Self::Black => 0,
            Self::Red => 1,
            Self::Green => 2,
            Self::Yellow => 3,
            Self::Blue => 4,
            Self::Magenta => 5,
            Self::Cyan => 6,
            Self::White => 7,
            Self::BrightBlack => 8,
            Self::BrightRed => 9,
            Self::BrightGreen => 10,
            Self::BrightYellow => 11,
            Self::BrightBlue => 12,
            Self::BrightMagenta => 13,
            Self::BrightCyan => 14,
            Self::BrightWhite => 15,
        })
    }

    /// The SGR parameter that selects the slot as a foreground (`39` for default, `7` for reverse video).
    #[must_use]
    pub const fn sgr_foreground(self) -> u8 {
        match (self, self.index()) {
            (Self::ReverseVideo, _) => 7,
            (_, None) => 39,
            (_, Some(i)) if i < 8 => 30 + i,
            (_, Some(i)) => 90 + (i - 8),
        }
    }
}

impl fmt::Display for Ansi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Ansi {
    type Err = ThemeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|slot| slot.as_str() == s)
            .ok_or_else(|| ThemeError::new(ThemeErrorKind::UnknownAnsi, s))
    }
}

/// The mono theme's role bindings. It defines no surface class (§11.1.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MonoRoles {
    /// Canvas.
    pub background: Ansi,
    /// Body text.
    pub foreground: Ansi,
    /// Primary accent.
    pub accent: Ansi,
    /// Structure and informational text.
    pub structure: Ansi,
    /// Success status.
    pub success: Ansi,
    /// Error status.
    pub error: Ansi,
    /// Warning status.
    pub warning: Ansi,
    /// Focus indicator.
    pub focus: Ansi,
    /// Surface boundaries.
    pub border: Ansi,
}

impl MonoRoles {
    /// The slot bound to `role`; `None` for the two surface roles, which mono does not define.
    #[must_use]
    pub const fn get(&self, role: Role) -> Option<Ansi> {
        Some(match role {
            Role::Background => self.background,
            Role::Surface | Role::SurfaceAlt => return None,
            Role::Foreground => self.foreground,
            Role::Accent => self.accent,
            Role::Structure => self.structure,
            Role::Success => self.success,
            Role::Error => self.error,
            Role::Warning => self.warning,
            Role::Focus => self.focus,
            Role::Border => self.border,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for slot in Ansi::ALL {
            assert_eq!(slot.as_str().parse::<Ansi>(), Ok(slot));
        }
        assert!("brightwhite".parse::<Ansi>().is_err());
    }

    #[test]
    fn sgr_codes_follow_the_standard_layout() {
        assert_eq!(Ansi::Default.sgr_foreground(), 39);
        assert_eq!(Ansi::Black.sgr_foreground(), 30);
        assert_eq!(Ansi::White.sgr_foreground(), 37);
        assert_eq!(Ansi::BrightBlack.sgr_foreground(), 90);
        assert_eq!(Ansi::BrightWhite.sgr_foreground(), 97);
        assert_eq!(Ansi::ReverseVideo.sgr_foreground(), 7);
        assert_eq!(Ansi::Blue.index(), Some(4));
        assert_eq!(Ansi::ReverseVideo.index(), None);
    }
}
