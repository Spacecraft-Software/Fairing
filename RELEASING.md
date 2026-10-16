<!--
SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
SPDX-License-Identifier: GPL-3.0-or-later
-->

# Releasing Fairing

How a Fairing release is cut (Steelbore Standard §5.5, §24.3, §25.3). The tag and
the publication are the maintainer's; everything else is a command. Commands are POSIX
sh; Nushell runs them unchanged.

## What a release is

| Artefact | Made by | Notes |
|---|---|---|
| `fairing-<v>.tar.gz` | `cargo xtask dist` | The tree with every crate vendored and cargo pointed at them; builds offline. Bit-reproducible |
| `fairing-<v>.cdx.json` | `cargo xtask sbom` | CycloneDX 1.5 from the committed lockfile (cargo-cyclonedx 0.5.9). Bit-reproducible |
| `fairing-<v>.manifest.json` | `cargo xtask manifest` | §25.3: identity, artefacts and checksums, provenance, verification, budgets, known issues, conformance |
| `matrix.json`, `matrix.md` | `cargo xtask trace` | The traceability matrix (§21.3) |
| `SHA256SUMS` | the release workflow | Every asset above |

The three package definitions, `packaging/default.nix`, `packaging/guix.scm` and
`packaging/PKGBUILD`, carry the archive's version and SHA-256 in their own notations.
`packaging/`, `doc/releases/` and the flake are `export-ignore`, so writing the checksum
into them changes nothing the archive holds; the package definitions are used from a
checkout of the tag. Every timestamp in the archive and the SBOM is the commit time
of the last commit that changed what the archive holds, so the commit that records the
checksums archives to the same bytes as the one before it.

## Before the release

1. **G1 and the evidence.** The requirement set is baselined and every requirement is
   `verified` or `withdrawn` with its reason (§20.3, §21.3); `cargo xtask trace` is green
   with zero orphans.
2. **Budgets.** Every reference-machine budget in `doc/budgets.toml` is measured on the
   ThinkPad T490s and appended to `doc/measurements.toml`. `cargo xtask budgets
   --release` passes: no red, exceeded or unmeasured budget.
3. **Coverage and mutation.** `cargo llvm-cov --workspace --exclude xtask --locked
   --json --summary-only --output-path target/llvm-cov.json` then `cargo xtask coverage`.
   Run the `mutants` workflow (Actions → mutants → Run workflow), download its
   `outcomes.json` files, and check each with `cargo xtask mutants --outcomes <file>`;
   every survivor is killed or triaged in `doc/mutants.toml`.
4. **Notes.** Date the `[Unreleased]` section of `CHANGELOG.md` as `[<v>] - YYYY-MM-DD`
   and open a new `[Unreleased]` above it. Complete `doc/releases/<v>.toml`: the G3
   validation note (§21.5), the known issues with their tracker identifiers and
   severities (§25.2), and the conformance claim.
5. **Merge.** These changes land on `main` like any other, through a reviewed pull
   request. `main`'s head is now the release content.

## Cutting the release

In a full clone (not shallow) of an up-to-date `main`, with nothing uncommitted, and
**outside `nix develop`**: `cargo vendor`'s output differs between cargo releases, so the
archive is built with rustup's cargo at the version `rust-toolchain.toml` pins, and
`cargo xtask dist` refuses any other. cargo-cyclonedx must be 0.5.9.

```sh
cargo xtask dist        # target/dist/fairing-<v>.tar.gz; stamps the three definitions
cargo xtask sbom        # target/dist/fairing-<v>.cdx.json
cargo xtask trace
```

### Install from each definition (§25.3)

Before anything is tagged, install the release from **each** package definition into a
clean environment, run it once, and remove it. The archive is not published yet, so each
package manager is handed it under the name and checksum the definition expects.

- **Nix**, in a fresh `nix shell` or VM:

  ```sh
  nix-store --add-fixed sha256 target/dist/fairing-<v>.tar.gz
  nix build --impure --expr '
    let pkgs = (builtins.getFlake (toString ./.)).inputs.nixpkgs.legacyPackages.x86_64-linux;
    in pkgs.callPackage ./packaging/default.nix { }'
  nix profile install ./result && fairing --version && nix profile remove fairing
  ```

- **Arch Linux**, in a throwaway `archlinux:base-devel` container. makepkg will not run
  as root, so it runs as a user, and pacman as root:

  ```sh
  pacman -Syu --noconfirm --needed cargo make texinfo
  useradd --create-home builder
  install -d -o builder /home/builder/pkg
  install -o builder packaging/PKGBUILD target/dist/fairing-<v>.tar.gz /home/builder/pkg/
  su builder -c 'cd /home/builder/pkg && makepkg --noconfirm'
  pacman -U --noconfirm /home/builder/pkg/fairing-*.pkg.tar.zst
  fairing --version
  pacman -R --noconfirm fairing
  ```

- **Guix**, on a Guix System or a foreign distribution with Guix:

  ```sh
  guix download "file://$PWD/target/dist/fairing-<v>.tar.gz"
  guix package --install-from-file=packaging/guix.scm
  fairing --version
  guix package --remove=fairing
  ```

Record the three results in the validation note of `doc/releases/<v>.toml`, then write
the manifest from the final notes (`cargo xtask manifest --check`, in the release
workflow, fails if the notes change afterwards):

```sh
# --mutants: the outcomes.json of every shard of the mutants workflow, comma-separated
cargo xtask manifest --coverage target/llvm-cov.json --mutants "$shards"
git diff --stat         # only packaging/{default.nix,guix.scm,PKGBUILD} and doc/releases/
```

### Commit, tag, push

```sh
git add packaging/default.nix packaging/guix.scm packaging/PKGBUILD doc/releases/
git commit -S -m "Release <v>"
git tag -s v<v> -m "Fairing <v>"
git push origin main v<v>
```

The release commit touches only `export-ignore` files, so it may go through a pull
request and a squash merge like any other commit; then tag the squashed commit on
`main` instead. Never put a change to archived content in the release commit: the
archive would no longer match its checksum.

## What the release workflow does

`.github/workflows/release.yml` runs on the pushed tag. It checks that the tag names the
workspace version, rebuilds the archive (`cargo xtask dist --check`) and the SBOM from
the tag and compares them with the checksums the definitions and the committed manifest
carry (`cargo xtask manifest --check`), regenerates the traceability matrix, fails on a
budget that blocks the release, and drafts a GitHub release with every asset and
`SHA256SUMS`. Review the draft, then publish it. Nothing is published to crates.io or any
other registry (§6.4).

## After the release

- Confirm the published archive's checksum matches `SHA256SUMS` and the definitions.
- `SECURITY.md`'s supported-versions table names the new release.
- An advisory for a later fix cites the affected releases by their SBOMs (§26.2).
