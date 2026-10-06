// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Binary entry point for `cargo xtask`; all logic lives in the library.

#![forbid(unsafe_code)]

use std::process::ExitCode;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let json = argv.iter().any(|a| a == "--json");
    let command = std::iter::once("xtask".to_owned())
        .chain(argv.iter().cloned())
        .collect::<Vec<_>>()
        .join(" ");
    match xtask::run(argv) {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            failure.emit(json, &command);
            ExitCode::from(failure.exit_code())
        }
    }
}
