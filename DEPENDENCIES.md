<!--
SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
SPDX-License-Identifier: GPL-3.0-or-later
-->

# Dependencies — Qualification Record (§24.1)

Fairing is Category B: every **trust-path** dependency is qualified here before it enters
`Cargo.lock`; the raised `fairing-askpass` crate (Category A) qualifies **every** direct
dependency. `cargo deny check` and `cargo audit` gate CI (§24.2); versions are pinned by
the committed lockfile and re-qualified at each G3 gate. Figures below are from the
lockfile of 2026-10-06 (102 packages; one duplicate, `linux-raw-sys` 0.9.4 via `drm-sys`
and 0.12.1 via `rustix`, reported by `cargo deny` as a warning). Security history is the
RUSTSEC record at adoption; `cargo audit` on 2026-10-06 reports one informational
advisory, recorded under *Accepted advisories*.

Every product crate is `#![forbid(unsafe_code)]`; the "Unsafe posture" column records
where the unsafe that Fairing relies on actually lives.

## Adopted (M0)

| Crate | Version | Purpose | Provenance (repo · licence) | Maintenance | Security history | Unsafe posture | Transitive | Alternatives | Exit plan |
|---|---|---|---|---|---|---|---|---|---|
| `clap` | 4.6.7 | Argument parsing, help, `schema` derivation for `fairing` | clap-rs/clap · MIT OR Apache-2.0 | Very active; frequent minor releases | No open RUSTSEC advisories | Safe Rust (`forbid(unsafe_code)` upstream) | 26 | `lexopt`, `argh` | Replace with `lexopt`; the `CommandSpec` table already carries the schema metadata |
| `serde` | 1.0.229 | Serialisation derive for envelopes, matrix, requirement model | serde-rs/serde · MIT OR Apache-2.0 | Very active | No open advisories | Minimal, audited upstream | 6 | Hand-written `Serialize` impls | Hand-rolled JSON writers (small surface) |
| `serde_json` | 1.0.151 | JSON envelopes, schema, matrix | serde-rs/json · MIT OR Apache-2.0 | Very active | RUSTSEC-2022 recursion issues long fixed; none open | Minimal, audited upstream | 10 | `json`, `miniserde` | Hand-rolled compact writer for the envelope shapes |
| `jiff` | 0.2.37 | ISO 8601 UTC timestamps (§14.5 preferred crate) | BurntSushi/jiff · Unlicense OR MIT | Active; 0.2 line stable | No open advisories | Safe Rust by default | 18 | `chrono` | `chrono::Utc` (one function changes) |
| `toml` | 1.1.6 | Parsing `doc/requirements.toml` (xtask) and the vendored palette file at build time (`fairing-theme` build script and tests; never in the binary) | toml-rs/toml · MIT OR Apache-2.0 | Active | No open advisories | Safe Rust | 14 | `toml_edit`, `basic-toml` | `basic-toml`; both schemas are flat |
| `regex` | 1.13.1 | Marker and identifier patterns (xtask only) | rust-lang/regex · MIT OR Apache-2.0 | Active, rust-lang owned | No open advisories | Minimal, audited | 4 | `regex-lite`, hand parser | `regex-lite` or a hand parser for the four fixed patterns |

## Adopted (M1 — rendering)

Trust path: everything below is linked into the `fairing` binary and runs in the initrd.

