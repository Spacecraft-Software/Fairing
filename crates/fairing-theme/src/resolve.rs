// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Theme variant selection (Steelbore Standard §11.6; FRN-SRS-045).
//!
//! Stage 1 picks a base palette from the first usable source: the kernel
//! parameter, `SPACECRAFT_THEME`, the theme's declared default, the family
//! default. Stage 2 picks a variant of that palette: a pinned sibling stays,
//! `NO_COLOR` selects mono, accessible mode selects the high-contrast sibling.
//! An unusable slug is skipped, never fatal (§11.6.5), and every skip is
//! reported so `--verbose` can say why the machine shows what it shows.

use std::fmt;

use crate::ansi::MonoRoles;
use crate::theme::generated::{DEFAULT, MONO, MONO_SLUG};
use crate::theme::{Theme, Variant};

/// The kernel command-line key that names a theme.
pub const KERNEL_PARAMETER: &str = "fairing.theme";

/// What the process already knows when it selects a theme.
///
/// The caller supplies the values; this crate reads neither `/proc/cmdline`
/// nor the environment, so the output-mode context stays the single reader of
/// `NO_COLOR` (and tests need no environment at all).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Request<'a> {
    /// The value of `fairing.theme=` on the kernel command line.
    pub kernel_parameter: Option<&'a str>,
    /// The value of `SPACECRAFT_THEME`.
    pub environment: Option<&'a str>,
    /// The theme artefact's declared default (from M2); `None` until then.
    pub declared_default: Option<&'a str>,
    /// `NO_COLOR` is set and non-empty.
    pub no_color: bool,
    /// Accessible mode is active (Steelbore Standard §18.1).
    pub accessible: bool,
}

/// Which stage-1 source supplied the selected palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    /// `fairing.theme=<slug>`.
    KernelParameter,
    /// `SPACECRAFT_THEME=<slug>`.
    Environment,
    /// The theme artefact's declared default.
    DeclaredDefault,
    /// `steelbore`, the family default (§11.4).
    FamilyDefault,
}

impl Source {
    /// Stable lowercase name for diagnostics.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::KernelParameter => "kernel-parameter",
            Self::Environment => "environment",
            Self::DeclaredDefault => "declared-default",
            Self::FamilyDefault => "family-default",
        }
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which stage-2 overlay was applied to the stage-1 palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Overlay {
    /// The base palette itself.
    None,
    /// The source named a `-high-contrast` or mono slug outright; no overlay may change it.
    Pinned,
    /// `NO_COLOR` selected the mono theme.
    Mono,
    /// Accessible mode selected the palette's high-contrast sibling.
    HighContrast,
}

impl Overlay {
    /// Stable lowercase name for diagnostics.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Pinned => "pinned",
            Self::Mono => "mono",
            Self::HighContrast => "high-contrast",
        }
    }
}

impl fmt::Display for Overlay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The selected theme: a registered colour theme or the mono bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selection {
    /// A registered colour theme.
    Colour(&'static Theme),
    /// The palette-independent mono theme.
    Mono(&'static MonoRoles),
}

impl Selection {
    /// The selected theme's slug.
    #[must_use]
    pub const fn slug(&self) -> &'static str {
        match self {
            Self::Colour(theme) => theme.slug,
            Self::Mono(_) => MONO_SLUG,
        }
    }

    /// The colour theme, if the selection is one.
    #[must_use]
    pub const fn theme(&self) -> Option<&'static Theme> {
        match self {
            Self::Colour(theme) => Some(theme),
            Self::Mono(_) => None,
        }
    }
}

impl fmt::Display for Selection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.slug())
    }
}

/// A source that named a slug nobody registered, skipped per §11.6.5.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// Where the slug came from.
    pub source: Source,
    /// The slug as supplied.
    pub slug: String,
}

/// The outcome of [`resolve`]: what was selected, from where, and what was skipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    /// The theme to draw with.
    pub selection: Selection,
    /// The stage-1 palette, reported even when an overlay or a pinned mono slug hides it (§11.6.3).
    pub base: &'static Theme,
    /// The stage-1 source that won.
    pub source: Source,
    /// The stage-2 overlay that was applied.
    pub overlay: Overlay,
    /// Sources skipped for naming an unregistered slug, in precedence order.
    pub skipped: Vec<Skipped>,
}

