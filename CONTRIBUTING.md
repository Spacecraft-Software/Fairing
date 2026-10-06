<!--
SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
SPDX-License-Identifier: GPL-3.0-or-later
-->

# Contributing to Fairing

Fairing is a personal project of Spacecraft Software (Steelbore Standard §5). Contributions
are welcome; acceptance, scope, naming and roadmap are at the maintainer's sole discretion
(§5.4), and a rejection reflects fit, not quality. `AGENTS.md` is the machine-facing
counterpart of this document; both describe the same repository.

## Ground rules

- **Licence.** By contributing you agree your work is licensed GPL-3.0-or-later (manual
  content CC-BY-SA-4.0). Every new file carries the two-tag REUSE header
  (`SPDX-FileCopyrightText` + `SPDX-License-Identifier`); `reuse lint` must pass.
- **Signed commits (§6.3).** Commits on this repository are cryptographically signed. The
  maintainer re-signs contributions with the project key at squash-merge, so an unsigned
  or differently-signed PR is fine to open but will not land as-is.
- **Inbound only (§6.4).** This repository never initiates upstream submissions,
  registry publications or issues on external trackers; vendoring a fix in-tree is the
  default.
- **Text files (§6.5).** UTF-8, LF, final newline. `.gitattributes` and `.editorconfig`
  enforce it; `cargo xtask check-eol` is the gate.
- **Security issues** go through [SECURITY.md](SECURITY.md), never a public issue.

## Development setup

```sh
git clone https://github.com/Spacecraft-Software/Fairing
cd Fairing
nix develop                 # toolchain, cargo-deny, cargo-audit, reuse, texinfo, nushell
cargo build --workspace --locked
cargo test --workspace --locked
```

Without Nix: install the toolchain named in `rust-toolchain.toml` with rustup, then
`cargo install --locked cargo-deny cargo-audit`, `pip install reuse`, and texinfo from your
distribution.

## Pull-request checklist

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- [ ] `cargo test --workspace --locked`
- [ ] `cargo xtask check-eol`, `cargo xtask req-texi --check`, `cargo xtask trace`
- [ ] `make check` (zero makeinfo warnings) if `doc/` changed
- [ ] `cargo deny check` and `cargo audit` if `Cargo.toml`/`Cargo.lock` changed, plus a
      `DEPENDENCIES.md` row for any new crate (§24.1)
- [ ] `reuse lint`
- [ ] `CHANGELOG.md` has an entry under `[Unreleased]`
- [ ] `PLAN.md` / `TODO.md` items ticked in the same commit when work completes
- [ ] New tests cite the requirement they verify (`// Verifies: FRN-SRS-NNN`)

## Commit messages

Imperative subject (≤ 72 characters), blank line, body explaining the *why*. Name the plan
item (`P-012`, `T-004`) or requirement (`FRN-SRS-045`) the change serves. Branches off
`main`; squash merge.

## Requirements changes

Edit `doc/requirements.toml`, run `cargo xtask req-texi`, and commit the TOML together
with the regenerated `doc/needs.texi` and `doc/requirements.texi`. Never reuse an
identifier; withdraw instead. Changing a requirement that is already `baselined` re-enters
the G1 gate and is the maintainer's decision.

## Code of conduct

Be kind, assume good faith, prefer rigour to speed and correctness to cleverness.
