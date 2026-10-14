// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! systemd ask-password agent for Fairing.
//!
//! The agent watches `/run/systemd/ask-password/` with inotify, parses the
//! bounded `ask.*` files, replies on the `Socket=` path and zeroises every
//! buffer that held secret bytes (milestone M3, FRN-SRS-020 to FRN-SRS-029,
//! FRN-SRS-102, FRN-SRS-104). [`ask`] is the question file's parser, [`secret`] the typed secret and
//! [`reply`] the answer datagram.
//!
//! **Assurance Category A** (raised subsystem): every parser is fuzzed, every
//! allocation on the prompt path has a documented worst-case bound, and
//! secrets are zeroised eagerly on reply, cancel, expiry and error. The
//! release profile sets `panic = "abort"`, which skips `Drop`, so zeroisation
//! never relies on drop glue alone.

#![forbid(unsafe_code)]

pub mod ask;
pub mod reply;
pub mod secret;
pub mod watch;
