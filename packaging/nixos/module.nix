# SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The `steelbore.fairing` NixOS module: the boot splash for Steelbore OS
# Bravais, from the systemd initrd to the greetd handoff.
#
# Two units carry the splash. `fairing-initrd.service` draws from early boot
# until switch-root and leaves the bar value in /run/fairing/state;
# `fairing.service` picks it up after switch-root and holds the screen until
# greetd starts, when `fairing-handoff.service` stops it. The shutdown splash
# arrives with `fairing splash --stage shutdown` at milestone M4.
#
# Neither unit is required by anything: a splash that fails, hangs or is
# missing never stops a boot (FRN-SRS-031).
#
# Both instances answer systemd's password questions in the splash (M3). While
# an agent is up it keeps /run/fairing/agent, and systemd's console agent is
# conditioned on that file's absence, as it is on Plymouth's pid file. Every
# way out of the splash, a crash included, removes the file and starts the
# console agent's path unit again, so a question still open goes back to the
# console (FRN-SRS-027).
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
      ++ lib.optional (!cfg.agent.enable) "--no-agent"
    );

  # The console agent's unit, either stage.
  consoleAgent = "systemd-ask-password-console.service";

  # What every instance runs on its way out: the agent flag goes, whatever
  # ended the splash, and the console agent looks at the question directory
  # again. `--job-mode=fail` leaves a stop already queued for the path unit
  # (switch-root, shutdown) alone.
  # Implements: FRN-SRS-027
  agentStopPost = systemd: coreutils: [
    "-${coreutils}/bin/rm -f /run/fairing/agent"
    "-${systemd}/bin/systemctl --no-block --job-mode=fail restart systemd-ask-password-console.path"
  ];

  # Once the splash is ready its agent is up and the flag exists: the console
  # agent's path unit, which may already be watching, is started again so its
  # condition is checked now. A watch left running would fire the skipped
  # service at every question until it hits its trigger limit and fails.
  # `try-restart` leaves a path unit that is not yet active to its own start.
  agentStartPost = systemd: [
    "-${systemd}/bin/systemctl --no-block try-restart systemd-ask-password-console.path"
  ];

  # The console agent, its watch and its service, asks only while Fairing's
  # agent is down; systemd conditions both the same way on Plymouth.
  consoleAgentDropIn = {
    unitConfig.ConditionPathExists = "!/run/fairing/agent";
  };

  # In the initrd the console agent also waits for the splash to be ready,
  # its agent up and the flag raised, so its watch never sees a question
  # before the condition can hold it back. Not in stage 2: fairing.service
  # is ordered after the mount of /var/lib/fairing, and an encrypted /var
  # waits for this very agent.
  initrdConsoleAgentDropIn = consoleAgentDropIn // {
    after = [ "fairing-initrd.service" ];
  };

  # The agent types through a fixed US layout (FRN-SRS-028): on any other
  # console keymap a passphrase would be sent wrong, so the default leaves
  # those machines to systemd's console agent, which honours the keymap.
  usLayout =
    if config.console.useXkbConfig then
      config.services.xserver.xkb.layout == "us"
    else
      lib.isString config.console.keyMap
      && lib.elem config.console.keyMap [
        "us"
        "en-latin9"
      ];

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
        the D-Bus client, so the initrd stays small. The initrd splash does not
        use the initrd's bus; emergency mode stops it through a conflict.
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

    agent.enable = lib.mkOption {
      type = lib.types.bool;
      default = usLayout;
      defaultText = lib.literalMD "`true` when the console keymap is US";
      description = ''
        Answer password questions, such as a LUKS passphrase, in the splash.
        When false, systemd's console agent asks, and a question stops the
        splash so its prompt is visible. The agent types through a fixed US
        layout, so it is off by default on any other console keymap.
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
    warnings = lib.optional (cfg.agent.enable && !usLayout) ''
      steelbore.fairing.agent types passphrases through a fixed US layout,
      but the console keymap is not US: passphrases will be sent wrong. Set
      steelbore.fairing.agent.enable = false to let systemd's console agent
      ask with the configured keymap.
    '';

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
    # evdev: the agent reads keyboards through /dev/input/event*, and NixOS
    # builds evdev as a module that nothing else loads in the initrd.
    boot.initrd.kernelModules = [
      "simpledrm"
    ]
    ++ cfg.kmsModules
    ++ lib.optional cfg.agent.enable "evdev";

    environment.systemPackages = [ cfg.package ];

    boot.initrd.systemd.storePaths = [
      (lib.getExe cfg.initrdPackage)
      "${artefact}/theme.fairing"
    ]
    ++ lib.optional cfg.agent.enable "${pkgs.coreutils}/bin/rm";

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
      # switched (FRN-SRS-014). Up, with its agent, before LUKS asks.
      before = [
        "initrd-switch-root.target"
        "shutdown.target"
      ]
      ++ lib.optional cfg.agent.enable "cryptsetup-pre.target";
      # cryptsetup-pre.target is passive: without a pull-in the ordering
      # above would order against nothing (systemd.special(7)).
      wants = lib.optional cfg.agent.enable "cryptsetup-pre.target";
      # Without the agent, a password question steps the splash aside so the
      # console agent's prompt is visible.
      # Emergency mode takes the console, and the initrd has no bus to tell
      # the splash: the conflict stops it (FRN-SRS-034 in the initrd).
      conflicts = [
        "initrd-switch-root.target"
        "shutdown.target"
        "emergency.target"
      ]
      ++ lib.optional (!cfg.agent.enable) consoleAgent;
      serviceConfig = common // {
        ExecStart = splash cfg.initrdPackage "initrd";
        ExecStartPost = lib.mkIf cfg.agent.enable (agentStartPost config.boot.initrd.systemd.package);
        ExecStopPost = lib.mkIf cfg.agent.enable (
          agentStopPost config.boot.initrd.systemd.package pkgs.coreutils
        );
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
      ]
      ++ lib.optional (!cfg.agent.enable) consoleAgent;
      restartIfChanged = false;
      stopIfChanged = false;
      serviceConfig = common // {
        ExecStart = splash cfg.splashPackage "system";
        ExecStartPost = lib.mkIf cfg.agent.enable (agentStartPost config.systemd.package);
        ExecStopPost = lib.mkIf cfg.agent.enable (agentStopPost config.systemd.package pkgs.coreutils);
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
        # Not ProtectKernelTunables: the fbdev fallback pauses fbcon's cursor
        # blink through /sys/class/graphics/fbcon/cursor_blink.
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

    boot.initrd.systemd.services.systemd-ask-password-console =
      lib.mkIf cfg.agent.enable initrdConsoleAgentDropIn;
    systemd.services.systemd-ask-password-console = lib.mkIf cfg.agent.enable consoleAgentDropIn;
    boot.initrd.systemd.paths.systemd-ask-password-console =
      lib.mkIf cfg.agent.enable initrdConsoleAgentDropIn;
    systemd.paths.systemd-ask-password-console = lib.mkIf cfg.agent.enable consoleAgentDropIn;

    # The handoff (FRN-SRS-017, FRN-SRS-032): greetd's start pulls this in and
    # waits for it. It marks the handoff, then stops the splash and waits for
    # it to exit, so DRM master is free when greetd opens the console. The
    # marker is what makes that SIGTERM the handoff (a full bar, the durations
    # cached); a stop without it is a plain stop. A unit of its own, not an
    # ExecStartPre of greetd: greetd is Type=idle, and systemd would hold
    # every command of it, ExecStartPre included, for its 5 s idle wait.
    # It runs once logins are allowed, after systemd-user-sessions.service as
    # greetd itself and plymouth-quit.service do; ordered after the splash
    # alone, it would run as soon as the splash sent READY=1 and end it at the
    # start of stage 2. Both commands may fail: once the splash has gone,
    # /run/fairing is gone.
    systemd.services.fairing-handoff = {
      description = "Fairing hands the screen to greetd";
      unitConfig.DefaultDependencies = "no";
      after = [
        "fairing.service"
        "systemd-user-sessions.service"
      ];
      before = [ "greetd.service" ];
      wantedBy = [ "greetd.service" ];
      restartIfChanged = false;
      serviceConfig = {
        Type = "oneshot";
        ExecStart = [
          "-${pkgs.coreutils}/bin/touch /run/fairing/handoff"
          "-${config.systemd.package}/bin/systemctl stop fairing.service"
        ];
      };
    };
  };
}
