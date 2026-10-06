// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Bidirectional traceability (Steelbore Standard §21.3).
//!
//! Source files cite requirements with `Verifies: FRN-SRS-012` (evidence) or
//! `Implements: FRN-SRS-012` (design element). [`markers`] collects them,
//! [`matrix`] joins them against the requirement set and decides verdicts, and
//! [`command`] writes the matrix as a build artifact and fails the gate on
//! orphans or unknown identifiers.

pub mod command;
pub mod markers;
pub mod matrix;

pub use markers::{Marker, MarkerKind};
pub use matrix::{Matrix, Verdict};
