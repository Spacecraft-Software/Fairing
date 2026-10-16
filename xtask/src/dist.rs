// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! `cargo xtask dist`: the release source archive and its checksums
//! (Steelbore Standard §5.5).
//!
//! The archive is the tree at a commit (`git archive`) with every crate it
//! builds from vendored in (`cargo vendor --locked`) and cargo pointed at
//! them, so all three package definitions build it offline: Guix and Nix
//! forbid network access in a build, and makepkg's `--frozen` build needs
//! none. It is written reproducibly: entries sorted, owners and modes
//! normalised, every timestamp that of the last commit to change what the
//! archive holds, gzip without a name or time. The same tree gives the same
//! bytes.
//!
//! The package definitions carry the archive's checksum, so they cannot be
//! inside it: `.gitattributes` marks them, and the release manifests under
//! `doc/releases/`, `export-ignore`. Writing the checksums into them after
//! the archive is built changes nothing the archive holds, and the release
//! commit that records them archives to the same bytes.
//!
//! - `cargo xtask dist [--ref <commit>] [--out <dir>]` builds the archive and
//!   writes version and checksum into the three definitions.
//! - `cargo xtask dist --check` builds it again and fails unless the
//!   definitions already carry that checksum: the release workflow's proof
//!   that the tag's packages build the archive it publishes.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use regex::Regex;
use serde::Serialize;

use crate::args::Args;
use crate::paths;
use crate::report::{Envelope, Failure};

/// Where the archive is written, relative to the root.
pub const DEFAULT_OUT: &str = "target/dist";

/// The package definitions that carry the archive's version and checksum.
pub const DEFINITIONS: [&str; 3] = [
    "packaging/default.nix",
    "packaging/guix.scm",
    "packaging/PKGBUILD",
];

/// Where each release's manifest is committed, left out of the archive too:
/// it records the archive's checksum.
pub const RELEASES_DIR: &str = "doc/releases";

/// What cargo is told about the vendored crates.
const VENDOR_CONFIG: &str = "
# Added by `cargo xtask dist`: the release archive builds from the crates
# vendored beside it, with no network.
[source.crates-io]
replace-with = \"vendored-sources\"

[source.vendored-sources]
directory = \"vendor\"
";

/// The archive's checksum in each package manager's own notation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Checksum {
    /// Lowercase hexadecimal, for the PKGBUILD's `sha256sums`.
    pub hex: String,
    /// Nix's subresource-integrity form, `sha256-<base64>`.
    pub sri: String,
    /// Nix base-32, for Guix's `(base32 ...)`.
    pub nix32: String,
}

impl Checksum {
    /// The three notations of a SHA-256 digest.
    #[must_use]
    pub fn of(digest: &[u8; 32]) -> Self {
        Self {
            hex: digest.iter().fold(String::new(), |mut hex, b| {
                let _ = write!(hex, "{b:02x}");
                hex
            }),
            sri: format!("sha256-{}", base64(digest)),
            nix32: nix32(digest),
        }
    }

    /// Parses `sha256sum`'s 64 hexadecimal digits.
    #[must_use]
    pub fn from_hex(hex: &str) -> Option<Self> {
        let hex = hex.trim();
        if hex.len() != 64 {
            return None;
        }
        let mut digest = [0u8; 32];
        for (i, byte) in digest.iter_mut().enumerate() {
            *byte = u8::from_str_radix(hex.get(2 * i..2 * i + 2)?, 16).ok()?;
        }
        Some(Self::of(&digest))
    }
}