/// Selects the theme for this process (two-stage §11.6 resolution).
///
/// Implements: FRN-SRS-045
#[must_use]
pub fn resolve(request: &Request<'_>) -> Resolution {
    let candidates = [
        (Source::KernelParameter, request.kernel_parameter),
        (Source::Environment, request.environment),
        (Source::DeclaredDefault, request.declared_default),
    ];
    let mut skipped = Vec::new();
    let mut picked = None;
    for (source, slug) in candidates {
        // An unset or blank value is "no opinion", not a typo worth reporting.
        let Some(slug) = slug.map(str::trim).filter(|s| !s.is_empty()) else {
            continue;
        };
        match lookup(slug) {
            Some(selection) => {
                picked = Some((source, selection));
                break;
            }
            None => skipped.push(Skipped {
                source,
                slug: slug.to_owned(),
            }),
        }
    }
    let (source, stage_one) = picked.unwrap_or((Source::FamilyDefault, Selection::Colour(DEFAULT)));
    let base = match stage_one {
        Selection::Colour(theme) => theme.base_theme(),
        Selection::Mono(_) => DEFAULT,
    };

    let (selection, overlay) = match stage_one {
        Selection::Mono(mono) => (Selection::Mono(mono), Overlay::Pinned),
        Selection::Colour(theme) if theme.variant == Variant::HighContrast => {
            (Selection::Colour(theme), Overlay::Pinned)
        }
        Selection::Colour(theme) => {
            if request.no_color {
                (Selection::Mono(&MONO), Overlay::Mono)
            } else if request.accessible
                && let Some(lifted) = theme.high_contrast()
            {
                (Selection::Colour(lifted), Overlay::HighContrast)
            } else {
                (Selection::Colour(theme), Overlay::None)
            }
        }
    };
    Resolution {
        selection,
        base,
        source,
        overlay,
        skipped,
    }
}

fn lookup(slug: &str) -> Option<Selection> {
    if slug == MONO_SLUG {
        Some(Selection::Mono(&MONO))
    } else {
        Theme::find(slug).map(Selection::Colour)
    }
}

/// Extracts the last `fairing.theme=<slug>` from a kernel command line.
#[must_use]
pub fn kernel_parameter(cmdline: &str) -> Option<&str> {
    kernel_value(cmdline, KERNEL_PARAMETER)
}

/// The value of `key=` on a kernel command line, tokenised as the kernel does.
///
/// Mirrors `lib/cmdline.c` `next_arg`: words split on whitespace, a word may
/// begin with `"` and then runs to the closing quote, and the value is
/// everything after the first `=` with one leading and one trailing `"`
/// stripped. The last occurrence wins, as it does for the kernel and for
/// systemd. A key given without `=` is not a value.
#[must_use]
pub fn kernel_value<'a>(cmdline: &'a str, key: &str) -> Option<&'a str> {
    let mut found = None;
    for word in kernel_words(cmdline) {
        let unquoted = word.strip_prefix('"').unwrap_or(word);
        if let Some(value) = unquoted
            .strip_prefix(key)
            .and_then(|rest| rest.strip_prefix('='))
        {
            let value = value.strip_prefix('"').unwrap_or(value);
            found = Some(value.strip_suffix('"').unwrap_or(value));
        }
    }
    found
}

