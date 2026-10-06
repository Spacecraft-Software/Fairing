// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Steelbore palette tokens and theme-variant resolution for Fairing.
//!
//! Every colour Fairing draws is a §11.1 role token ([`Role`]) of a registered
//! theme ([`Theme`]), and every value comes from the vendored house palette
//! file `assets/steelbore.toml`, read at build time by `build.rs` and never
//! retyped (Steelbore Standard §11, §11.6). The mono theme binds the same roles
//! to ANSI slots ([`MonoRoles`]) for `NO_COLOR` and text consoles.
//!
//! [`resolve`] implements the two-stage §11.6 selection adapted to a process
//! with no in-app selector (FRN-SRS-045): a slug named on the command line,
//! then the kernel parameter, then `SPACECRAFT_THEME`, then the theme's
//! declared default, then the family default; a sibling the user named is
//! pinned, otherwise `NO_COLOR` overlays mono and accessible mode overlays the
//! high-contrast sibling. The caller supplies what it already read; this crate
//! reads no environment of its own.
//!
//! Milestone M2 adds the compiled theme artefact (Nickel contract, layout,
//! `theme check` / `theme compile`). Assurance Category B.

#![forbid(unsafe_code)]

mod ansi;
mod fault;
mod resolve;
mod rgb;
mod role;
mod theme;

#[doc(inline)]
pub use ansi::{Ansi, MonoRoles};
#[doc(inline)]
pub use fault::{ThemeError, ThemeErrorKind};
#[doc(inline)]
pub use resolve::{
    KERNEL_PARAMETER, Overlay, Request, Resolution, Selection, Skipped, Source, kernel_parameter,
    kernel_value, resolve,
};
#[doc(inline)]
pub use rgb::Rgb;
#[doc(inline)]
pub use role::Role;
#[doc(inline)]
pub use theme::generated::{
    DEFAULT_SLUG, ENV_VAR, MONO_SLUG, PALETTE_STANDARD, PALETTE_VERSION, SYSTEM_FILE, USER_FILE,
};
#[doc(inline)]
pub use theme::{Polarity, Roles, Theme, Variant};
