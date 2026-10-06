# SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Fairing — boot splash for Steelbore OS Bravais.
# `nix develop` gives the full verification toolchain; `nix build` builds the
# binary and the Info manual. The NixOS module (steelbore.fairing) arrives at M2.
{
  description = "Fairing — boot splash for Steelbore OS Bravais";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
      pkgsFor = system: nixpkgs.legacyPackages.${system};
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
          fairing = pkgs.callPackage ./packaging/default.nix { };
        in
        {
          inherit fairing;
          default = fairing;
        }
      );

      devShells = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
        in
        {
          default = pkgs.mkShell {
            name = "fairing";
            packages = [
              pkgs.cargo
              pkgs.rustc
              pkgs.clippy
              pkgs.rustfmt
              pkgs.rust-analyzer
              pkgs.cargo-deny
              pkgs.cargo-audit
              pkgs.reuse
              pkgs.texinfo
              pkgs.nushell
              pkgs.nickel
              pkgs.nixfmt-rfc-style
              pkgs.gnumake
            ];
            # The repository pins the toolchain in rust-toolchain.toml for rustup
            # users; inside this shell nixpkgs' rustc is used, which must satisfy
            # the workspace rust-version. `cargo --version` tells you which you have.
            shellHook = ''
              echo "fairing dev shell — rustc $(rustc --version | cut -d' ' -f2)"
            '';
          };
        }
      );

      checks = forAllSystems (system: {
        package = self.packages.${system}.default;
      });

      formatter = forAllSystems (system: (pkgsFor system).nixfmt-rfc-style);
    };
}
