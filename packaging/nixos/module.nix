# SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The `steelbore.fairing` NixOS module: the boot splash for Steelbore OS
# Bravais, from the systemd initrd to the greetd handoff.
#
# Two units carry the splash. `fairing-initrd.service` draws from early boot
# until switch-root and leaves the bar value in /run/fairing/state;
# `fairing.service` picks it up after switch-root and holds the screen until
# greetd starts. The shutdown splash, the third unit, arrives with
# `fairing splash --stage shutdown` at milestone M4.
#
# Neither unit is required by anything: a splash that fails, hangs or is
# missing never stops a boot (FRN-SRS-031).
#
# Implements: FRN-SRS-090
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.steelbore.fairing;

  # The registered palette slugs, read from the palette file the binary is
  # built from, never retyped.
  registered =
    (lib.importTOML ../../crates/fairing-theme/assets/steelbore.toml).meta."registered-set";

  artefact = pkgs.callPackage ./theme.nix {
    fairing = cfg.package;
    inherit (cfg) theme;
  };

  # The command line of one splash instance.
  splash =
    package: stage:
    lib.escapeShellArgs (
      [
        (lib.getExe package)
        "splash"
        "--stage"
        stage
        "--theme"
        "${artefact}/theme.fairing"
      ]
      ++ lib.optionals (cfg.palette != null) [
        "--palette"
        cfg.palette
      ]
    );

  # Shared by both instances. A splash has nothing to say to a core dump, and
  # every wait it makes is bounded by five seconds (FRN-SRS-037), so neither
  # start nor stop needs longer.
  common = {
    Type = "notify";
    NotifyAccess = "main";
    TimeoutStartSec = "5s";
    TimeoutStopSec = "1s";
    LimitCORE = 0;
  };
