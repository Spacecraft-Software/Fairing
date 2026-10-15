// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Any bytes as a compiled theme: decoding returns a theme or an error and
//! never panics; every image a decoded theme names can be read back.

#![no_main]

use fairing_theme::CompiledTheme;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(theme) = CompiledTheme::from_bytes(data) {
        for index in 0..theme.meta().images.len() {
            let _image = theme.image(u32::try_from(index).unwrap_or(u32::MAX));
        }
    }
});
