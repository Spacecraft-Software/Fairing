// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Evaluating a Nickel theme against the embedded contract (FRN-SRS-040,
//! FRN-SRS-041), at build time only.
//!
//! The theme file need not import the contract: Fairing evaluates a small
//! wrapper, registered beside the theme so every relative import resolves
//! next to it,
//!
//! ```text
//! let __fairing_contract = (<the embedded contract>) in
//! (import "<theme file>") | __fairing_contract
//! ```
//!
//! and fully evaluates it for export, so a contract violation anywhere in the
//! theme surfaces. A failure is rendered as Nickel's own diagnostic in plain
//! text (no ANSI escapes), which `fairing theme check` prints verbatim.
//!
//! The evaluator is recursive and has no budget of its own, so it runs on a
//! worker thread with a large stack and a deadline; a theme that does not
//! finish in time is refused and the worker abandoned (the process is a
//! short-lived CLI invocation, never the splash).

use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use nickel_lang_core::error::Error as NickelError;
use nickel_lang_core::error::report::{ColorOpt, report_as_str};
use nickel_lang_core::eval::cache::CacheImpl;
use nickel_lang_core::program::{Program, ProgramBuilder};
use nickel_lang_core::serialize::{ExportFormat, validate};

use crate::compile::CONTRACT;
use crate::fault::{ThemeError, ThemeErrorKind};

/// How long a theme may take to evaluate.
///
/// The reference theme evaluates in milliseconds; a theme still running after
/// five seconds is looping, and a build that waits forever is worse than a
/// refusal naming the cause. The deadline also bounds memory: a runaway
/// evaluation grows by the better part of a gigabyte a second. The process
/// exits on the refusal, which ends the evaluation thread with it.
pub const EVALUATION_DEADLINE: Duration = Duration::from_secs(5);

/// Stack size of the evaluation thread.
///
/// Nickel's evaluator recurses on the term structure; the default 2 MiB
/// thread stack overflows (aborting the process) on deeply nested input that
/// 64 MiB evaluates normally.
const EVALUATION_STACK: usize = 64 * 1024 * 1024;

/// Largest theme file evaluated; the reference theme is under 2 KiB.
pub const MAX_SOURCE_BYTES: u64 = 1024 * 1024;

/// Deepest bracket nesting evaluated.
///
/// Nickel recurses on nesting, and a deep enough literal overflows even the
/// 64 MiB evaluation stack, which aborts the process instead of returning an
/// error. A theme needs a handful of levels. Files the theme imports are not
/// scanned; this bounds the file the operator hands over.
pub const MAX_NESTING: usize = 256;

/// Name of the wrapper source, inside the theme's directory so its relative
/// import resolves there; the angle brackets cannot collide with a real file.
const WRAPPER_NAME: &str = "<fairing: theme with the embedded contract>";

/// Evaluates the theme at `path` with the contract applied, as JSON data.
///
/// # Errors
///
/// [`ThemeErrorKind::Contract`] carrying Nickel's plain-text diagnostic for a
/// parse, import, type or contract error; [`ThemeErrorKind::InvalidSource`]
/// for a path that cannot be spliced into Nickel, a deadline overrun or a
/// value JSON cannot hold.
///
/// Implements: FRN-SRS-040, FRN-SRS-041
pub fn evaluate(path: &Path) -> Result<serde_json::Value, ThemeError> {
    let absolute = std::path::absolute(path)
        .map_err(|e| source_error(format!("cannot resolve `{}`: {e}", path.display())))?;
    let directory = absolute
        .parent()
        .ok_or_else(|| source_error(format!("`{}` has no directory", absolute.display())))?
        .to_path_buf();
    let file = absolute
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            source_error(format!("`{}` is not a UTF-8 file name", absolute.display()))
        })?;
    let source = format!(
        "let __fairing_contract = (\n{CONTRACT}\n) in (import {}) | __fairing_contract\n",
        string_literal(file)?
    );
    let wrapper = directory.join(WRAPPER_NAME).into_os_string();
    check_shape(&absolute)?;

    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("nickel-eval".to_owned())
        .stack_size(EVALUATION_STACK)
        .spawn(move || {
            let outcome = ProgramBuilder::new()
                .add_source_string(source, wrapper)
                .build::<CacheImpl>()
                .map_err(|e| source_error(format!("cannot start the evaluator: {e}")))
                .and_then(evaluate_program);
            // The receiver may have given up on the deadline; nothing to do then.
            let _ = tx.send(outcome);
        })
        .map_err(|e| source_error(format!("cannot start the evaluation thread: {e}")))?;
    match rx.recv_timeout(EVALUATION_DEADLINE) {
        Ok(outcome) => outcome,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(source_error(format!(
            "the theme did not finish evaluating within {} s",
            EVALUATION_DEADLINE.as_secs()
        ))),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err(source_error("the evaluator stopped without a result"))
        }
    }
}

