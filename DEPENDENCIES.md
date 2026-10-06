<!--
SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
SPDX-License-Identifier: GPL-3.0-or-later
-->

# Dependencies — Qualification Record (§24.1)

Fairing is Category B: every **trust-path** dependency is qualified here before it enters
`Cargo.lock`; the raised `fairing-askpass` crate (Category A) qualifies **every** direct
dependency. `cargo deny check` and `cargo audit` gate CI (§24.2); versions are pinned by
the committed lockfile and re-qualified at each G3 gate. Figures below are from the
lockfile of 2026-10-06 (83 packages in total, no duplicate versions per `cargo tree
--duplicates`). Security history is the RUSTSEC record at adoption; `cargo audit` was
clean on 2026-10-06.

## Adopted (M0)

| Crate | Version | Purpose | Provenance (repo · licence) | Maintenance | Security history | Unsafe posture | Transitive | Alternatives | Exit plan |
|---|---|---|---|---|---|---|---|---|---|
| `clap` | 4.6.7 | Argument parsing, help, `schema` derivation for `fairing` | clap-rs/clap · MIT OR Apache-2.0 | Very active; frequent minor releases | No open RUSTSEC advisories | Safe Rust (`forbid(unsafe_code)` upstream) | 26 | `lexopt`, `argh` | Replace with `lexopt`; the `CommandSpec` table already carries the schema metadata |
| `serde` | 1.0.229 | Serialisation derive for envelopes, matrix, requirement model | serde-rs/serde · MIT OR Apache-2.0 | Very active | No open advisories | Minimal, audited upstream | 6 | Hand-written `Serialize` impls | Hand-rolled JSON writers (small surface) |
| `serde_json` | 1.0.151 | JSON envelopes, schema, matrix | serde-rs/json · MIT OR Apache-2.0 | Very active | RUSTSEC-2022 recursion issues long fixed; none open | Minimal, audited upstream | 10 | `json`, `miniserde` | Hand-rolled compact writer for the envelope shapes |
| `jiff` | 0.2.37 | ISO 8601 UTC timestamps (§14.5 preferred crate) | BurntSushi/jiff · Unlicense OR MIT | Active; 0.2 line stable | No open advisories | Safe Rust by default | 18 | `chrono` | `chrono::Utc` (one function changes) |
| `toml` | 1.1.6 | Parsing `doc/requirements.toml` (xtask only) | toml-rs/toml · MIT OR Apache-2.0 | Active | No open advisories | Safe Rust | 14 | `toml_edit`, `basic-toml` | `basic-toml`; the schema is flat |
| `regex` | 1.13.1 | Marker and identifier patterns (xtask only) | rust-lang/regex · MIT OR Apache-2.0 | Active, rust-lang owned | No open advisories | Minimal, audited | 4 | `regex-lite`, hand parser | `regex-lite` or a hand parser for the four fixed patterns |

Development-only (not shipped, not on any trust path): `assert_cmd` 2.2.2, `predicates`
3.1.4, `tempfile` 3.27.0 (all MIT OR Apache-2.0; 24, 12 and 12 transitive packages).

## Planned (qualified at G2 before adoption)

Seeded from the PRD's dependency table; each row is completed with the §24.1 fields
when the crate is added, never before `cargo audit` and `cargo deny` have run on it.

| Crate | Purpose | Trust path | Unsafe posture accepted | Exit plan |
|---|---|---|---|---|
| `drm` (Smithay) | KMS mode-setting, dumb buffers, page flip | Yes (render) | ioctl wrappers, unsafe audited upstream | Own thin ioctl layer over `rustix` |
| `rustix` | `/dev/fb0` ioctls, mmap, inotify, sockets | Yes (render, agent) | unsafe at the syscall boundary only | `nix` crate |
| `zbus` | `Manager.Progress` property over the systemd socket | No (degraded path exists) | Safe Rust | Time estimate only |
| `tiny-skia` | Rasterising logo, bar, text | Yes (render) | Minimal SIMD unsafe | `raqote`, or hand-rolled fills |
| `fontdue` | Glyph rasterisation for status and prompt text | No | Safe Rust | Pre-rasterised bitmap font in the theme |
| `png` | Decoding theme assets at compile time only | No (build time) | Safe Rust | Store raw RGBA in the artefact |
| `evdev` | Keyboard input, exclusive grab | Yes (agent) | ioctl wrappers | `rustix` directly |
| `postcard` | Compiled theme encoding (parser is fuzzed) | Yes | Safe Rust | `bincode` |
| `zeroize` | Secret buffer zeroisation | Yes (agent, Category A) | Volatile writes, audited | In-house volatile-write helper |
| `nickel-lang-core` | `theme check` / `theme compile` only; never on the boot path | No (build time) | Safe Rust | Separate `fairing-theme-tool` binary |

**Not adopted:** direct `libc` use (`rustix` covers it), `gstreamer` (a large C surface for no
v1 feature), Plymouth compatibility shims.
