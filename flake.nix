# SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Fairing — boot splash for Steelbore OS Bravais.
# `nix develop` gives the full verification toolchain; `nix build` builds the
# binary and the Info manual; `nixosModules.fairing` is the steelbore.fairing
# module; `nix flake check` evaluates the module and, on x86_64-linux with
# KVM, boots it in NixOS VM tests.
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
          # The stage-2 splash (no Nickel) and the initrd copy (no Nickel, no
          # D-Bus) the NixOS module runs.
          fairing-splash = fairing.override { withThemeTool = false; };
          fairing-initrd = fairing.override {
            withThemeTool = false;
            withDbus = false;
          };
          # The reference theme, compiled.
          theme = pkgs.callPackage ./packaging/nixos/theme.nix {
            inherit fairing;
            theme = ./themes/steelbore.ncl;
          };
        }
      );

      nixosModules = {
        fairing = ./packaging/nixos/module.nix;
        default = self.nixosModules.fairing;
      };

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
              pkgs.nixfmt
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

      checks = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
          module = self.nixosModules.fairing;
        in
        {
          package = self.packages.${system}.default;
          theme = self.packages.${system}.theme;
          initrd-binary = self.packages.${system}.fairing-initrd;
          module-eval = import ./packaging/nixos/tests/module-eval.nix { inherit pkgs; };
        }
        # The VM tests boot x86_64 guests under KVM.
        // nixpkgs.lib.optionalAttrs (system == "x86_64-linux") {
          vm-boot = import ./packaging/nixos/tests/boot.nix { inherit pkgs module; };
          vm-failed-unit = import ./packaging/nixos/tests/failed-unit.nix { inherit pkgs module; };
        }
      );

      formatter = forAllSystems (system: (pkgsFor system).nixfmt);
    };
}
