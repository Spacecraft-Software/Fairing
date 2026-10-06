# SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Evaluates the steelbore.fairing module and checks the units it generates,
# without building or booting anything. A failed check fails evaluation and
# names itself. `nix flake check` runs it as `checks.<system>.module-eval`.
#
# Verifies: FRN-SRS-030, FRN-SRS-031, FRN-SRS-032, FRN-SRS-091, FRN-SRS-093
{
  pkgs,
  lib ? pkgs.lib,
}:
let
  base = {
    boot.loader.grub.enable = false;
    fileSystems."/" = {
      device = "/dev/vda";
      fsType = "ext4";
    };
    boot.initrd.systemd.enable = true;
    services.greetd = {
      enable = true;
      settings.default_session.command = "true";
    };
    system.stateVersion = "26.05";
    steelbore.fairing.enable = true;
  };

  evaluate =
    extra:
    import (pkgs.path + "/nixos/lib/eval-config.nix") {
      system = null;
      modules = [
        ../module.nix
        base
        extra
        { nixpkgs.hostPlatform = pkgs.stdenv.hostPlatform; }
      ];
    };

  config = (evaluate { }).config;
  failedAssertions =
    evaluated: map (a: a.message) (lib.filter (a: !a.assertion) evaluated.config.assertions);

  initrdUnits = config.boot.initrd.systemd.units;
  systemUnits = config.systemd.units;
  initrdText = initrdUnits."fairing-initrd.service".text;
  systemText = systemUnits."fairing.service".text;
  handoffText = systemUnits."fairing-handoff.service".text;
  greetdText = systemUnits."greetd.service".text;

  # Does `text` hold the line `line` exactly?
  hasLine = text: line: lib.elem line (lib.splitString "\n" text);

  # Every value of `key` in the unit `text`, split on spaces.
  directive =
    text: key:
    lib.concatMap (line: lib.splitString " " (lib.removePrefix "${key}=" line)) (
      lib.filter (lib.hasPrefix "${key}=") (lib.splitString "\n" text)
    );

  # FRN-SRS-031: no unit, in either stage, names a Fairing unit in a hard
  # dependency, and no Fairing unit is installed as a hard dependency.
  hardDependency = lib.any (
    line:
    lib.any (key: lib.hasPrefix key line) [
      "Requires="
      "Requisite="
      "BindsTo="
      "Upholds="
    ]
    && lib.hasInfix "fairing" line
  );
  unitHardDepends = unit: unit.text != null && hardDependency (lib.splitString "\n" unit.text);
  # Both stages, kept apart: many units exist under one name in each.
  allUnits = lib.attrValues initrdUnits ++ lib.attrValues systemUnits;
  fairingUnits = lib.filter (unit: lib.hasPrefix "fairing" unit.name) (
    lib.mapAttrsToList (name: unit: unit // { inherit name; }) initrdUnits
    ++ lib.mapAttrsToList (name: unit: unit // { inherit name; }) systemUnits
  );

  checks = {
    "FRN-SRS-030: fairing-initrd has DefaultDependencies=no" =
      hasLine initrdText "DefaultDependencies=no";
    "FRN-SRS-030: fairing-initrd is after systemd-udev-trigger.service" =
      lib.elem "systemd-udev-trigger.service" (directive initrdText "After");
    "FRN-SRS-030: fairing-initrd is wanted by initrd.target" =
      lib.elem "initrd.target" initrdUnits."fairing-initrd.service".wantedBy;
    "FRN-SRS-031: no unit hard-depends on a Fairing unit" =
      !(lib.any unitHardDepends allUnits);
    "FRN-SRS-031: no Fairing unit is installed as a hard dependency" = lib.all (
      unit: unit.requiredBy == [ ] && (unit.upheldBy or [ ]) == [ ]
    ) fairingUnits;
    "FRN-SRS-032: fairing.service is ordered before greetd.service" =
      lib.elem "greetd.service" (directive systemText "Before");
    "FRN-SRS-032: greetd's start stops fairing.service first" =
      lib.elem "greetd.service" systemUnits."fairing-handoff.service".wantedBy
      && lib.elem "greetd.service" (directive handoffText "Before")
      && lib.elem "fairing.service" (directive handoffText "After")
      && hasLine handoffText "Type=oneshot";
    "FRN-SRS-017: the handoff is marked before the splash is stopped" =
      let
        exec = lib.filter (lib.hasPrefix "ExecStart=") (lib.splitString "\n" handoffText);
        index = needle: lib.lists.findFirstIndex (lib.hasInfix needle) null exec;
      in
      index "touch /run/fairing/handoff" != null
      && index "touch /run/fairing/handoff" < index "systemctl stop fairing.service";
    "the stop is not carried by greetd, which is Type=idle" =
      !(lib.hasInfix "fairing" greetdText);
    "FRN-SRS-032: fairing.service does not conflict with greetd.service" =
      !(lib.elem "greetd.service" (directive systemText "Conflicts"));
    "FRN-SRS-034: emergency mode stops the initrd splash" =
      lib.elem "emergency.target" (directive initrdText "Conflicts");
    "FRN-SRS-034: rescue and emergency mode stop the stage-2 splash" = lib.all (
      target: lib.elem target (directive systemText "Conflicts")
    ) [ "emergency.target" "rescue.target" ];
    "fairing.service starts only during a boot" =
      hasLine systemText "ConditionPathIsDirectory=/run/fairing";
    "the initrd keeps /run/fairing across switch-root" =
      hasLine initrdText "RuntimeDirectoryPreserve=yes";
    "FRN-SRS-090: the theme artefact is in the initrd" = lib.any (
      path: lib.hasSuffix "/theme.fairing" (toString (path.source or path))
    ) config.boot.initrd.systemd.storePaths;
    "FRN-SRS-090: simpledrm is loaded in the initrd" =
      lib.elem "simpledrm" config.boot.initrd.kernelModules;
    "FRN-SRS-090: quiet and splash are on the kernel command line" =
      lib.all (p: lib.elem p config.boot.kernelParams) [
        "quiet"
        "splash"
      ];
    "FRN-SRS-093: fairing.service bounds its capabilities" =
      hasLine systemText "CapabilityBoundingSet=CAP_SYS_ADMIN CAP_SYS_TTY_CONFIG";
    "FRN-SRS-093: fairing.service has ProtectSystem=strict" =
      hasLine systemText "ProtectSystem=strict";
    "FRN-SRS-093: fairing.service has ProtectHome=yes" = hasLine systemText "ProtectHome=yes";
    "FRN-SRS-093: fairing.service has PrivateTmp=yes" = hasLine systemText "PrivateTmp=yes";
    "FRN-SRS-093: fairing.service writes only its two directories" =
      hasLine systemText "ReadWritePaths=/var/lib/fairing /run/fairing";
    "the module's own configuration raises no assertion" = failedAssertions (evaluate { }) == [ ];
    "FRN-SRS-091: Plymouth alongside Fairing is a configuration error" = lib.any (
      message: lib.hasInfix "replaces Plymouth" message
    ) (failedAssertions (evaluate { boot.plymouth.enable = true; }));
    "an unregistered palette is a configuration error" = lib.any (
      message: lib.hasInfix "not a registered palette" message
    ) (failedAssertions (evaluate { steelbore.fairing.palette = "tokyo-night"; }));
    "a registered palette is accepted" =
      failedAssertions (evaluate { steelbore.fairing.palette = "steelbore-high-contrast"; }) == [ ];
    "a disabled module adds no unit" =
      !((evaluate { steelbore.fairing.enable = lib.mkForce false; }).config.systemd.units ? "fairing.service");
  };

  failures = lib.attrNames (lib.filterAttrs (_: passed: !passed) checks);
in
if failures == [ ] then
  pkgs.runCommand "fairing-module-eval" { } ''
    printf '%s\n' ${lib.escapeShellArgs (lib.attrNames checks)} > "$out"
  ''
else
  throw "steelbore.fairing module checks failed:\n  ${lib.concatStringsSep "\n  " failures}"
