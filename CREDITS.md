<!--
SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
SPDX-License-Identifier: GPL-3.0-or-later
-->

# Credits

Third-party work that ships inside Fairing or that Fairing reproduces (The Steelbore
Standard §15.3). Crates pulled from crates.io are qualified in
[DEPENDENCIES.md](DEPENDENCIES.md); licence texts live in `LICENSES/`. An entry is added
in the same commit that brings the work in.

| Name | Author | License | Source | Scope |
|---|---|---|---|---|
| Inconsolata Regular 3.100 | The Inconsolata Project Authors (designed by Raph Levien) | OFL-1.1, no Reserved Font Name (`LICENSES/OFL-1.1.txt`; upstream notice as `OFL.txt` beside the font) | <https://github.com/cyrealtype/Inconsolata> (`fonts/ttf/Inconsolata-Regular.ttf`) | Bundled in `crates/fairing-render/assets/fonts/`; the only face the splash draws (Steelbore §12 body/code face) |
| Linux virtual console default palette | Linux kernel contributors, `drivers/tty/vt/vt.c` | GPL-2.0, reproduced as data | <https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/drivers/tty/vt/vt.c> | The sixteen console colours the mono theme's ANSI slots map to when drawn on a framebuffer (`crates/fairing-render/src/palette.rs`) |
| Steelbore palette file 3.5.0 | Mohamed Hammad & Spacecraft Software | GPL-3.0-or-later (own header) | `steelbore-color-palette` skill asset, `assets/steelbore.toml` | Vendored byte-identically in `crates/fairing-theme/assets/`; every colour value in Fairing comes from it |
