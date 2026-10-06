# SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
# SPDX-License-Identifier: GPL-3.0-or-later
#
# NixOS VM test: an unencrypted Bravais-shaped machine boots through both
# splash instances to greetd. CI runs it under KVM.
#
# Verifies: FRN-SRS-014, FRN-SRS-015, FRN-SRS-017, FRN-SRS-032, FRN-SRS-036
#
# What it checks:
#   - the initrd instance drew on DRM and stopped at switch-root;
#   - the stage-2 instance carried the bar over, became ready, drew, and
#     stopped on the greetd handoff with exit 0, once logins were allowed and
#     before greetd started;
#   - the duration cache was written and /run/fairing is gone afterwards;
#   - switch-to-configuration, as nixos-rebuild switch runs it, does not
#     start the splash again over the running session.
#
# `tcg = true` runs it without KVM, under QEMU's emulator: about ten times
# slower, so the units' 5 s start timeout is relaxed for the run.
{
  pkgs,
  module,
  tcg ? false,
}:
pkgs.testers.runNixOSTest {
  name = "fairing-boot";
  requiredFeatures.kvm = !tcg;

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
      # Test VMs ship without switch-to-configuration unless asked.
      system.switch.enable = true;
      systemd.services.fairing.serviceConfig.TimeoutStartSec = lib.mkIf tcg (lib.mkForce "120s");
      boot.initrd.systemd.services.fairing-initrd.serviceConfig.TimeoutStartSec = lib.mkIf tcg (
        lib.mkForce "120s"
      );
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
    # The report carries the f32 bar widened to f64: 0.30 reads 0.30000001.
    assert 0.0 < initrd["bar"] <= 0.30 + 1e-6, initrd

    system = report("fairing.service")
    assert system["reason"] == "handoff", system
    assert system["bar"] == 1.0, system
    assert abs(system["carried_bar"] - initrd["bar"]) < 1e-3, (initrd, system)
    assert system["dbus"], system
    assert system["first_frame_ms"] is not None, system
    assert system["release_ms"] is not None, system

    machine.succeed("test \"$(systemctl show fairing.service -p ExecMainStatus --value)\" = 0")
    machine.fail("systemctl is-failed --quiet fairing-initrd.service")
    machine.fail("systemctl is-failed --quiet fairing.service")
    machine.succeed("test \"$(systemctl show fairing-handoff.service -p Result --value)\" = success")

    def monotonic(unit, prop):
        return int(machine.succeed(f"systemctl show {unit} -p {prop} --value"))

    # Both instances became active, which for Type=notify means READY=1 arrived.
    assert monotonic("fairing.service", "ActiveEnterTimestampMonotonic") > 0
    # The splash held the screen until logins were allowed, and stopped
    # before greetd ran.
    allowed = monotonic("systemd-user-sessions.service", "ActiveEnterTimestampMonotonic")
    stopped = monotonic("fairing.service", "InactiveEnterTimestampMonotonic")
    started = monotonic("greetd.service", "ExecMainStartTimestampMonotonic")
    assert 0 < allowed <= stopped <= started, (allowed, stopped, started)

    machine.succeed("test -s /var/lib/fairing/boot-duration")
    machine.succeed("test ! -e /run/fairing")

    # What nixos-rebuild switch runs: it restarts every active target.
    machine.succeed("/run/current-system/bin/switch-to-configuration test")
    machine.succeed("test \"$(systemctl show fairing.service -p ActiveState --value)\" = inactive")
    machine.succeed("test \"$(systemctl show fairing.service -p ConditionResult --value)\" = no")
  '';
}
