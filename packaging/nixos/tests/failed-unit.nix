# SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
# SPDX-License-Identifier: GPL-3.0-or-later
#
# NixOS VM test: a unit fails during stage 2 and the splash leaves the screen
# to the console within a second (FRN-SRS-034), exit 0, while the boot goes
# on. greetd is installed but not started, so nothing but the failure can
# end the splash. `tcg = true` runs it without KVM.
#
# Verifies: FRN-SRS-034
{
  pkgs,
  module,
  tcg ? false,
}:
pkgs.testers.runNixOSTest {
  name = "fairing-failed-unit";
  requiredFeatures.kvm = !tcg;

  nodes.machine =
    { config, lib, ... }:
    {
      imports = [ module ];
      steelbore.fairing = {
        enable = true;
        kmsModules = [ "bochs" ];
      };
      boot.initrd.systemd.enable = true;
      systemd.services.fairing.serviceConfig.TimeoutStartSec = lib.mkIf tcg (lib.mkForce "120s");
      boot.initrd.systemd.services.fairing-initrd.serviceConfig.TimeoutStartSec = lib.mkIf tcg (
        lib.mkForce "120s"
      );
      services.greetd = {
        enable = true;
        settings.default_session.command = "${lib.getExe' config.services.greetd.package "agreety"} --cmd true";
      };
      # Keep greetd out of the boot (graphical.target would still pull it in as
      # display-manager.service): the splash would otherwise stop at the
      # handoff, and the failure would never be its reason to go.
      systemd.services.greetd.enable = lib.mkForce false;

      # It fails only once the splash can see it fail: after the splash is up
      # and the bus is running, with time for a first reading. A failure the
      # first reading already shows does not end the splash (FRN-SRS-034).
      systemd.services.doomed = {
        description = "A unit that fails a few seconds into stage 2";
        wantedBy = [ "multi-user.target" ];
        after = [
          "fairing.service"
          "dbus.service"
        ];
        serviceConfig = {
          Type = "oneshot";
          ExecStart = "${pkgs.coreutils}/bin/sleep ${if tcg then "30" else "5"}";
          ExecStartPost = "${pkgs.coreutils}/bin/false";
        };
      };
    };

  testScript = ''
    import json

    machine.wait_for_unit("multi-user.target")
    machine.wait_until_succeeds(
        "test \"$(systemctl show fairing.service -p ActiveState --value)\" = inactive"
    )
    line = machine.succeed(
        "journalctl -b -u fairing.service -o cat --no-pager | grep '^{\"data\"'"
    ).strip()
    data = json.loads(line)["data"]
    assert data["reason"] == "failed-unit", data
    machine.succeed("test \"$(systemctl show fairing.service -p ExecMainStatus --value)\" = 0")

    failed = int(machine.succeed("systemctl show doomed.service -p InactiveEnterTimestampMonotonic --value"))
    gone = int(machine.succeed("systemctl show fairing.service -p ExecMainExitTimestampMonotonic --value"))
    assert 0 < gone - failed <= 1_000_000, f"the splash left {gone - failed} us after the failure"
  '';
}
