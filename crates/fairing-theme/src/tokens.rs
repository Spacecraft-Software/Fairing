// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Named palette tokens: the inventory a theme may refer to by name.
//!
//! A theme draws in §11.1 roles. It may also name a token of its own palette
//! ("Plasma Orange"), which is accepted only when that token is what one of
//! the palette's roles binds, and then stands for that role: the accessible
//! siblings lift roles, not tokens, so a theme that kept a raw token would
//! keep the unlifted value in high contrast. A token of any other palette is a
//! cross-palette mix (§11.4, FRN-SRS-043) and is refused.

use crate::fault::{ThemeError, ThemeErrorKind};
use crate::rgb::Rgb;
use crate::role::Role;
use crate::theme::{Theme, generated};

/// One named colour of a palette, as the palette file spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Token {
    pub(crate) name: &'static str,
    pub(crate) rgb: Rgb,
}

impl Token {
    /// The token's name, e.g. `Plasma Orange`.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The token's value.
    #[must_use]
    pub const fn rgb(&self) -> Rgb {
        self.rgb
    }
}

/// A palette's named tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PaletteTokens {
    pub(crate) slug: &'static str,
    pub(crate) tokens: &'static [Token],
}

impl PaletteTokens {
    /// Every palette in the palette file, registered or not, in file order.
    #[must_use]
    pub const fn all() -> &'static [Self] {
        generated::PALETTES
    }

    /// The palette with `slug`, e.g. `steelbore`.
    #[must_use]
    pub fn find(slug: &str) -> Option<&'static Self> {
        Self::all().iter().find(|p| p.slug == slug)
    }

    /// The palette's slug.
    #[must_use]
    pub const fn slug(&self) -> &'static str {
        self.slug
    }

    /// The palette's tokens, in file order.
    #[must_use]
    pub const fn tokens(&self) -> &'static [Token] {
        self.tokens
    }

    /// The token called `name`, compared exactly.
    #[must_use]
    pub fn token(&self, name: &str) -> Option<&'static Token> {
        self.tokens.iter().find(|t| t.name == name)
    }
}

/// The role a token reference stands for in `theme`'s base palette.
///
/// # Errors
///
/// - [`ThemeErrorKind::ForeignToken`] when `name` is a token of another
///   palette only (FRN-SRS-043); the error input names that palette.
/// - [`ThemeErrorKind::UnboundToken`] when it is a token of the theme's own
///   palette that no role binds (a high-contrast `Lift`, a non-text tint).
/// - [`ThemeErrorKind::UnknownToken`] when no palette has it.
pub fn role_of_token(theme: &'static Theme, name: &str) -> Result<Role, ThemeError> {
    let base = theme.base_theme();
    let own = PaletteTokens::find(base.base);
    if let Some(token) = own.and_then(|palette| palette.token(name)) {
        return Role::ALL
            .into_iter()
            .find(|role| base.color(*role) == token.rgb)
            .ok_or_else(|| {
                ThemeError::new(
                    ThemeErrorKind::UnboundToken,
                    format!("{name} ({})", base.base),
                )
            });
    }
    match PaletteTokens::all()
        .iter()
        .find(|palette| palette.token(name).is_some())
    {
        Some(other) => Err(ThemeError::new(
            ThemeErrorKind::ForeignToken,
            format!("{name} ({})", other.slug),
        )),
        None => Err(ThemeError::new(ThemeErrorKind::UnknownToken, name)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme(slug: &str) -> &'static Theme {
        Theme::find(slug).unwrap_or_else(|| panic!("{slug} is registered"))
    }

    #[test]
    fn every_palette_in_the_file_is_listed_with_its_tokens() {
        let slugs: Vec<&str> = PaletteTokens::all()
            .iter()
            .map(PaletteTokens::slug)
            .collect();
        for slug in [
            "steelbore",
            "steelbore-navywhite",
            "tokyonight",
            "solarized-dark",
        ] {
            assert!(slugs.contains(&slug), "{slug} missing from {slugs:?}");
        }
        let modern = PaletteTokens::find("steelbore").unwrap_or_else(|| panic!("steelbore"));
        let orange = modern
            .token("Plasma Orange")
            .unwrap_or_else(|| panic!("Plasma Orange"));
        assert_eq!(orange.rgb(), theme("steelbore").color(Role::Accent));
        assert!(
            modern.token("reference").is_none(),
            "metadata is not a token"
        );
    }

    #[test]
    fn own_palette_tokens_stand_for_the_role_they_bind() {
        let modern = theme("steelbore");
        assert_eq!(role_of_token(modern, "Plasma Orange"), Ok(Role::Accent));
        assert_eq!(role_of_token(modern, "Void Navy"), Ok(Role::Background));
        // A token two roles share maps to the first in §11.1 order.
        assert_eq!(role_of_token(modern, "Pulse Violet"), Ok(Role::Structure));
        // The sibling resolves through its base palette.
        assert_eq!(
            role_of_token(theme("steelbore-high-contrast"), "Plasma Orange"),
            Ok(Role::Accent)
        );
    }

    #[test]
    fn foreign_unbound_and_unknown_tokens_are_refused() {
        // Verifies: FRN-SRS-043
        let modern = theme("steelbore");
        let foreign = role_of_token(modern, "Electric Blue")
            .err()
            .unwrap_or_else(|| panic!("Electric Blue is Steelbore Blue's"));
        assert_eq!(foreign.kind(), ThemeErrorKind::ForeignToken);
        assert_eq!(foreign.input(), "Electric Blue (steelbore-blue)");
        let lift = role_of_token(modern, "Plasma Orange Lift")
            .err()
            .unwrap_or_else(|| panic!("a Lift binds no base role"));
        assert_eq!(lift.kind(), ThemeErrorKind::UnboundToken);
        let tint = role_of_token(theme("steelbore-navywhite"), "Cerulean Edge (tint)")
            .err()
            .unwrap_or_else(|| panic!("tints bind no role"));
        assert_eq!(tint.kind(), ThemeErrorKind::UnboundToken);
        let unknown = role_of_token(modern, "Hot Lava")
            .err()
            .unwrap_or_else(|| panic!("no palette has Hot Lava"));
        assert_eq!(unknown.kind(), ThemeErrorKind::UnknownToken);
    }
}
