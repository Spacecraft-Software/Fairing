<!--
SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
SPDX-License-Identifier: GPL-3.0-or-later
-->

# Security Policy

## Reporting a vulnerability

Report privately through GitHub's private vulnerability reporting on
<https://github.com/Spacecraft-Software/Fairing/security/advisories/new>, or by e-mail to
<Mohamed.Hammad@SpacecraftSoftware.org>. **Never open a public issue for a security
problem.** Please include the affected version or commit, steps to reproduce, and your
assessment of impact.

## Acknowledgement target

Fairing is a hobby project (Steelbore Standard §5.1): you will receive an acknowledgement
within **14 days**. There is no service-level commitment on a fix date; critical issues
(anything that could disclose a passphrase, stop a machine from booting, or weaken the
password-agent path) are worked first.

## Scope

- In scope: the `fairing` binary and the four workspace crates, the NixOS module and
  systemd units once they ship, and the compiled-theme and ask-password parsers.
- Out of scope: the `xtask` developer tooling, documentation, and vulnerabilities in
  dependencies that are already published upstream (report those upstream; tell us too so
  the qualification record in `DEPENDENCIES.md` is updated).

## Supported versions

Best effort on the **latest release only** (§26.4). There are no backports. Until the
first release, the `main` branch is the only supported line.

## Coordinated disclosure

We ask for a **90-day** embargo from acknowledgement, extendable by agreement when a fix
needs a Bravais release to reach users. After the fix ships, or after 90 days, the
finding is published as a GitHub security advisory citing the affected releases by SBOM
(§26.2). A finding that is already public is handled without embargo.

## Credit

Reporters are credited by name in the advisory and in `CHANGELOG.md` unless they ask to
stay anonymous. Security anomalies are classified S1 by default (§25.2).