fn evaluate_program(mut program: Program<CacheImpl>) -> Result<serde_json::Value, ThemeError> {
    let value = match program.eval_full_for_export() {
        Ok(value) => value,
        Err(error) => return Err(diagnostic(&program, error)),
    };
    // A value JSON cannot hold (a function left in a field) gets Nickel's
    // positioned diagnostic, not serde's bare message.
    if let Err(error) = validate(ExportFormat::Json, &value) {
        let error: NickelError = error.with_pos_table(program.pos_table().clone()).into();
        return Err(diagnostic(&program, error));
    }
    serde_json::to_value(&value)
        .map_err(|e| source_error(format!("the evaluated theme is not JSON data: {e}")))
}

/// Nickel's own diagnostic for `error`, as plain text (FRN-SRS-041).
fn diagnostic(program: &Program<CacheImpl>, error: NickelError) -> ThemeError {
    let mut files = program.files();
    let report = report_as_str(&mut files, error, ColorOpt::Never);
    ThemeError::new(ThemeErrorKind::Contract, report)
}

/// Refuses a theme file too large or nested too deep to evaluate safely. A
/// file that cannot be read is left for Nickel to report.
fn check_shape(path: &Path) -> Result<(), ThemeError> {
    let Ok(metadata) = std::fs::metadata(path) else {
        return Ok(());
    };
    if metadata.len() > MAX_SOURCE_BYTES {
        return Err(source_error(format!(
            "`{}` is {} bytes; a theme file is at most {MAX_SOURCE_BYTES}",
            path.display(),
            metadata.len()
        )));
    }
    let Ok(text) = std::fs::read(path) else {
        return Ok(());
    };
    let depth = nesting_depth(&text);
    if depth > MAX_NESTING {
        return Err(source_error(format!(
            "`{}` nests brackets {depth} deep; a theme may nest at most {MAX_NESTING}",
            path.display()
        )));
    }
    Ok(())
}

