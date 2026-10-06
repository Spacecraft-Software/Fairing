// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Everything written to stdout: the mode cascade and the response envelope.
//!
//! stdout carries data only; every diagnostic goes to stderr (CLI Standard §7).

pub mod envelope;
pub mod mode;
