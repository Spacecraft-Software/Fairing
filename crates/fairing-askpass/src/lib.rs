// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! systemd ask-password agent for Fairing.
//!
//! This crate will watch `/run/systemd/ask-password/` with inotify, parse the
//! bounded `ask.*` files, reply on the `Socket=` path and zeroise every buffer
//! that held secret bytes (milestone M3, FRN-SRS-020 to FRN-SRS-029,
//! FRN-SRS-102, FRN-SRS-104). In M0 it is a compile-only stub.
//!
//! **Assurance Category A** (raised subsystem): every parser is fuzzed, every
//! allocation on the prompt path has a documented worst-case bound, and
//! secrets are zeroised eagerly on reply, cancel, expiry and error. The
//! release profile sets `panic = "abort"`, which skips `Drop`, so zeroisation
//! never relies on drop glue alone.

#![forbid(unsafe_code)]
