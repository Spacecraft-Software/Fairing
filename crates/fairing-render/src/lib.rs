// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Rendering backends and frame compositor for Fairing.
//!
//! This crate will own the backend trait with its DRM/KMS, fbdev and text
//! implementations and the compositor that draws the logo, bar, percentage
//! text and status line (milestone M1, FRN-SRS-001 to FRN-SRS-007 and
//! FRN-SRS-053). In M0 it is a compile-only stub: no business logic yet.
//!
//! Assurance Category B.

#![forbid(unsafe_code)]
