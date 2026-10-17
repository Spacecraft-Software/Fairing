// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! The FRN-SRS-104 analysis, kept honest.
//!
//! The argument (the "Password agent safety argument" node of the manual) is
//! that nothing in the crate allocates by what a question or a secret
//! contains: text goes into fixed `Text<N>` buffers, the secret into the fixed
//! `Secret`, a file into the one buffer the watcher allocates at open. Its
//! enforcement is `clippy.toml`, which bans the growable types, macros and
//! methods such an allocation would come back through, under CI's `-D
//! warnings`. This test checks that enforcement: the bans are in place, and
//! the only code exempt from them is test code.

#![expect(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "a test reading source files as text"
)]

use std::path::Path;

fn crate_file(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn content_sized_allocation_is_banned_outside_tests() {
    // Verifies: FRN-SRS-104
    let config = crate_file("clippy.toml");
    for banned in [
        r#"path = "std::string::String""#,
        r#"path = "std::format""#,
        r#"path = "std::fs::read""#,
        r#"path = "std::fs::read_to_string""#,
        r#"path = "std::io::Read::read_to_end""#,
        r#"path = "std::io::Read::read_to_string""#,
        r#"path = "std::vec::Vec::extend_from_slice""#,
    ] {
        assert!(
            config.contains(banned),
            "clippy.toml no longer bans {banned}"
        );
    }
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = 0;
    for entry in std::fs::read_dir(&src).unwrap_or_else(|e| panic!("{e}")) {
        let path = entry.unwrap_or_else(|e| panic!("{e}")).path();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        files += 1;
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{e}"));
        let tests_start = text.find("#[cfg(test)]").unwrap_or(text.len());
        if let Some(at) = text.find("clippy::disallowed_") {
            assert!(
                at > tests_start,
                "{} exempts code outside its tests from the FRN-SRS-104 bans",
                path.display()
            );
        }
    }
    assert!(
        files >= 6,
        "only {files} source files found in {}",
        src.display()
    );
}
