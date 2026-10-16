// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! `cargo xtask sbom`: the release's `CycloneDX` bill of materials (Steelbore
//! Standard §24.3).
//!
//! `cargo cyclonedx` describes the `fairing` binary from the committed
//! lockfile, for every target, with the full dependency graph. Its output
//! carries a wall-clock timestamp, a random serial number and the absolute
//! path of the checkout; this verb replaces the first with the archive's
//! content time, the second with a UUID derived from the document itself,
//! and the third with `.`, so the same tree gives the same bytes. The
//! manifest committed at the tag can then carry the SBOM's checksum, and the
//! release workflow regenerates exactly that file.
//!
//! `cargo xtask sbom [--ref <commit>] [--out <dir>]`

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::args::Args;
use crate::dist::{self, Checksum};
use crate::paths;
use crate::report::{Envelope, Failure};

/// The cargo-cyclonedx release the SBOM is generated with; CI pins it.
pub const CYCLONEDX_VERSION: &str = "0.5.9";

/// The `CycloneDX` specification version written.
pub const SPEC_VERSION: &str = "1.5";

/// The SBOM written.
#[derive(Debug, Clone, Serialize)]
pub struct Sbom {
    /// The file.
    pub path: PathBuf,
    /// Its size in bytes.
    pub bytes: u64,
    /// Its checksum.
    pub sha256: Checksum,
    /// Components listed.
    pub components: usize,
}

/// Rewrites cargo-cyclonedx's output to depend only on its inputs: the
/// checkout path becomes `.`, the timestamp `timestamp`, and the serial
/// number a UUID taken from `digest` of the rest.
///
/// # Errors
///
/// Output that is not a `CycloneDX` JSON document.
pub fn normalise(
    text: &str,
    root: &str,
    timestamp: &str,
    digest: impl Fn(&[u8]) -> Result<[u8; 32], Failure>,
) -> Result<serde_json::Value, Failure> {
    let text = text.replace(root.trim_end_matches('/'), ".");
    let mut document: serde_json::Value = serde_json::from_str(&text)?;
    let object = document
        .as_object_mut()
        .filter(|o| o.get("bomFormat").and_then(serde_json::Value::as_str) == Some("CycloneDX"))
        .ok_or_else(|| Failure::internal("cargo cyclonedx wrote no CycloneDX document"))?;
    object.remove("serialNumber");
    let metadata = object
        .get_mut("metadata")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| Failure::internal("the SBOM has no metadata"))?;
    metadata.insert("timestamp".into(), timestamp.into());
    let sum = digest(serde_json::to_string(&document)?.as_bytes())?;
    if let Some(object) = document.as_object_mut() {
        object.insert("serialNumber".into(), uuid(&sum).into());
    }
    Ok(document)
}

