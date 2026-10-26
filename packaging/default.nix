# SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Nix derivation for Fairing, consumed by flake.nix through `callPackage`.
# Steelbore Standard §5.5 release form: without `workingTree` it fetches the
# release archive `cargo xtask dist` builds, by version and SHA-256, and
# builds from the crates vendored inside it. flake.nix passes the working tree
# as `workingTree` instead, with the dependencies read from Cargo.lock. `cargo xtask dist`
# writes `version` and `hash` below; never edit them by hand.
#
# Two feature switches make the splash builds the NixOS module runs:
#
#   withThemeTool = false   no Nickel evaluator: `fairing theme check|compile`
#                           report FEATURE_UNAVAILABLE (the boot loads
#                           compiled themes only)
#   withDbus = false        no systemd D-Bus client: the initrd has no bus
#
# `fairing-splash` (no theme tool) is the stage-2 splash; `fairing-initrd`
# (neither) is the copy the initrd carries, the one the 2.5 MiB budget binds.
# That one alone is optimised for size (`opt-level = "s"`, the §3.2 flag
# register in Cargo.toml): it draws a few frames a second, far inside its
# first-frame budget, and every byte of it is in the initrd.
{
  lib,
  rustPlatform,
  fetchurl,
  texinfo,
  gnumake,
  withThemeTool ? true,
  withDbus ? true,
  workingTree ? null,
}:
let
  full = withThemeTool && withDbus;
  initrd = !withThemeTool && !withDbus;
  version = "0.1.0";
  # The release archive: the tree at the tag with every crate vendored.
  release = fetchurl {
    url = "https://github.com/Spacecraft-Software/Fairing/releases/download/v${version}/fairing-${version}.tar.gz";
    hash = "sha256-B1Rl+nq1zppxEruCKHUAu3/cEAVBFFhaJY/9jV09RKc=";
  };
in
rustPlatform.buildRustPackage (finalAttrs: {
  pname =
    if full then
      "fairing"
    else if withDbus then
      "fairing-splash"
    else if withThemeTool then
      "fairing-tool"
    else
      "fairing-initrd";
  inherit version;

  src = if workingTree == null then release else workingTree;

  # The archive carries its crates in `vendor/`; the working tree has only
  # Cargo.lock, whose crates Nix fetches by their checksums.
  ${if workingTree == null then "cargoVendorDir" else "cargoLock"} =
    if workingTree == null then "vendor" else { lockFile = ../Cargo.lock; };

  # Only the product binary is installed; xtask is development tooling.
  cargoBuildFlags = [
    "--package"
    "fairing"
  ];
  # xtask's tests read the repository itself (git, the package definitions,
  # doc/releases), which the release archive leaves out; CI runs them.
  cargoTestFlags = [
    "--workspace"
    "--exclude"
    "xtask"
  ];

  buildNoDefaultFeatures = !full;
  buildFeatures = lib.optionals (!full) (
    lib.optional withThemeTool "theme-tool" ++ lib.optional withDbus "dbus"
  );
  # The workspace suite runs once, in the full build; the others differ only
  # by feature switches, which CI tests separately.
  doCheck = full;

  # The initrd copy is built for size (maintainer decision, 2026-10-26).
  env = lib.optionalAttrs initrd { CARGO_PROFILE_RELEASE_OPT_LEVEL = "s"; };

  nativeBuildInputs = [
    texinfo
    gnumake
  ];

  postBuild = ''
    make -C doc info
  '';

  postInstall = ''
    install -Dm644 doc/build/fairing.info "$out/share/info/fairing.info"
  '';

  meta = {
    description = "Boot splash for Steelbore OS Bravais: real systemd progress, in-splash LUKS prompt, greetd handoff";
    homepage = "https://Fairing.SpacecraftSoftware.org/";
    # The theme tool links LGPL-3.0-only malachite (under nickel-lang-core), so
    # a build with it is conveyed under GPL-3.0; the splash builds are
    # GPL-3.0-or-later throughout (DEPENDENCIES.md).
    license =
      if withThemeTool then
        [
          lib.licenses.gpl3Only
          lib.licenses.lgpl3Only
        ]
      else
        lib.licenses.gpl3Plus;
    platforms = lib.platforms.linux;
    mainProgram = "fairing";
  };
})
