# SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Nix derivation for Fairing, consumed by flake.nix through `callPackage`.
# Builds from the local tree. The §5.5 release form (fetch from the tagged
# archive with its SHA-256) lands at M5 alongside guix.scm and PKGBUILD.
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
{
  lib,
  rustPlatform,
  texinfo,
  gnumake,
  withThemeTool ? true,
  withDbus ? true,
}:
let
  full = withThemeTool && withDbus;
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
  version = "0.1.0";

  # Only what the build and its tests read: never `target/` or `doc/build/`,
  # which a working copy may hold.
  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.difference (lib.fileset.unions [
      ../.cargo
      ../Cargo.toml
      ../Cargo.lock
      ../crates
      ../xtask
      ../contracts
      ../themes
      ../doc
    ]) (lib.fileset.maybeMissing ../doc/build);
  };

  cargoLock.lockFile = ../Cargo.lock;

  # Only the product binary is installed; xtask is development tooling.
  cargoBuildFlags = [
    "--package"
    "fairing"
  ];
  cargoTestFlags = [ "--workspace" ];

  buildNoDefaultFeatures = !full;
  buildFeatures = lib.optionals (!full) (
    lib.optional withThemeTool "theme-tool" ++ lib.optional withDbus "dbus"
  );
  # The workspace suite runs once, in the full build; the others differ only
  # by feature switches, which CI tests separately.
  doCheck = full;

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