/// The deepest nesting of `(`, `[` and `{` in `text`. Brackets inside strings
/// and comments count too, which can only overstate the depth.
fn nesting_depth(text: &[u8]) -> usize {
    let mut depth = 0_usize;
    let mut deepest = 0_usize;
    for byte in text {
        match byte {
            b'(' | b'[' | b'{' => {
                depth += 1;
                deepest = deepest.max(depth);
            }
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    deepest
}

/// `text` as a double-quoted Nickel string: `\`, `"` and `%` (which starts an
/// interpolation) are escaped; a control character is refused.
fn string_literal(text: &str) -> Result<String, ThemeError> {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '\\' | '"' | '%' => {
                out.push('\\');
                out.push(ch);
            }
            ch if ch.is_control() => {
                return Err(source_error(format!(
                    "the file name {text:?} holds a control character"
                )));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
    Ok(out)
}

fn source_error(message: impl Into<String>) -> ThemeError {
    ThemeError::new(ThemeErrorKind::InvalidSource, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes/steelbore.ncl")
    }

    #[test]
    fn the_reference_theme_satisfies_the_contract() {
        // Verifies: FRN-SRS-040
        let value = evaluate(&reference()).unwrap_or_else(|e| panic!("{e}: {}", e.input()));
        assert_eq!(value["palette"], "steelbore");
        assert_eq!(value["schema_version"], 1, "the contract's default applies");
        assert_eq!(value["layouts"]["shutdown"]["status"]["show"], true);
    }

    #[test]
    fn a_contract_violation_is_reported_as_plain_text() {
        // Verifies: FRN-SRS-041
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let theme = std::fs::read_to_string(reference()).unwrap_or_else(|e| panic!("{e}"));
        let broken = theme.replace("width = 640,", "width = \"wide\",");
        assert_ne!(broken, theme);
        let path = dir.path().join("broken.ncl");
        std::fs::write(&path, broken).unwrap_or_else(|e| panic!("{e}"));
        let error = evaluate(&path).err().unwrap_or_else(|| panic!("accepted"));
        assert_eq!(error.kind(), ThemeErrorKind::Contract);
        let report = error.input();
        assert!(report.contains("width"), "{report}");
        assert!(!report.contains('\u{1b}'), "no ANSI escapes: {report}");
    }

    #[test]
    fn literal_colours_and_syntax_errors_are_refused_by_nickel() {
        // Verifies: FRN-SRS-044
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let theme = std::fs::read_to_string(reference()).unwrap_or_else(|e| panic!("{e}"));
        let literal = dir.path().join("literal.ncl");
        std::fs::write(
            &literal,
            theme.replace("fill = 'accent", "fill = \"#FF5E00\""),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            evaluate(&literal).err().map(|e| e.kind()),
            Some(ThemeErrorKind::Contract)
        );
        let syntax = dir.path().join("syntax.ncl");
        std::fs::write(&syntax, "{ name = ").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            evaluate(&syntax).err().map(|e| e.kind()),
            Some(ThemeErrorKind::Contract)
        );
    }

    #[test]
    fn imports_resolve_beside_the_theme_and_names_are_escaped() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let theme = std::fs::read_to_string(reference()).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(dir.path().join("bar.ncl"), "{ x = 640 }").unwrap_or_else(|e| panic!("{e}"));
        let importing = theme.replace("x = 640,", "x = (import \"bar.ncl\").x,");
        let path = dir.path().join("we\"ird%{name}.ncl");
        std::fs::write(&path, importing).unwrap_or_else(|e| panic!("{e}"));
        let value = evaluate(&path).unwrap_or_else(|e| panic!("{e}: {}", e.input()));
        assert_eq!(value["layouts"]["boot"]["bar"]["x"], 640);
        assert!(string_literal("a\nb").is_err());
    }

    #[test]
    fn deep_nesting_is_refused_before_nickel_sees_it() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let path = dir.path().join("deep.ncl");
        let depth = 100_000;
        let text = format!("{}1{}", "[".repeat(depth), "]".repeat(depth));
        std::fs::write(&path, text).unwrap_or_else(|e| panic!("{e}"));
        let error = evaluate(&path).err().unwrap_or_else(|| panic!("evaluated"));
        assert_eq!(error.kind(), ThemeErrorKind::InvalidSource);
        assert!(error.to_string().contains("nests brackets"), "{error}");
        assert_eq!(nesting_depth(b"{ a = [ (1) ] }"), 3);
    }

    #[test]
    fn the_reference_theme_file_is_the_builtin_layout() {
        // The file and `BootLayout::builtin()` must not drift apart.
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes/steelbore.ncl");
        let theme = crate::compile::compile_file(&path).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(theme.meta().boot, crate::artefact::BootLayout::builtin());
        assert_eq!(
            theme.meta().shutdown,
            crate::artefact::ShutdownLayout::builtin()
        );
    }
}
