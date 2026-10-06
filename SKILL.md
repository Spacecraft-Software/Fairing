---
name: fairing
description: >
  Fairing is the boot splash for Steelbore OS Bravais (Rust, GPL-3.0-or-later). Use this
  skill when an agent must drive the `fairing` command line or work in its repository:
  `fairing describe --json` returns the live capability manifest, `fairing schema
  [<command>]` returns JSON Schema Draft 2020-12 for function calling, `fairing preview`
  draws the splash with a simulated bar (off-screen with `--backend memory --snapshot`
  under an agent), and `--version --json` returns the attribution envelope. In the
  repository, `cargo xtask` runs the requirements generator, traceability gate, progress
  block and text-format gate. The splash, shutdown and theme verbs arrive with milestones
  M2–M4 and are not available yet.
license: GPL-3.0-or-later
project: Spacecraft Software
component: Fairing
version: 0.1.0
---

# Fairing — capability surface

## Sub-command tree (today)

- `fairing describe` — capability manifest; safe, idempotent, no side effects
- `fairing schema [<command>]` — JSON Schema Draft 2020-12 for the tool or one command
- `fairing preview [--seconds n] [--backend auto|drm|fbdev|memory] [--snapshot file.ppm]
  [--palette slug] [--status text] [--size WxH] [--fps hz]` — draw the splash with a
  simulated bar, then restore the console; `--dry-run` reports the plan without opening
  a device; `--theme <path>` is refused with `FEATURE_UNAVAILABLE` until M2
- `fairing --version` — name, version, maintainer, website, copyright

Planned (not yet present): `fairing splash`, `fairing shutdown`, `fairing theme check`,
`fairing theme compile`.

## Agents and `preview`

Under `AI_AGENT`, `AGENT`, `CI`, `CLAUDECODE`, `CURSOR_AGENT` or `GEMINI_CLI` no virtual
terminal is opened: `--backend auto` renders to memory and emits a `[WARN]`
(`AGENT_MEMORY_BACKEND`); `--backend drm|fbdev` exits 2. Pair memory rendering with
`--snapshot frame.ppm` (binary PPM, `P6`) to inspect the frame. The JSON `data` carries
`backend`, `width`, `height`, `format`, `palette.{slug,base,source,overlay,skipped}`,
`frames`, `dropped`, `first_frame_ms`, `measured_fps`, `snapshot`, `fallbacks`, `planned`.

## Output formats

`human` (TTY default), `json` (default when piped or under an agent/CI variable),
`jsonl`. `yaml`, `csv` and `explore` are accepted and answered with
`FEATURE_UNAVAILABLE` (exit 1) or, for `explore`, a `TUI_FALLBACK` warning plus JSON.

## Global flags

`--json`, `--format`, `--fields`, `--dry-run`, `--verbose`/`-v`, `--quiet`/`-q`,
`--no-color`, `--color`, `--absolute-time`, `--print0`/`-0`, `--yes`, `--force`,
`--help`, `--version`/`-V`.

## Exit codes

0 success · 1 general failure (`FEATURE_UNAVAILABLE`, `INTERNAL_ERROR`) · 2 usage error
(`INVALID_ARGUMENT`, `MISSING_ARGUMENT`) · 3 `NOT_FOUND` (no DRM device or `/dev/fb0`) ·
4 `PERMISSION_DENIED` (another client is DRM master, or no `video` group) · 5 `CONFLICT`. Every non-zero exit in machine mode writes one line
`{"error":{code,exit_code,message,hint,timestamp,command}}` to stderr; `hint` is a
runnable command.

## Agent environment

`AI_AGENT` or `AGENT` set to any non-empty value → JSON, no colour, no prompts, severity
floor `warn`. `CI` truthy → JSON, no colour, default floor. `CLAUDECODE`, `CURSOR_AGENT`,
`GEMINI_CLI` → informational (`metadata.tool_agent`), colour and prompts off, format
unchanged. `NO_COLOR`, `FORCE_COLOR`, `CLICOLOR=0`, `TERM=dumb` follow the CLI Standard
precedence.

## Examples

```sh
fairing describe --json | jq .data.commands
fairing describe --json --fields tool,version
fairing schema preview
AI_AGENT=my-agent fairing describe        # JSON without asking
fairing preview --backend memory --seconds 0 --snapshot frame.ppm --json
fairing preview --dry-run --json          # plan only: backend chain and palette
```

```nu
fairing describe --json | from json | get data.commands
```

## Repository tasks

```sh
cargo xtask req-texi --check   # generated Texinfo chapters current
cargo xtask trace --json       # traceability matrix as an envelope
cargo xtask progress           # §17.1 progress block
cargo xtask check-eol          # §6.5 text-file gate
```

Live manifest: `fairing describe --json` is authoritative at run time.
