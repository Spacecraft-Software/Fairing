# SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
# SPDX-License-Identifier: GPL-3.0-or-later
#
# NixOS VM test: an unencrypted Bravais-shaped machine boots through both
# splash instances to greetd. It needs KVM, so it runs in CI and on the
# maintainer's machine, not in a container. It becomes the evidence for the
# handoff requirements (FRN-SRS-014, 015, 017, 032, 036, 090) once it has run
# green there; until then it carries no marker.
#
# What it checks:
#   - the initrd instance drew on DRM and stopped at switch-root;
#   - the stage-2 instance carried the bar over, drew, and stopped on the
#     greetd handoff with exit 0, before greetd started;
#   - the duration cache was written and /run/fairing is gone afterwards;
#   - restarting the boot targets, as nixos-rebuild switch does, does not
#     start the splash again over the running session.
{
  pkgs,
  module,
}:
pkgs.testers.runNixOSTest {
  name = "fairing-boot";

  nodes.machine =
    { config, lib, ... }:
    {
      imports = [ module ];
      steelbore.fairing = {
        enable = true;
        # QEMU's standard VGA; the splash starts on it in the initrd.
        kmsModules = [ "bochs" ];
      };
      boot.initrd.systemd.enable = true;
      services.greetd = {
        enable = true;
        settings.default_session.command = "${lib.getExe' config.services.greetd.package "agreety"} --cmd true";
      };
    };

  testScript = ''
    import json

    def report(unit):
        """The splash's JSON report for this boot, from its journal."""
        lines = machine.succeed(
            f"journalctl -b -u {unit} -o cat --no-pager | grep '^{{\"data\"'"
        ).strip().splitlines()
        assert len(lines) == 1, f"{unit}: {lines}"
        return json.loads(lines[0])["data"]

    machine.wait_for_unit("greetd.service")

    initrd = report("fairing-initrd.service")
    assert initrd["reason"] == "switch-root", initrd
    assert initrd["backend"] == "drm", initrd
    assert 0.0 < initrd["bar"] <= 0.30, initrd

    system = report("fairing.service")
    assert system["reason"] == "handoff", system
    assert system["bar"] == 1.0, system
    assert abs(system["carried_bar"] - initrd["bar"]) < 1e-3, (initrd, system)
    assert system["dbus"], system

    machine.succeed("test \"$(systemctl show fairing.service -p ExecMainStatus --value)\" = 0")
    # The splash stopped before greetd ran.
    stopped = int(machine.succeed("systemctl show fairing.service -p InactiveEnterTimestampMonotonic --value"))
    started = int(machine.succeed("systemctl show greetd.service -p ExecMainStartTimestampMonotonic --value"))
    assert 0 < stopped <= started, (stopped, started)

    machine.succeed("test -s /var/lib/fairing/boot-duration")
    machine.succeed("test ! -e /run/fairing")

    # What switch-to-configuration does: restart every active target.
    machine.succeed("systemctl start sysinit.target multi-user.target")
    machine.succeed("test \"$(systemctl show fairing.service -p ActiveState --value)\" = inactive")
    machine.succeed("test \"$(systemctl show fairing.service -p ConditionResult --value)\" = no")
  '';
}
