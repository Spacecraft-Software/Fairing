# SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Nix derivation for Fairing, consumed by flake.nix through `callPackage`.
# Builds from the local tree. The §5.5 release form (fetch from the tagged
# archive with its SHA-256) lands at M5 alongside guix.scm and PKGBUILD.
{
  lib,
  rustPlatform,
  texinfo,
  gnumake,
}:
rustPlatform.buildRustPackage (finalAttrs: {
  pname = "fairing";
  version = "0.1.0";

  src = lib.cleanSource ../.;

  cargoLock.lockFile = ../Cargo.lock;

  # Only the product binary is installed; xtask is development tooling.
  cargoBuildFlags = [
    "--package"
    "fairing"
  ];
  cargoTestFlags = [ "--workspace" ];

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
    license = lib.licenses.gpl3Plus;
    platforms = lib.platforms.linux;
    mainProgram = "fairing";
  };
})
