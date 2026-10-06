# SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Compiles a Nickel theme into a Fairing artefact at build time (FRN-SRS-042,
# FRN-SRS-090). The theme's directory is copied into the store whole, because
# a theme may import files beside it (frame sequences, a PNG logo); keep a
# theme in a directory of its own.
#
# The result is a directory holding `theme.fairing`; the module copies it into
# the initrd and points both splash instances at it.
{
  lib,
  runCommand,
  # The full build: compiling needs the Nickel evaluator (`theme-tool`).
  fairing,
  # Path to the theme's `.ncl` file.
  theme,
}:
let
  source = builtins.path {
    path = dirOf theme;
    name = "fairing-theme-source";
  };
  file = baseNameOf theme;
in
runCommand "fairing-theme" { } ''
  mkdir -p "$out"
  ${lib.getExe fairing} --quiet theme compile ${lib.escapeShellArg "${source}/${file}"} \
    --output "$out/theme.fairing"
''