/// Standard base64 with padding.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                let index = usize::try_from((n >> shift) & 63).unwrap_or(0);
                out.push(char::from(ALPHABET[index]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Nix's base-32: its own alphabet, the digest read from the last bit.
fn nix32(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"0123456789abcdfghijklmnpqrsvwxyz";
    let len = (bytes.len() * 8).div_ceil(5);
    (0..len)
        .rev()
        .map(|n| {
            let bit = n * 5;
            let (byte, shift) = (bit / 8, bit % 8);
            let low = u16::from(bytes[byte]) >> shift;
            let high = bytes
                .get(byte + 1)
                .map_or(0, |&next| u16::from(next) << (8 - shift));
            char::from(ALPHABET[usize::from((low | high) & 31)])
        })
        .collect()
}

/// Writes `version` and `checksum` into a package definition's text: the
/// version and checksum lines its format uses, each of which must appear
/// exactly once.
///
/// # Errors
///
/// A definition that lacks one of its lines, or has it twice.
pub fn stamp(
    name: &str,
    text: &str,
    version: &str,
    checksum: &Checksum,
) -> Result<String, Failure> {
    let rules: &[(&str, String)] = match Path::new(name).file_name().and_then(|f| f.to_str()) {
        Some("default.nix") => &[
            (r#"(?m)^(  version = ")[^"]*(";)$"#, version.to_owned()),
            (
                r#"(?m)^(    hash = ")sha256-[^"]*(";)$"#,
                checksum.sri.clone(),
            ),
        ],
        Some("guix.scm") => &[
            (r#"(?m)^(  \(version ")[^"]*("\))$"#, version.to_owned()),
            (
                r#"(?m)^(     \(sha256 \(base32 ")[^"]*("\)\)\)\))$"#,
                checksum.nix32.clone(),
            ),
        ],
        Some("PKGBUILD") => &[
            (r"(?m)^(pkgver=)\S*()$", version.to_owned()),
            (r"(?m)^(sha256sums=\(')[^']*('\))$", checksum.hex.clone()),
        ],
        _ => {
            return Err(Failure::internal(format!(
                "`{name}` is not a package definition dist knows"
            )));
        }
    };
    let mut out = text.to_owned();
    for (pattern, value) in rules {
        let re = Regex::new(pattern).map_err(|e| Failure::internal(e.to_string()))?;
        let found = re.find_iter(&out).count();
        if found != 1 {
            return Err(Failure::gate(
                format!("`{name}` has {found} line(s) matching `{pattern}`, not one"),
                "cargo xtask dist",
            ));
        }
        out = re
            .replace(&out, |caps: &regex::Captures<'_>| {
                format!("{}{value}{}", &caps[1], &caps[2])
            })
            .into_owned();
    }
    Ok(out)
}

/// Runs `program` with `args` in `dir` and returns its stdout.
pub(crate) fn run(dir: &Path, program: &str, args: &[&str]) -> Result<String, Failure> {
    let output = Command::new(program)
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|e| Failure::internal(format!("cannot run `{program}`: {e}")))?;
    if !output.status.success() {
        return Err(Failure::internal(format!(
            "`{program} {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    String::from_utf8(output.stdout)
        .map_err(|e| Failure::internal(format!("`{program}` wrote non-UTF-8: {e}")))
}

/// The workspace version, from `[workspace.package]`.
///
/// # Errors
///
/// A root manifest without one.
pub fn version(root: &Path) -> Result<String, Failure> {
    let manifest: toml::Value = toml::from_str(&paths::read_text(
        &root.join("Cargo.toml"),
        "cargo xtask dist",
    )?)?;
    manifest
        .get("workspace")
        .and_then(|w| w.get("package"))
        .and_then(|p| p.get("version"))
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| Failure::internal("Cargo.toml has no [workspace.package] version"))
}

/// The commit time, in seconds since the epoch, of the last commit up to
/// `commit` that changed what the archive holds: the commit that only stamps
/// the definitions moves nothing. Every timestamp in the archive and the SBOM
/// is this one.
///
/// # Errors
///
/// A failed `git log`.
pub fn content_epoch(root: &Path, commit: &str) -> Result<String, Failure> {
    let excluded: Vec<String> = DEFINITIONS
        .iter()
        .chain(&[RELEASES_DIR])
        .map(|path| format!(":(exclude){path}"))
        .collect();
    let mut log = vec!["log", "-1", "--format=%ct", commit, "--", "."];
    log.extend(excluded.iter().map(String::as_str));
    Ok(run(root, "git", &log)?.trim().to_owned())
}

/// The archive built: where, how big, its checksum.
#[derive(Debug, Clone, Serialize)]
pub struct Archive {
    /// The archive file.
    pub path: PathBuf,
    /// The version it releases.
    pub version: String,
    /// The commit it was built from.
    pub commit: String,
    /// Its size in bytes.
    pub bytes: u64,
    /// Its checksum.
    pub sha256: Checksum,
}

/// Builds the vendored archive of `reference` into `out`.
///
/// # Errors
///
/// A failed `git`, `cargo vendor`, `tar`, `gzip` or `sha256sum`.
pub fn build(root: &Path, reference: &str, out: &Path) -> Result<Archive, Failure> {
    let version = version(root)?;
    let name = format!("fairing-{version}");
    let commit = run(
        root,
        "git",
        &["rev-parse", "--verify", &format!("{reference}^{{commit}}")],
    )?
    .trim()
    .to_owned();
    let epoch = content_epoch(root, &commit)?;
    let stage = out.join("stage");
    if stage.exists() {
        std::fs::remove_dir_all(&stage)?;
    }
    std::fs::create_dir_all(&stage)?;
    let tarball = stage.join("tree.tar");
    run(
        root,
        "git",
        &[
            "archive",
            "--format=tar",
            &format!("--prefix={name}/"),
            "--output",
            &tarball.to_string_lossy(),
            &commit,
        ],
    )?;
    run(&stage, "tar", &["-xf", "tree.tar"])?;
    std::fs::remove_file(&tarball)?;
    let tree = stage.join(&name);
    run(
        &tree,
        "cargo",
        &["vendor", "--locked", "--versioned-dirs", "vendor"],
    )?;
    let config = tree.join(".cargo/config.toml");
    let mut text = std::fs::read_to_string(&config).unwrap_or_default();
    text.push_str(VENDOR_CONFIG);
    std::fs::write(&config, text)?;
    let file = out.join(format!("{name}.tar.gz"));
    let script = format!(
        "tar --sort=name --format=gnu --mtime=@{epoch} --owner=0 --group=0 --numeric-owner \
         --mode='u+rwX,go+rX,go-w' -cf - '{name}' | gzip -n -9 > '{}'",
        file.display()
    );
    run(&stage, "sh", &["-c", &script])?;
    std::fs::remove_dir_all(&stage)?;
    let sum = run(out, "sha256sum", &[&file.to_string_lossy()])?;
    let sha256 = sum
        .split_whitespace()
        .next()
        .and_then(Checksum::from_hex)
        .ok_or_else(|| Failure::internal(format!("`sha256sum` printed `{sum}`")))?;
    let bytes = std::fs::metadata(&file)?.len();
    Ok(Archive {
        path: file,
        version,
        commit,
        bytes,
        sha256,
    })
}

#[derive(Debug, Serialize)]
struct Output {
    archive: Archive,
    check: bool,
    definitions: Vec<String>,
}

/// `cargo xtask dist`.
///
/// # Errors
///
/// A failed build, or under `--check` a definition that does not carry the
/// archive's version and checksum.
pub fn run_dist(args: &Args) -> Result<(), Failure> {
    args.ensure_known(&["ref", "out", "check"])?;
    let root = args.root();
    let out = paths::resolve(&root, args.value("out").unwrap_or(DEFAULT_OUT));
    std::fs::create_dir_all(&out)?;
    let archive = build(&root, args.value("ref").unwrap_or("HEAD"), &out)?;
    let check = args.flag("check");
    let mut stale = Vec::new();
    for name in DEFINITIONS {
        let path = root.join(name);
        let text = paths::read_text(&path, "cargo xtask dist")?;
        let stamped = stamp(name, &text, &archive.version, &archive.sha256)?;
        if stamped != text {
            if check {
                stale.push(name.to_owned());
            } else {
                std::fs::write(&path, stamped)?;
            }
        }
    }
    let output = Output {
        archive,
        check,
        definitions: DEFINITIONS.iter().map(|&d| d.to_owned()).collect(),
    };
    if args.json() {
        Envelope::new(&args.command_line(), &output).print()?;
    } else {
        println!(
            "[OK] {} ({} bytes) from {}",
            paths::relative(&root, &output.archive.path),
            output.archive.bytes,
            output.archive.commit
        );
        println!("[OK] sha256 {}", output.archive.sha256.hex);
        if !check {
            println!("[OK] {} stamped", DEFINITIONS.join(", "));
        }
    }
    if stale.is_empty() {
        Ok(())
    } else {
        Err(Failure::gate(
            format!(
                "package definition(s) do not carry this archive's checksum: {}",
                stale.join(", ")
            ),
            "cargo xtask dist",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SHA-256 of the empty string.
    const EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    #[test]
    fn a_digest_is_written_in_every_notation() {
        let sum = Checksum::from_hex(EMPTY).unwrap_or_else(|| panic!("not hex"));
        assert_eq!(sum.hex, EMPTY);
        assert_eq!(
            sum.sri,
            "sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU="
        );
        assert_eq!(
            sum.nix32,
            "0mdqa9w1p6cmli6976v4wi0sw9r4p5prkj7lzfd1877wk11c9c73"
        );
        assert_eq!(base64(b"ab"), "YWI=");
        assert_eq!(base64(b"a"), "YQ==");
        assert!(Checksum::from_hex("abc").is_none());
        assert!(Checksum::from_hex(&"z".repeat(64)).is_none());
    }

    #[test]
    fn every_definition_is_stamped_in_its_own_notation() {
        let sum = Checksum::from_hex(EMPTY).unwrap_or_else(|| panic!("not hex"));
        let root = crate::paths::default_root();
        for name in DEFINITIONS {
            let text = std::fs::read_to_string(root.join(name)).unwrap_or_else(|e| panic!("{e}"));
            let stamped =
                stamp(name, &text, "9.8.7", &sum).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(stamped.contains("9.8.7"), "{name}");
            let expected = match name {
                "packaging/default.nix" => &sum.sri,
                "packaging/guix.scm" => &sum.nix32,
                _ => &sum.hex,
            };
            assert!(stamped.contains(expected.as_str()), "{name}");
            // Stamping twice changes nothing more.
            assert_eq!(
                stamp(name, &stamped, "9.8.7", &sum).ok().as_deref(),
                Some(stamped.as_str())
            );
        }
        assert!(stamp("packaging/PKGBUILD", "pkgver=1\n", "2", &sum).is_err());
    }
}
