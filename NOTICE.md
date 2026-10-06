<!--
SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
SPDX-License-Identifier: GPL-3.0-or-later
-->

# NOTICE

**Fairing** is provided **AS IS**, without warranty of any kind, express or implied,
including but not limited to the warranties of merchantability, fitness for a particular
purpose and non-infringement. In no event shall the author or copyright holder be liable
for any claim, damages or other liability, whether in an action of contract, tort or
otherwise, arising from, out of or in connection with the software or its use.

This is a personal hobby project of Spacecraft Software (The Steelbore Standard §5.1):
no service-level commitment, no support window beyond best effort on the latest release,
no obligation to accept contributions. It runs in the boot path of an operating system;
test it on your own hardware before relying on it.

The binding terms are those of the project licence, **GPL-3.0-or-later** (see
`LICENSE`), whose sections 15 and 16 govern warranty and liability. The manual is
licensed CC-BY-SA-4.0. Where this notice and the licence differ, the licence prevails.

## Third-party notices

Per-item attribution with authors and source URLs is kept in [CREDITS.md](CREDITS.md)
(Steelbore Standard §15.3); this section is the summary.

- **Inconsolata** (Regular, version 3.100), bundled in `crates/fairing-render/assets/fonts/`.
  Copyright 2006 The Inconsolata Project Authors (<https://github.com/cyrealtype/Inconsolata>).
  Licensed under the SIL Open Font License, Version 1.1; the upstream notice ships verbatim
  as `OFL.txt` beside the font and the licence text as `LICENSES/OFL-1.1.txt`.
- **Steelbore palette file** (`steelbore.toml` 3.5.0), bundled in
  `crates/fairing-theme/assets/`, is the `steelbore-color-palette` skill asset of Spacecraft
  Software, GPL-3.0-or-later, copied byte for byte so that no colour value is ever retyped.
- The Linux virtual console's default 16-colour palette, used only when the mono theme is
  drawn on a framebuffer, is reproduced from `drivers/tty/vt/vt.c` of the Linux kernel
  (GPL-2.0), as data.

Copyright (C) 2026 Mohamed Hammad & Spacecraft Software ·
<https://Fairing.SpacecraftSoftware.org/>