in
{
  options.steelbore.fairing = {
    enable = lib.mkEnableOption "the Fairing boot splash";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ../default.nix { };
      defaultText = lib.literalExpression "pkgs.callPackage ./packaging/default.nix { }";
      description = ''
        The full Fairing build. It compiles the theme at build time and is
        installed for `fairing theme check` and `fairing preview`.
      '';
    };

    splashPackage = lib.mkOption {
      type = lib.types.package;
      default = cfg.package.override { withThemeTool = false; };
      defaultText = lib.literalExpression "config.steelbore.fairing.package.override { withThemeTool = false; }";
      description = ''
        The build `fairing.service` runs after switch-root: Fairing without
        the Nickel evaluator, which the boot never needs.
      '';
    };

    initrdPackage = lib.mkOption {
      type = lib.types.package;
      default = cfg.package.override {
        withThemeTool = false;
        withDbus = false;
      };
      defaultText = lib.literalExpression "config.steelbore.fairing.package.override { withThemeTool = false; withDbus = false; }";
      description = ''
        The build the initrd carries: without the Nickel evaluator and without
        the D-Bus client (no bus runs in the initrd), so the initrd stays small.
      '';
    };

    theme = lib.mkOption {
      type = lib.types.path;
      default = ../../themes/steelbore.ncl;
      defaultText = lib.literalExpression "./themes/steelbore.ncl";
      description = ''
        The Nickel theme to compile into the initrd. Its directory is copied
        into the store whole, because a theme may import files beside it.
      '';
    };

    palette = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "steelbore-high-contrast";
      description = ''
        A registered palette slug that overrides the theme's own. The
        `fairing.theme=` kernel parameter, `SPACECRAFT_THEME` and `NO_COLOR`
        still apply in their §11.6 order when this is null.
      '';
    };

    kmsModules = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      example = [ "i915" ];
      description = ''
        Native KMS drivers to load in the initrd, so the splash moves from
        simpledrm to the real driver early. simpledrm is always added.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      # Implements: FRN-SRS-091
      {
        assertion = !config.boot.plymouth.enable;
        message = "steelbore.fairing replaces Plymouth; set boot.plymouth.enable = false.";
      }
      {
        assertion = config.boot.initrd.systemd.enable;
        message = "steelbore.fairing runs in the systemd initrd; set boot.initrd.systemd.enable = true.";
      }
      {
        assertion = cfg.palette == null || lib.elem cfg.palette registered;
        message = "steelbore.fairing.palette `${toString cfg.palette}` is not a registered palette; one of: ${lib.concatStringsSep ", " registered}.";
      }
      {
        assertion = config.services.greetd.enable;
        message = "steelbore.fairing holds the screen until greetd starts; enable services.greetd.";
      }
    ];

    boot.kernelParams = [
      "quiet"
      "splash"
    ];
    boot.initrd.kernelModules = [ "simpledrm" ] ++ cfg.kmsModules;

    environment.systemPackages = [ cfg.package ];

    boot.initrd.systemd.storePaths = [
      (lib.getExe cfg.initrdPackage)
      "${artefact}/theme.fairing"
    ];

    boot.initrd.systemd.services.fairing-initrd = {
      description = "Fairing boot splash (initrd)";
      # Implements: FRN-SRS-030
      unitConfig.DefaultDependencies = "no";
      # The KMS drivers of boot.initrd.kernelModules are loaded first, so the
      # first frame goes to the real device when there is one.
      after = [
        "systemd-modules-load.service"
        "systemd-udev-trigger.service"
      ];
      wantedBy = [ "initrd.target" ];
      # Stopped, and so finished writing /run/fairing/state, before root is
      # switched (FRN-SRS-014).
      before = [
        "initrd-switch-root.target"
        "shutdown.target"
      ];
      # A password question steps the splash aside so the console agent's
      # prompt is visible; the in-splash agent replaces this at M3.
      # Emergency mode takes the console, and the initrd has no bus to tell
      # the splash: the conflict stops it (FRN-SRS-034 in the initrd).
      conflicts = [
        "initrd-switch-root.target"
        "shutdown.target"
        "emergency.target"
        "systemd-ask-password-console.service"
      ];
      serviceConfig = common // {
        ExecStart = splash cfg.initrdPackage "initrd";
        # /run/fairing outlives this unit and switch-root: it carries the
        # handoff, and its presence tells stage 2 that this is a boot.
        RuntimeDirectory = "fairing";
        RuntimeDirectoryPreserve = "yes";
      };
    };

    systemd.services.fairing = {
      description = "Fairing boot splash";
      unitConfig = {
        DefaultDependencies = "no";
        # Only during a boot. switch-to-configuration restarts every active
        # target, which would otherwise start the splash over a running
        # session; /run/fairing exists from the initrd until this unit stops.
        ConditionPathIsDirectory = "/run/fairing";
      };
      after = [
        "systemd-modules-load.service"
        "systemd-udev-trigger.service"
      ];
      wantedBy = [ "sysinit.target" ];
      # Implements: FRN-SRS-032
      #
      # greetd starts after this unit and, before it runs, stops it: the stop
      # (SIGTERM, the handoff) completes before greetd takes the screen. A
      # `Conflicts=greetd.service` here would put both in the boot
      # transaction, and systemd resolves that by dropping greetd's start.
      before = [
        "greetd.service"
        "shutdown.target"
      ];
      # Rescue and emergency mode take the console. `OnFailure=` starts them
      # without isolating, so the conflict is what stops the splash; D-Bus would
      # only see it a poll later (FRN-SRS-034).
      conflicts = [
        "shutdown.target"
        "emergency.target"
        "rescue.target"
        "systemd-ask-password-console.service"
      ];
      restartIfChanged = false;
      stopIfChanged = false;
      serviceConfig = common // {
        ExecStart = splash cfg.splashPackage "system";
        RuntimeDirectory = "fairing";
        StateDirectory = "fairing";
        # Implements: FRN-SRS-093
        # Spelt as the requirement spells them, one line each, so the unit
        # file can be inspected against its text.
        CapabilityBoundingSet = "CAP_SYS_ADMIN CAP_SYS_TTY_CONFIG";
        ProtectSystem = "strict";
        ProtectHome = "yes";
        PrivateTmp = "yes";
        ReadWritePaths = "/var/lib/fairing /run/fairing";
        # Beyond FRN-SRS-093: nothing the splash does needs these.
        NoNewPrivileges = true;
        PrivateNetwork = true;
        RestrictAddressFamilies = [ "AF_UNIX" ];
        ProtectKernelModules = true;
        ProtectKernelLogs = true;
        ProtectKernelTunables = true;
        ProtectControlGroups = true;
        ProtectClock = true;
        ProtectHostname = true;
        RestrictNamespaces = true;
        RestrictRealtime = true;
        RestrictSUIDSGID = true;
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
        SystemCallArchitectures = "native";
        UMask = "0077";
      };
    };

    # The handoff (FRN-SRS-017): greetd's start marks it, then stops the
    # splash and waits for it, so DRM master is free when greetd opens the
    # console. The marker is what makes this SIGTERM the handoff (a full bar,
    # the durations cached); a stop without it is a plain stop. Both lines are
    # allowed to fail: after the splash has gone, /run/fairing is gone too.
    systemd.services.greetd.serviceConfig.ExecStartPre = [
      "-${pkgs.coreutils}/bin/touch /run/fairing/handoff"
      "-${config.systemd.package}/bin/systemctl stop fairing.service"
    ];
  };
}
