// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Everything written to stdout: the mode cascade, the response envelope and
//! the one function that writes a line.
//!
//! stdout carries data only; every diagnostic goes to stderr (CLI Standard §7).

pub mod envelope;
pub mod mode;

use std::io::{self, Write as _};

use crate::error::AppError;

/// Writes one line of data to stdout.
///
/// A reader that closes the pipe early (`fairing schema | head -1`) is making a
/// choice, not reporting a fault: on `EPIPE` the process ends quietly with
/// status 0 instead of panicking the way `println!` does. Any other write
/// failure is an internal error carrying `command`.
///
/// # Errors
///
/// `INTERNAL_ERROR` when stdout cannot be written for a reason other than a
/// closed pipe.
pub fn write_line(line: &str, command: &str) -> Result<(), AppError> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    let written = handle
        .write_all(line.as_bytes())
        .and_then(|()| handle.write_all(b"\n"))
        .and_then(|()| handle.flush());
    match written {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => std::process::exit(0),
        Err(error) => Err(AppError::internal(
            format!("writing to stdout: {error}"),
            command,
        )),
    }
}
