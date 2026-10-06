// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Role tokens as drawable colours for one theme selection.
//!
//! A colour theme maps straight through. The mono theme binds roles to ANSI
//! slots whose hue belongs to the terminal; on a framebuffer there is no
//! terminal, so the Linux console's own default palette stands in for the
//! slots. Those sixteen values are kernel data, not brand colours
//! (`drivers/tty/vt/vt.c`, `default_red`/`default_grn`/`default_blu`), and
//! this module is the only place they appear.

use std::fmt;

use fairing_theme::{Ansi, MonoRoles, Rgb, Role, Selection, Theme};

/// The Linux virtual console's default 16-colour palette, indexed by ANSI slot
/// (`drivers/tty/vt/vt.c`: `default_red`, `default_grn`, `default_blu`; the
/// kernel orders them black, red, green, brown, blue, magenta, cyan, grey, then
/// the bright half). The console's default attribute is slot 7 on slot 0.
const CONSOLE_PALETTE: [Rgb; 16] = [
    Rgb::new(0x00, 0x00, 0x00),
    Rgb::new(0xAA, 0x00, 0x00),
    Rgb::new(0x00, 0xAA, 0x00),
    Rgb::new(0xAA, 0x55, 0x00),
    Rgb::new(0x00, 0x00, 0xAA),
    Rgb::new(0xAA, 0x00, 0xAA),
    Rgb::new(0x00, 0xAA, 0xAA),
    Rgb::new(0xAA, 0xAA, 0xAA),
    Rgb::new(0x55, 0x55, 0x55),
    Rgb::new(0xFF, 0x55, 0x55),
    Rgb::new(0x55, 0xFF, 0x55),
    Rgb::new(0xFF, 0xFF, 0x55),
    Rgb::new(0x55, 0x55, 0xFF),
    Rgb::new(0xFF, 0x55, 0xFF),
    Rgb::new(0x55, 0xFF, 0xFF),
    Rgb::new(0xFF, 0xFF, 0xFF),
];

/// The console's default foreground slot (grey) and background slot (black).
const CONSOLE_DEFAULT_FOREGROUND: usize = 7;
const CONSOLE_DEFAULT_BACKGROUND: usize = 0;

/// Drawable colours for every §11.1 role under one selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    slug: &'static str,
    colors: [Rgb; 11],
}

impl Palette {
    /// The palette for a theme selection.
    #[must_use]
    pub fn from_selection(selection: Selection) -> Self {
        match selection {
            Selection::Colour(theme) => Self::from_theme(theme),
            Selection::Mono(mono) => Self::from_mono(mono, selection.slug()),
        }
    }

    /// The palette of a registered colour theme.
    #[must_use]
    pub fn from_theme(theme: &'static Theme) -> Self {
        Self {
            slug: theme.slug,
            colors: Role::ALL.map(|role| theme.color(role)),
        }
    }

    fn from_mono(mono: &MonoRoles, slug: &'static str) -> Self {
        let background = mono
            .get(Role::Background)
            .map_or(CONSOLE_PALETTE[CONSOLE_DEFAULT_BACKGROUND], |slot| {
                console_color(slot, true)
            });
        let colors = Role::ALL.map(|role| match mono.get(role) {
            // Mono defines no surface class; surfaces dissolve into the canvas.
            None => background,
            Some(slot) if role == Role::Background => console_color(slot, true),
            Some(slot) => console_color(slot, false),
        });
        Self { slug, colors }
    }

    /// The theme slug this palette renders.
    #[must_use]
    pub const fn slug(&self) -> &'static str {
        self.slug
    }

    /// The colour bound to `role`.
    #[must_use]
    pub fn color(&self, role: Role) -> Rgb {
        let index = Role::ALL
            .iter()
            .position(|candidate| *candidate == role)
            .unwrap_or(0);
        self.colors[index]
    }
}

impl fmt::Display for Palette {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.slug)
    }
}

/// The console colour for an ANSI slot drawn as a background or a foreground.
///
/// `default` is the console's default attribute (grey on black); reverse video
/// swaps the two, so a reverse-video foreground reads as the default
/// background and a reverse-video background as the default foreground.
fn console_color(slot: Ansi, background: bool) -> Rgb {
    if let Some(index) = slot.index() {
        return CONSOLE_PALETTE[usize::from(index) & 0xF];
    }
    let reversed = slot == Ansi::ReverseVideo;
    if background == reversed {
        CONSOLE_PALETTE[CONSOLE_DEFAULT_FOREGROUND]
    } else {
        CONSOLE_PALETTE[CONSOLE_DEFAULT_BACKGROUND]
    }
}

#[cfg(test)]
mod tests {
    use fairing_theme::{Request, resolve};

    use super::*;

    #[test]
    fn colour_theme_maps_roles_verbatim() {
        let theme = Theme::family_default();
        let palette = Palette::from_theme(theme);
        for role in Role::ALL {
            assert_eq!(palette.color(role), theme.color(role), "{role}");
        }
        assert_eq!(palette.slug(), theme.slug);
    }

    #[test]
    fn mono_uses_console_slots_and_dissolves_surfaces() {
        let resolution = resolve(&Request {
            no_color: true,
            ..Request::default()
        });
        let palette = Palette::from_selection(resolution.selection);
        assert_eq!(palette.slug(), fairing_theme::MONO_SLUG);
        let background = palette.color(Role::Background);
        assert_eq!(background, CONSOLE_PALETTE[CONSOLE_DEFAULT_BACKGROUND]);
        assert_eq!(palette.color(Role::Surface), background);
        assert_eq!(palette.color(Role::SurfaceAlt), background);
        assert_eq!(
            palette.color(Role::Foreground),
            CONSOLE_PALETTE[CONSOLE_DEFAULT_FOREGROUND]
        );
        // Every slot the file binds resolves to its console entry.
        let Selection::Mono(mono) = resolution.selection else {
            panic!("expected mono");
        };
        for role in [
            Role::Accent,
            Role::Structure,
            Role::Success,
            Role::Error,
            Role::Warning,
        ] {
            let slot = mono.get(role).unwrap_or(Ansi::Default);
            let index = usize::from(slot.index().unwrap_or(0));
            assert_eq!(palette.color(role), CONSOLE_PALETTE[index], "{role} {slot}");
        }
        // Default is grey on black; reverse video swaps the pair.
        assert_eq!(
            console_color(Ansi::Default, false),
            CONSOLE_PALETTE[CONSOLE_DEFAULT_FOREGROUND]
        );
        assert_eq!(
            console_color(Ansi::Default, true),
            CONSOLE_PALETTE[CONSOLE_DEFAULT_BACKGROUND]
        );
        assert_eq!(
            console_color(Ansi::ReverseVideo, false),
            CONSOLE_PALETTE[CONSOLE_DEFAULT_BACKGROUND]
        );
        assert_eq!(
            console_color(Ansi::ReverseVideo, true),
            CONSOLE_PALETTE[CONSOLE_DEFAULT_FOREGROUND]
        );
    }
}