/// An RFC 9562 version-8 UUID URN from the first 16 bytes of `sum`.
fn uuid(sum: &[u8; 32]) -> String {
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&sum[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex = Checksum::of(&{
        let mut padded = [0u8; 32];
        padded[..16].copy_from_slice(&bytes);
        padded
    })
    .hex;
    format!(
        "urn:uuid:{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// SHA-256 of `bytes` through `sha256sum`, as `dist` takes the archive's.
fn sha256(dir: &Path, bytes: &[u8]) -> Result<[u8; 32], Failure> {
    let scratch = dir.join(".sbom-digest");
    std::fs::write(&scratch, bytes)?;
    let printed = dist::run(dir, "sha256sum", &[&scratch.to_string_lossy()]);
    std::fs::remove_file(&scratch)?;
    let printed = printed?;
    let hex = printed.split_whitespace().next().unwrap_or_default();
    let mut digest = [0u8; 32];
    for (i, byte) in digest.iter_mut().enumerate() {
        *byte = hex
            .get(2 * i..2 * i + 2)
            .and_then(|pair| u8::from_str_radix(pair, 16).ok())
            .ok_or_else(|| Failure::internal(format!("`sha256sum` printed `{printed}`")))?;
    }
    Ok(digest)
}

/// Every `*.cdx.json` cargo-cyclonedx left beside a workspace manifest.
fn generated(root: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![root.join("xtask")];
    if let Ok(entries) = std::fs::read_dir(root.join("crates")) {
        dirs.extend(entries.filter_map(Result::ok).map(|e| e.path()));
    }
    dirs.iter()
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.to_string_lossy().ends_with(".cdx.json"))
        .collect()
}

/// Generates the SBOM of `reference` into `out`.
///
/// # Errors
///
/// A failed `cargo cyclonedx`, or output it cannot normalise.
pub fn build(root: &Path, reference: &str, out: &Path) -> Result<Sbom, Failure> {
    let version = dist::version(root)?;
    let commit = dist::run(
        root,
        "git",
        &["rev-parse", "--verify", &format!("{reference}^{{commit}}")],
    )?;
    let epoch = dist::content_epoch(root, commit.trim())?;
    let timestamp = epoch
        .parse::<i64>()
        .ok()
        .and_then(|s| jiff::Timestamp::from_second(s).ok())
        .ok_or_else(|| Failure::internal(format!("`git log` printed `{epoch}`")))?
        .strftime("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    for stale in generated(root) {
        std::fs::remove_file(stale)?;
    }
    let described = dist::run(
        root,
        "cargo",
        &[
            "cyclonedx",
            "--manifest-path",
            "crates/fairing/Cargo.toml",
            "--format",
            "json",
            "--spec-version",
            SPEC_VERSION,
            "--describe",
            "binaries",
            "--target",
            "all",
            "--quiet",
        ],
    );
    let written = root.join("crates/fairing/fairing_bin.cdx.json");
    let text = described.and_then(|_| paths::read_text(&written, "cargo install cargo-cyclonedx"));
    for file in generated(root) {
        std::fs::remove_file(file)?;
    }
    std::fs::create_dir_all(out)?;
    let document = normalise(&text?, &root.to_string_lossy(), &timestamp, |b| {
        sha256(out, b)
    })?;
    let components = document
        .get("components")
        .and_then(serde_json::Value::as_array)
        .map_or(0, Vec::len);
    let mut bytes = serde_json::to_string_pretty(&document)?;
    bytes.push('\n');
    let path = out.join(format!("fairing-{version}.cdx.json"));
    std::fs::write(&path, &bytes)?;
    Ok(Sbom {
        bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        sha256: Checksum::of(&sha256(out, bytes.as_bytes())?),
        path,
        components,
    })
}

/// `cargo xtask sbom`.
///
/// # Errors
///
/// See [`build`].
pub fn run(args: &Args) -> Result<(), Failure> {
    args.ensure_known(&["ref", "out"])?;
    let root = args.root();
    let out = paths::resolve(&root, args.value("out").unwrap_or(dist::DEFAULT_OUT));
    let sbom = build(&root, args.value("ref").unwrap_or("HEAD"), &out)?;
    if args.json() {
        Envelope::new(&args.command_line(), &sbom).print()?;
    } else {
        println!(
            "[OK] {} ({} components, {} bytes)",
            paths::relative(&root, &sbom.path),
            sbom.components,
            sbom.bytes
        );
        println!("[OK] sha256 {}", sbom.sha256.hex);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[expect(
        clippy::unnecessary_wraps,
        reason = "stands in for the fallible sha256sum digest"
    )]
    fn fake_digest(bytes: &[u8]) -> Result<[u8; 32], Failure> {
        let mut sum = [0u8; 32];
        for (i, b) in bytes.iter().enumerate() {
            sum[i % 32] ^= *b;
        }
        Ok(sum)
    }

    const RAW: &str = r#"{
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "serialNumber": "urn:uuid:random",
        "metadata": {
            "timestamp": "2026-10-16T08:21:15.461863477Z",
            "component": { "bom-ref": "path+file:///work/Fairing/crates/fairing#0.1.0" }
        },
        "components": []
    }"#;

    #[test]
    fn the_sbom_depends_only_on_its_inputs() {
        let one = normalise(RAW, "/work/Fairing", "2026-10-16T00:00:00Z", fake_digest)
            .unwrap_or_else(|e| panic!("{e}"));
        let moved = RAW
            .replace("/work/Fairing", "/elsewhere/x")
            .replace("08:21:15.461863477", "09:00:00")
            .replace("random", "other");
        let two = normalise(&moved, "/elsewhere/x", "2026-10-16T00:00:00Z", fake_digest)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(one, two);
        assert_eq!(one["metadata"]["timestamp"], "2026-10-16T00:00:00Z");
        assert_eq!(
            one["metadata"]["component"]["bom-ref"],
            "path+file://./crates/fairing#0.1.0"
        );
        let serial = one["serialNumber"].as_str().unwrap_or_default();
        assert!(serial.starts_with("urn:uuid:"), "{serial}");
        assert_eq!(serial.len(), 45);
        assert_eq!(&serial[23..24], "8", "version nibble: {serial}");
    }

    #[test]
    fn a_document_that_is_not_cyclonedx_is_refused() {
        assert!(normalise("{}", "/", "t", fake_digest).is_err());
        assert!(normalise("[]", "/", "t", fake_digest).is_err());
    }
}