/// Splits a command line into words, keeping quoted whitespace inside a word.
fn kernel_words(cmdline: &str) -> impl Iterator<Item = &str> {
    let mut in_quotes = false;
    cmdline
        .split(move |c: char| {
            if c == '"' {
                in_quotes = !in_quotes;
            }
            c.is_ascii_whitespace() && !in_quotes
        })
        .filter(|word| !word.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::generated::DEFAULT_SLUG;

    fn request() -> Request<'static> {
        Request::default()
    }

    #[test]
    fn nothing_set_gives_the_family_default() {
        // Verifies: FRN-SRS-045
        let resolution = resolve(&request());
        assert_eq!(resolution.selection.slug(), DEFAULT_SLUG);
        assert_eq!(resolution.source, Source::FamilyDefault);
        assert_eq!(resolution.overlay, Overlay::None);
        assert_eq!(resolution.base, DEFAULT);
        assert!(resolution.skipped.is_empty());
    }

    #[test]
    fn blank_values_are_absent_not_skipped() {
        // Verifies: FRN-SRS-045
        let resolution = resolve(&Request {
            kernel_parameter: Some("  "),
            environment: Some(""),
            ..request()
        });
        assert_eq!(resolution.source, Source::FamilyDefault);
        assert!(resolution.skipped.is_empty());
    }

    #[test]
    fn stage_one_order_is_kernel_then_environment_then_declared() {
        // Verifies: FRN-SRS-045
        let full = Request {
            kernel_parameter: Some("steelbore-blue"),
            environment: Some("steelbore-green"),
            declared_default: Some("tokyonight"),
            ..request()
        };
        assert_eq!(resolve(&full).selection.slug(), "steelbore-blue");
        assert_eq!(resolve(&full).source, Source::KernelParameter);

        let no_kernel = Request {
            kernel_parameter: None,
            ..full
        };
        assert_eq!(resolve(&no_kernel).selection.slug(), "steelbore-green");
        assert_eq!(resolve(&no_kernel).source, Source::Environment);

        let declared_only = Request {
            environment: None,
            ..no_kernel
        };
        assert_eq!(resolve(&declared_only).selection.slug(), "tokyonight");
        assert_eq!(resolve(&declared_only).source, Source::DeclaredDefault);
    }

    #[test]
    fn unusable_slugs_are_skipped_never_fatal() {
        // Verifies: FRN-SRS-045
        let resolution = resolve(&Request {
            kernel_parameter: Some("no-such-theme"),
            environment: Some("Steelbore"),
            declared_default: Some("steelbore-magnetar"),
            ..request()
        });
        assert_eq!(resolution.selection.slug(), "steelbore-magnetar");
        assert_eq!(resolution.source, Source::DeclaredDefault);
        assert_eq!(
            resolution.skipped,
            vec![
                Skipped {
                    source: Source::KernelParameter,
                    slug: "no-such-theme".to_owned()
                },
                Skipped {
                    source: Source::Environment,
                    slug: "Steelbore".to_owned()
                },
            ]
        );
    }

    #[test]
    fn no_color_overlays_mono_and_accessible_overlays_high_contrast() {
        // Verifies: FRN-SRS-045
        let mono = resolve(&Request {
            environment: Some("steelbore-biolume"),
            no_color: true,
            accessible: true,
            ..request()
        });
        assert_eq!(mono.selection, Selection::Mono(&MONO));
        assert_eq!(mono.overlay, Overlay::Mono);
        assert_eq!(mono.source, Source::Environment);
        assert_eq!(
            mono.base.slug, "steelbore-biolume",
            "stage-1 palette is still reported"
        );

        let lifted = resolve(&Request {
            environment: Some("steelbore-biolume"),
            accessible: true,
            ..request()
        });
        assert_eq!(lifted.selection.slug(), "steelbore-biolume-high-contrast");
        assert_eq!(lifted.overlay, Overlay::HighContrast);
    }

    #[test]
    fn pinned_siblings_are_never_overridden() {
        // Verifies: FRN-SRS-045
        let pinned = resolve(&Request {
            kernel_parameter: Some("steelbore-high-contrast"),
            no_color: true,
            ..request()
        });
        assert_eq!(pinned.selection.slug(), "steelbore-high-contrast");
        assert_eq!(pinned.overlay, Overlay::Pinned);
        assert_eq!(pinned.base.slug, "steelbore");

        let mono = resolve(&Request {
            environment: Some(MONO_SLUG),
            accessible: true,
            ..request()
        });
        assert_eq!(mono.selection, Selection::Mono(&MONO));
        assert_eq!(mono.overlay, Overlay::Pinned);
    }

    #[test]
    fn kernel_parameter_is_parsed_with_last_wins_and_quotes() {
        // Verifies: FRN-SRS-045
        assert_eq!(
            kernel_parameter("quiet fairing.theme=steelbore-blue splash"),
            Some("steelbore-blue")
        );
        assert_eq!(
            kernel_parameter("fairing.theme=a fairing.theme=\"tokyonight\""),
            Some("tokyonight")
        );
        assert_eq!(kernel_parameter("quiet splash"), None);
        assert_eq!(kernel_parameter("fairing.themes=x"), None);
        assert_eq!(kernel_parameter("fairing.theme"), None);
        // The kernel's own tokeniser: a quote may precede the key, and quoted spaces stay.
        assert_eq!(
            kernel_parameter("\"fairing.theme=steelbore-green\" quiet"),
            Some("steelbore-green")
        );
        assert_eq!(
            kernel_value("init=\"/bin/sh -c x\" fairing.theme=a", "init"),
            Some("/bin/sh -c x")
        );
        assert_eq!(
            kernel_value("fairing.accessible=1\n", "fairing.accessible"),
            Some("1")
        );
        assert_eq!(kernel_value("", "x"), None);
    }

    #[test]
    fn names_are_stable() {
        assert_eq!(Source::KernelParameter.to_string(), "kernel-parameter");
        assert_eq!(Overlay::HighContrast.to_string(), "high-contrast");
        assert_eq!(Selection::Mono(&MONO).to_string(), MONO_SLUG);
        assert_eq!(Selection::Colour(DEFAULT).theme(), Some(DEFAULT));
    }
}
