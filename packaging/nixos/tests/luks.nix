# SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
# SPDX-License-Identifier: GPL-3.0-or-later
#
# NixOS VM test: a LUKS root unlocked at boot. With the agent on (the
# default), the passphrase is typed on QEMU's virtual keyboard into the
# splash's own prompt, through evdev and the grab; with `agent = false`, the
# splash steps aside and systemd's console agent asks on the serial console.
# Either way the machine boots to greetd. CI runs both under KVM.
#
# Verifies: FRN-SRS-022, FRN-SRS-025, FRN-SRS-092
#
# What it checks, agent on:
#   - the initrd splash grabbed a keyboard for the question and answered it
#     once;
#   - systemd's console agent stood aside while the splash agent was up;
#   - the passphrase appears nowhere in the boot's journal;
#   - the root is the unlocked LUKS device.
# Agent off: the console agent asked, the splash ran with no agent, and the
# boot still reached greetd.
#
# `tcg = true` runs it without KVM, under QEMU's emulator.
{
  pkgs,
  module,
  agent ? true,
  tcg ? false,
}:
pkgs.testers.runNixOSTest {
  name = if agent then "fairing-luks" else "fairing-luks-no-agent";
  requiredFeatures.kvm = !tcg;

  nodes.machine =
    { config, lib, ... }:
    {
      imports = [ module ];
      virtualisation = {
        emptyDiskImages = [ 512 ];
        useBootLoader = true;
        # The encrypted root has no store of its own.
        mountHostNixStore = true;
        useEFIBoot = true;
      };
      boot.loader.systemd-boot.enable = true;
      environment.systemPackages = [ pkgs.cryptsetup ];
      boot.initrd.systemd.enable = true;
      steelbore.fairing = {
        enable = true;
        agent.enable = agent;
        kmsModules = [ "bochs" ];
      };
      systemd.services.fairing.serviceConfig.TimeoutStartSec = lib.mkIf tcg (lib.mkForce "120s");
      boot.initrd.systemd.services.fairing-initrd.serviceConfig.TimeoutStartSec = lib.mkIf tcg (
        lib.mkForce "120s"
      );
      services.greetd = {
        enable = true;
        settings.default_session.command = "${lib.getExe' config.services.greetd.package "agreety"} --cmd true";
      };

      specialisation.boot-luks.configuration = {
        boot.initrd.luks.devices = lib.mkVMOverride {
          cryptroot.device = "/dev/vdb";
        };
        virtualisation.rootDevice = "/dev/mapper/cryptroot";
      };
    };

  testScript =
    { nodes, ... }:
    let
      boot-luks = nodes.machine.specialisation.boot-luks.configuration.system.build.toplevel;
    in
    ''
      import json

      def report(unit):
          """The splash's JSON report for this boot, from its journal."""
          lines = machine.succeed(
              f"journalctl -b -u {unit} -o cat --no-pager | grep '^{{\"data\"'"
          ).strip().splitlines()
          assert len(lines) == 1, f"{unit}: {lines}"
          return json.loads(lines[0])["data"]

      machine.wait_for_unit("multi-user.target")
      machine.succeed("echo -n supersecret | cryptsetup luksFormat -q --iter-time=1 /dev/vdb -")
      machine.succeed("echo -n supersecret | cryptsetup luksOpen -q /dev/vdb cryptroot")
      machine.succeed("mkfs.ext4 /dev/mapper/cryptroot")
      machine.succeed("${boot-luks}/bin/switch-to-configuration boot")
      machine.succeed("sync")
      machine.crash()

      machine.start()
    ''
    + (
      if agent then
        ''
          # The splash's journal line, forwarded to the serial console.
          machine.wait_for_console_text("a password question is open; 1 keyboard")
          machine.send_chars("supersecret\n")
          machine.wait_for_unit("greetd.service")

          initrd = report("fairing-initrd.service")
          assert initrd["agent"], initrd
          assert initrd["answered"] == 1, initrd
          assert initrd["reason"] == "switch-root", initrd

          journal = machine.succeed("journalctl -b -o cat --no-pager")
          assert "the password question was answered" in journal
          # The console agent was asked to start and stood aside.
          assert "Dispatch Password Requests to Console was skipped" in journal, "console agent"
          # FRN-SRS-025: not in any message, field or unit's output.
          machine.fail("journalctl -b -o verbose --no-pager | grep -q supersecret")
          # The flag is gone with the splash.
          machine.fail("test -e /run/fairing/agent")
        ''
      else
        ''
          machine.wait_for_console_text("Please enter passphrase for disk cryptroot")
          machine.send_console("supersecret\n")
          machine.wait_for_unit("greetd.service")

          initrd = report("fairing-initrd.service")
          assert not initrd["agent"], initrd
          assert initrd["answered"] == 0, initrd
        ''
    )
    + ''
      assert "/dev/mapper/cryptroot on / type ext4" in machine.succeed("mount")
      machine.fail("systemctl is-failed --quiet fairing-initrd.service")
    '';
}
