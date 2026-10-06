<!--
SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
SPDX-License-Identifier: GPL-3.0-or-later
-->

# CLAUDE.md — Fairing

@AGENTS.md

> Record project knowledge in `AGENTS.md`, not here. This file holds only
> Claude-Code-only context (skills, `.claude/`, slash commands, MCP client).

## Skills to load

- `spacecraft-steelbore-standard` — master Standard (load §4, §17, §19–§26 references as needed)
- `microsoft-rust-guidelines` — mandatory before touching any `.rs` file
- `spacecraft-rust-guidelines` — concurrency and performance doctrine
- `spacecraft-cli-standard` and `spacecraft-agentic-cli` — the `fairing` CLI surface
- `spacecraft-texinfo-document` — `doc/fairing.texi` and the generated chapters
- `spacecraft-nix-guidelines` — `flake.nix`, `packaging/default.nix`, the NixOS module (M2)
- `spacecraft-nickel-guidelines` — the theme contract (M2)
- `steelbore-color-palette` — the only source of colour values (M2)
- `spacecraft-accessibility-support` — the text backend and §18 verification (M4)
- `spacecraft-cli-shell` and `spacecraft-cli-preference` — any shell command you emit

## Claude Code specifics

- Plan mode is the norm for a new milestone: read the PRD (the Claude Doc "Fairing —
  Product Requirements Document"), `PLAN.md` and `TODO.md` before proposing work.
- End every turn with the §17.1 progress block (`cargo xtask progress`) and a two-line
  `TL;DR:`.
- Prefer `rg` over `grep` and `fd` over `find` when they are installed.
