<!--
SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
SPDX-License-Identifier: GPL-3.0-or-later
-->

# Compliance — Tailoring Register

Fairing conforms to **The Steelbore Standard v2.12** as a **Category B** project with two
subsystems raised to **Category A**: the password agent (`crates/fairing-askpass`) and the
initrd lifecycle unit (`fairing-initrd.service`). This register (§19.5) lists every clause
that is tailored or not applicable; a clause absent from the table is applied in full.
Dates are ISO 8601. Entries are re-read at each G3 release gate.

| Clause | Status | Justification | Date |
|---|---|---|---|
| §13 UI/UX design system | not-applicable | Fairing draws to a raw DRM/fbdev surface and ships no widget toolkit; there is no component system to declare. The §11 palette still applies through the theme. | 2026-10-06 |
| §10 Key bindings, remappability | tailored | The only bindings are Escape, Enter, Backspace, the arrows, PgUp/PgDn and j/k in the log overlay. Remapping is through the theme file, not a runtime keymap, because an initrd has no settings surface. | 2026-10-06 |
| §18.3 GUI accessible names and roles (AccessKit) | tailored | No toolkit widgets exist to publish roles for. The §18 obligation is met by the text backend (FRN-SRS-070 to FRN-SRS-073): status never colour-only, no animation in accessible mode, verified with a screen reader on the console per §18.4 at M4. | 2026-10-06 |
| §6.1 POSIX compliance | tailored | Fairing is Linux-only by nature (DRM/KMS, systemd, evdev). The CLI, file formats and scripts stay POSIX-portable; the daemon does not. | 2026-10-06 |
| §11.6 System theme resolution | tailored | Stage 1 reads the explicit slug (`--palette`), the `fairing.theme=` kernel parameter (wired at M2), `SPACECRAFT_THEME` and the family default. The §11.6.4 declaration file is not read on the boot path: the initrd carries no `/etc/steelbore`, so the declared default travels inside the compiled theme artefact instead (M2). Stage 2 pins a `-high-contrast` or mono sibling only when the command line or `SPACECRAFT_THEME` named it (§11.6.3 sources 1 and 2; the kernel parameter stands in for the source-3 declaration and is overlaid like one), then overlays `NO_COLOR` or `--no-color` → mono and the accessible-mode toggle → high contrast; a text console has no platform high-contrast or light/dark signal, so those sources are absent, and the theme is resolved once at start-up and held (§11.6.2: a splash cannot switch palettes atomically mid-boot). An unusable slug from the environment or a declaration is skipped as §11.6 requires; an unregistered slug given explicitly on the command line is a usage error (exit 2), because the operator who typed it can see the answer. | 2026-10-06 |
| §11.1 Colour values only through palette tokens | tailored | `fairing-render/src/palette.rs` carries the Linux console's default 16-colour table (`drivers/tty/vt/vt.c`) as the only bare colour values outside the vendored `steelbore.toml`. They are kernel data, not design values: when the mono theme is drawn on a framebuffer there is no terminal to resolve ANSI slots, so the frame shows what the console itself would show. Every other colour is a §11.1 role token read from the palette file at build time. | 2026-10-06 |
| §20.1 Requirements authored in Texinfo | tailored | The canonical Needs and Requirements nodes of `doc/fairing.texi` are generated from `doc/requirements.toml` by `cargo xtask req-texi`. The TOML is the structured authoring source that also feeds the §21.3 traceability tooling; the generated chapters are committed, never hand-edited, and CI fails when they are stale. Nothing is maintained twice. | 2026-10-06 |

## Related records

- Dependency qualification (§24.1): [DEPENDENCIES.md](DEPENDENCIES.md).
- Security policy (§26.1): [SECURITY.md](SECURITY.md).
- Requirement set (§20): `doc/requirements.toml` → `doc/fairing.texi`.
- Traceability (§21.3): `cargo xtask trace` → `target/trace/matrix.{json,md}` (CI artifact).
- Progress (§17): `cargo xtask progress` over `PLAN.md` and `TODO.md`.

## Gate status

| Gate | State |
|---|---|
| G1 — Requirements | Open. 68 requirements drafted with ID, rationale, source, priority, method and status; the maintainer confirmed the seven open scoping questions on 2026-10-06. Baselining (`status = "baselined"`) is the maintainer's act. |
| G2 — Design | Not started. Budgets (§22), the ICD (§23) and dependency pinning (§24) are declared in the PRD and move into the manual at G2. |
| G3 — Release | Not started. |
