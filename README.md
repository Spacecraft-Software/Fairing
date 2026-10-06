<!--
SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
SPDX-License-Identifier: GPL-3.0-or-later
-->

# Fairing

a Rust boot splash for Steelbore OS Bravais that shows real boot progress, prompts for disk passphrases in-theme, and holds the screen until greetd takes over.

**Status:** pre-release. Milestone M0 (repository and posture) is in place and milestone M1 (draws a frame) is implemented: `fairing preview` renders the splash with a simulated bar through DRM/KMS, `/dev/fb0` or an off-screen buffer. Boot integration (M2), the password agent (M3) and accessibility (M4) are not started.

Conforms to The Steelbore Standard v2.12 — Category B (password agent and initrd unit raised to A), tailored (§6.1, §10, §13, §18.3, §20.1; see [COMPLIANCE.md](COMPLIANCE.md)).

## What Fairing will do

- Draw from the first frame simpledrm can give in the initrd, through switch-root, until greetd claims the virtual terminal, on DRM/KMS with an fbdev fallback and a text backend for accessible mode.
- Show a bar driven by real systemd work: a cached time estimate in the initrd, the `Manager.Progress` property after switch-root, never moving backwards.
- Act as the systemd password agent, drawing the LUKS (and any other) prompt inside the splash, with every secret buffer zeroised.
- Fail open: no boot target requires it, every wait has a timeout, and a crash leaves the console and `systemd-tty-ask-password-agent` to finish the boot.
- Take its theme from a Nickel file evaluated at build time by the NixOS module into a compact artefact; the Steelbore palette and its high-contrast and mono siblings come from the house palette file, never retyped.

The full requirement set lives in the manual: `doc/fairing.texi` (Needs and Requirements chapters), generated from `doc/requirements.toml`.

## Repository layout

| Path | What it is |
|---|---|
| `crates/fairing` | The binary: the CLI (`preview`, `describe`, `schema`); the lifecycle state machine and systemd notify arrive at M2 (Category B) |
| `crates/fairing-render` | Frame compositor, DRM/KMS and fbdev backends, memory backend for tests; the text backend arrives at M4 (B) |
| `crates/fairing-theme` | Steelbore palette tokens and §11.6 variant resolution; the compiled theme format, Nickel contract and `theme check` / `theme compile` arrive at M2 (B) |
| `crates/fairing-askpass` | systemd ask-password agent (**Category A**) |
| `xtask/` | In-tree task runner: requirements generation, traceability, progress, text-format gate |
| `doc/` | Texinfo manual, requirement set, Makefile |
| `packaging/` | Nix derivation used by `flake.nix` (Guix and PKGBUILD arrive at M5) |

## Building and verifying

```sh
cargo build --workspace --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo fmt --all --check
cargo xtask check-eol          # Steelbore §6.5 text-file gate (stored LF, no BOM, final newline)
cargo xtask req-texi --check   # generated Texinfo chapters are current
cargo xtask trace              # traceability matrix; fails on unknown or orphaned requirements
cargo xtask progress           # §17.1 progress block from PLAN.md and TODO.md
make check info html           # manual: zero makeinfo warnings, then .info and .html under doc/build/
cargo deny check && cargo audit
reuse lint
```

`nix develop` provides every tool above. The `fairing` binary currently offers `fairing preview`, `fairing describe`, `fairing schema [<command>]`, `--version` and the global flags of the Spacecraft Software CLI Standard; run `fairing describe --json` for the live capability manifest.

### Previewing the splash

```sh
fairing preview --seconds 3                                   # on a text console: DRM/KMS, then /dev/fb0
fairing preview --backend memory --snapshot frame.ppm --json  # off-screen, for agents and CI
fairing preview --palette steelbore-high-contrast --status "Mounting /home"
```

The layout is authored at 1920×1080 and scaled uniformly into any mode, letterboxed in the
canvas colour. Colours are the §11.1 role tokens of a registered Steelbore theme, chosen by
`--palette`, then `SPACECRAFT_THEME`, then the family default; `NO_COLOR` selects the mono
theme. The theme is resolved once at start-up and held (§11.6.2): a splash cannot switch
palettes atomically mid-boot, and a text console has no light/dark preference to follow.
Under `AI_AGENT`, `CI` or `CLAUDECODE` no virtual terminal is opened: the automatic chain
renders to memory and says so. Hardware tests are `#[ignore]`d and run only on request:
`cargo test --workspace -- --ignored` on a free text console; they fail without a device,
as they should.

## Requirements and traceability

Requirements are authored in `doc/requirements.toml` and rendered into the manual by `cargo xtask req-texi`. Source code cites them with `// Verifies: FRN-SRS-012` (evidence) and `// Implements: FRN-SRS-012` (design element); `cargo xtask trace` joins the two and fails CI on an identifier that does not exist or a baselined requirement with no evidence. Progress figures (`cargo xtask progress`) are read straight off `PLAN.md` and `TODO.md`.

## Project Posture

Fairing is a personal hobby project of Spacecraft Software (Steelbore Standard §5.1): maintained for the maintainer's own use case, at hobby pace, with no service-level commitment, **no warranty and no liability** ([NOTICE.md](NOTICE.md)). Contributions are welcome but not guaranteed to be accepted ([CONTRIBUTING.md](CONTRIBUTING.md)); forking is encouraged. Security reports go through the private channel in [SECURITY.md](SECURITY.md).

- **Assurance category (§19):** B — Significant. Two subsystems are raised to A: the password agent (`fairing-askpass`) and the initrd lifecycle unit (`fairing-initrd.service`).
- **Component system (§13):** not applicable. Fairing draws to a raw DRM/fbdev surface and ships no widget toolkit; the §11 Steelbore palette still applies through the theme.
- **Accessibility (§18):** the text backend is the accessible mode (arrives in M4). The project is pre-release and has declared itself usable to nobody, so no §18.4 remediation entry is owed yet.
- **Privacy (§9):** no network access at all, at build or at run time; no telemetry.
- **Support window (§26.4):** best effort on the latest release only; no backports.

## Maintainer

Mohamed Hammad · <Mohamed.Hammad@SpacecraftSoftware.org> · <https://Fairing.SpacecraftSoftware.org/>

## License

Software is GPL-3.0-or-later — see [LICENSE](LICENSE). The manual (`doc/*.texi`, `doc/requirements.toml`) is CC-BY-SA-4.0 — see `LICENSES/CC-BY-SA-4.0.txt`. The repository is REUSE-compliant.

---

*— Built by [Spacecraft Software](https://SpacecraftSoftware.org/) —*