| Crate | Version | Purpose | Provenance (repo · licence) | Maintenance | Security history | Unsafe posture | Transitive | Alternatives | Exit plan |
|---|---|---|---|---|---|---|---|---|---|
| `drm` | 0.15.0 | KMS enumeration, dumb buffers, legacy mode-set, page flip, event read (`fairing-render`) | Smithay/drm-rs · MIT | Active (Smithay org, Wayland compositors depend on it) | No RUSTSEC advisories | ~50 `unsafe` sites: ioctl plumbing, `mmap`/`munmap` of dumb buffers, unaligned event reads, handle `Vec` transmutes; `DumbMapping::drop` calls `munmap(..).expect(..)`, unreachable for a valid mapping but noted under `panic = "abort"` | 11 | Hand-written ioctl layer over `rustix::ioctl` | Own thin ioctl module over `rustix` (the backend uses eight ioctls) |
| `drm-ffi` | 0.9.1 | Typed ioctl wrappers used by `drm` | Smithay/drm-rs · MIT | Active, same release train as `drm` | None | `unsafe` ioctl calls through `rustix::ioctl`; no C build | 4 | — | Falls with `drm` |
| `drm-sys` | 0.8.1 | Prebuilt kernel UAPI bindings (`build.rs` is a no-op without `use_bindgen`) | Smithay/drm-rs · MIT | Active | None | Generated `repr(C)` structs; no runtime unsafe | 1 | — | Falls with `drm` |
| `drm-fourcc` | 2.2.0 | `DrmFourcc::Xrgb8888` and friends | danielzfranklin/drm-fourcc-rs · MIT | Low activity, stable constants | None | Safe Rust | 0 | Literal `u32` | Inline the two fourcc constants |
| `rustix` | 1.1.5 | `poll()` on the DRM descriptor (`event`) and errno constants; also pulled by `drm` (`mm`, `fs`) | bytecodealliance/rustix · Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | Very active, Bytecode Alliance | No open advisories (history of promptly fixed issues) | Unsafe at the syscall boundary only, extensively audited; raw-syscall backend on Linux (no libc) | 2 | `libc`, `nix` | `nix` (same two calls) |
| `linux-raw-sys` | 0.9.4 and 0.12.1 | Kernel ABI constants for `drm-sys` and `rustix` | sunfishcode/linux-raw-sys · Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | Active | None | Generated constants; no runtime code | 0 | — | Falls with its parents; the duplicate disappears when `drm-sys` moves to 0.12 |
| `bytemuck` + `bytemuck_derive` | 1.25.2 / 1.12.1 | Plain-old-data casts inside `drm` (`Mode`) and `tiny-skia` (pixels) | Lokathor/bytemuck · Zlib OR Apache-2.0 OR MIT | Active | None open | `unsafe impl Pod/Zeroable` behind checked derives; well-reviewed | 4 (proc-macro) | — | Falls with its parents |
| `bitflags` | 2.13.2 | Flag sets in `drm` and `rustix` | bitflags/bitflags · MIT OR Apache-2.0 | Active | None | Safe Rust | 0 | — | Falls with its parents |
| `tiny-skia` | 0.12.0 | CPU rasterisation of canvas, logo, bar (`default-features = false`, `std` + `simd`; no `png`) | linebender/tiny-skia · BSD-3-Clause | Active (Linebender) | No RUSTSEC advisories | SIMD intrinsics gated on target features, `unsafe impl Pod`, three `new_unchecked` constants; no raw-pointer indexing in the pipeline | 7 | `raqote`, hand-rolled fills | Hand-rolled span fills and a cubic flattener (the splash draws rectangles, a capsule and one closed path) |
| `tiny-skia-path` | 0.12.0 | Geometry for `tiny-skia` | linebender/tiny-skia · BSD-3-Clause | Active | None | Three `unsafe { new_unchecked }` constants | 3 | — | Falls with `tiny-skia` |
| `strict-num`, `arrayref`, `arrayvec`, `cfg-if`, `log` | 0.1.1, 0.3.9, 0.7.8, 1.0.5, 0.4.34 | Small utilities under `tiny-skia` (`log` carries its misuse warnings; Fairing installs no logger, so they are silent) | RazrFalcon/strict-num · MIT; droundy/arrayref · BSD-2-Clause; bluss/arrayvec · MIT OR Apache-2.0; rust-lang/cfg-if · MIT OR Apache-2.0; rust-lang/log · MIT OR Apache-2.0 | Stable, widely used | None open | `arrayvec` and `log` carry small, long-audited unsafe; the rest are safe | 0 | — | Fall with `tiny-skia` |
| `fontdue` | 0.9.4 | Glyph rasterisation for the percentage and status text (`default-features = false`, `simd` + `hashbrown`) | mooman219/fontdue · MIT OR Apache-2.0 OR Zlib | Low activity, stable API | None open | 32 `unsafe` sites: x86 SIMD, unchecked raster accumulation, `Vec::from_raw_parts` in `get_bitmap`, unicode tables. Fairing only rasterises its bundled font; glyph output is bounds-checked by `fairing-render` before blitting | 6 | `ab_glyph`, pre-rasterised bitmap font | Pre-rasterised bitmap font compiled into the theme artefact (M2) |
| `ttf-parser` | 0.25.1 | Font table parsing for `fontdue` | harfbuzz/ttf-parser · MIT OR Apache-2.0 | **Unmaintained** per RUSTSEC-2026-0192 (see below) | RUSTSEC-2026-0192 (informational) | `#![forbid(unsafe_code)]` | 1 | `skrifa`, `read-fonts` | Moves with `fontdue`'s exit plan; a bitmap font needs no parser |
| `hashbrown` | 0.15.5 | `fontdue`'s glyph map (`no_std`-capable `HashMap`) | rust-lang/hashbrown · MIT OR Apache-2.0 | Active (std's own table) | None open | The std `HashMap` implementation; extensive unsafe, extensively tested | 3 | `fontdue` feature `std` | Enable `fontdue/std` (uses std's `HashMap`, same code) |
| `foldhash`, `allocator-api2`, `equivalent`, `core_maths`, `libm` | 0.1.5, 0.2.21, 1.0.2, 0.1.1, 0.2.16 | Hashing and float helpers under `hashbrown`/`fontdue`/`ttf-parser` | orlp/foldhash · Zlib; zakarumych/allocator-api2 · MIT OR Apache-2.0; indexmap-rs/equivalent · MIT OR Apache-2.0; robertbastian/core_maths · MIT; rust-lang/libm · MIT | Stable | None open | `foldhash` and `allocator-api2` carry small unsafe; `libm` is pure Rust math | 0 | — | Fall with `fontdue` |

Development-only (not shipped, not on any trust path): `assert_cmd` 2.2.2, `predicates`
3.1.4, `tempfile` 3.27.0 (all MIT OR Apache-2.0).

### Accepted advisories

| Advisory | Crate | Nature | Decision | Review |
|---|---|---|---|---|
| RUSTSEC-2026-0192 | `ttf-parser` 0.25.1 | Informational: unmaintained | Accepted for M1. The parser only ever reads the one font Fairing bundles, which is pinned by size and glyph count in `fairing-render`'s tests, so no untrusted input reaches it. The exit plan (bitmap font in the theme artefact) removes both `fontdue` and `ttf-parser`. | G2 |

## Bundled assets

| Asset | Version · source | Licence | Where | Why |
|---|---|---|---|---|
| Inconsolata Regular (static TTF, 964 glyphs, 105 284 bytes, sha256 `ab56ea18c5c24d2b909261f0c63a14f9576dfabaf2e9ebd353062aa4149cefc7`) | 3.100 · googlefonts/Inconsolata `fonts/ttf/Inconsolata-Regular.ttf` | OFL-1.1, no Reserved Font Name; upstream notice shipped verbatim as `OFL.txt` beside the font, licence text in `LICENSES/OFL-1.1.txt` | `crates/fairing-render/assets/fonts/` | Steelbore §12 body/code face; the initrd has no fontconfig, so the face is compiled in (§9.1 fidelity choice) |
| Steelbore palette file (`steelbore.toml` 3.5.0, §11 v2.08, sha256 `64a03c3b67589e5b420e108278bb475b202b8053afff42348081f1e4e6634138`) | `steelbore-color-palette` skill asset, byte-identical | GPL-3.0-or-later (carries its own header) | `crates/fairing-theme/assets/` | The only source of colour values (§11.6): `fairing-theme`'s build script generates the role tables from it and fails the build on any retyped hex |

## Planned (qualified at G2 before adoption)

Seeded from the PRD's dependency table; each row is completed with the §24.1 fields
when the crate is added, never before `cargo audit` and `cargo deny` have run on it.

| Crate | Purpose | Trust path | Unsafe posture accepted | Exit plan |
|---|---|---|---|---|
| `zbus` | `Manager.Progress` property over the systemd socket | No (degraded path exists) | Safe Rust | Time estimate only |
| `png` | Decoding theme assets at compile time only | No (build time) | Safe Rust | Store raw RGBA in the artefact |
| `evdev` | Keyboard input, exclusive grab | Yes (agent) | ioctl wrappers | `rustix` directly |
| `postcard` | Compiled theme encoding (parser is fuzzed) | Yes | Safe Rust | `bincode` |
| `zeroize` | Secret buffer zeroisation | Yes (agent, Category A) | Volatile writes, audited | In-house volatile-write helper |
| `nickel-lang-core` | `theme check` / `theme compile` only; never on the boot path | No (build time) | Safe Rust | Separate `fairing-theme-tool` binary |

**Not adopted:** direct `libc` use (`rustix` covers it), `gstreamer` (a large C surface for no
v1 feature), Plymouth compatibility shims, `crossbeam-channel` (std's `mpsc` is the same
algorithm and adds no unsafe to qualify; revisit only if `select!` over several receivers
is ever needed).
