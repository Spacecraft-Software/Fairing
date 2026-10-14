// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Any bytes as an `ask.*` file: the parser returns a question or an error,
//! never panics, and a question it returns holds to its own bounds.

#![no_main]

use fairing_askpass::ask::{Ask, MAX_FILE};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(ask) = Ask::parse(data) {
        assert!(data.len() <= MAX_FILE);
        assert!(ask.socket.as_str().starts_with('/'));
        assert!(!ask.socket.as_str().contains('\0'));
        let _expired = ask.expired(u64::MAX);
    }
});
