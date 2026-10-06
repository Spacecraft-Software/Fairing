// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! `cargo xtask req-texi [--check]`: generate or verify the Texinfo chapters.

use std::path::Path;

use serde::Serialize;

use super::{texinfo, validate};
use crate::args::Args;
use crate::paths;
use crate::report::{Envelope, Failure, print_findings};

/// Generated file names, relative to `--out-dir`.
pub const NEEDS_FILE: &str = "needs.texi";
/// Generated file names, relative to `--out-dir`.
pub const REQUIREMENTS_FILE: &str = "requirements.texi";

#[derive(Debug, Serialize)]
struct FileStatus {
    path: String,
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct Output {
    needs: usize,
    requirements: usize,
    check: bool,
    files: Vec<FileStatus>,
}

/// Runs the verb.
///
/// # Errors
///
/// Usage, not-found, or gate failures (invalid set, stale generated files).
pub fn run(args: &Args) -> Result<(), Failure> {
    args.ensure_known(&["requirements", "out-dir", "check"])?;
    let root = args.root();
    let toml_path = paths::resolve(
        &root,
        args.value("requirements").unwrap_or(super::DEFAULT_PATH),
    );
    let out_dir = paths::resolve(&root, args.value("out-dir").unwrap_or("doc"));
    let check = args.flag("check");

    let set = super::load(&toml_path)?;
    let errors = validate::validate(&set);
    if !errors.is_empty() {
        print_findings("ERROR", &errors);
        return Err(Failure::gate(
            format!(
                "`{}` has {} validation error(s)",
                paths::relative(&root, &toml_path),
                errors.len()
            ),
            "cargo xtask req-texi",
        ));
    }
    if !args.json() {
        print_findings("WARN", &validate::lint(&set));
    }

    let outputs = [
        (out_dir.join(NEEDS_FILE), texinfo::render_needs(&set)),
        (
            out_dir.join(REQUIREMENTS_FILE),
            texinfo::render_requirements(&set),
        ),
    ];
    let mut files = Vec::new();
    let mut stale = Vec::new();
    for (path, content) in &outputs {
        let rel = paths::relative(&root, path);
        let status = if check {
            if current(path, content) {
                "current"
            } else {
                "stale"
            }
        } else {
            std::fs::create_dir_all(&out_dir)?;
            std::fs::write(path, content)?;
            "written"
        };
        if status == "stale" {
            stale.push(rel.clone());
        }
        files.push(FileStatus { path: rel, status });
    }

    let output = Output {
        needs: set.need.len(),
        requirements: set.requirement.len(),
        check,
        files,
    };
    if args.json() {
        Envelope::new(&args.command_line(), output).print()?;
    } else {
        for file in &output.files {
            println!("[OK] {} {}", file.path, file.status);
        }
    }
    if stale.is_empty() {
        Ok(())
    } else {
        Err(Failure::gate(
            format!("generated file(s) out of date: {}", stale.join(", ")),
            "cargo xtask req-texi",
        ))
    }
}

fn current(path: &Path, expected: &str) -> bool {
    std::fs::read(path).is_ok_and(|bytes| bytes == expected.as_bytes())
}
