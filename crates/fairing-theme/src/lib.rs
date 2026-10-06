// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Compiled theme format and Nickel theme tooling for Fairing.
//!
//! This crate will own the postcard-encoded theme artefact the initrd loads,
//! the Nickel contract, and the `theme check` / `theme compile` logic
//! (milestone M2, FRN-SRS-040 to FRN-SRS-047). Nickel is evaluated at build
//! time only; no evaluator is linked into the boot path. In M0 it is a
//! compile-only stub.
//!
//! Assurance Category B.

#![forbid(unsafe_code)]
