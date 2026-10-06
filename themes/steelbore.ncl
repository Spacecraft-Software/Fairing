# SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The reference Fairing theme (P-021): Steelbore Modern, the family default.
#
# Its accessible siblings need no files of their own. Colours here are roles,
# so `steelbore-high-contrast` (accessible mode) and `steelbore-mono`
# (`NO_COLOR`) are the same theme drawn with another role table (§11.6).
# Geometry is in the 1920x1080 reference frame, and equals Fairing's built-in
# layout, which a test holds to.
{
  name = "steelbore",
  palette = "steelbore",
  layouts = {
    boot = {
      logo = { x = 840, y = 300, width = 240, height = 240, image = 'builtin },
      bar = {
        x = 640,
        y = 640,
        width = 640,
        height = 14,
        radius = 7,
        outline = 1.5,
        fill = 'accent,
        track = 'surface,
        border = 'border,
      },
      percent = { gap = 28, size = 30, color = 'foreground },
      status = { show = true, top = 704, size = 26, color = 'foreground },
      prompt = { x = 560, y = 780, width = 800, height = 140, size = 28 },
    },
    shutdown = {
      logo = { x = 840, y = 300, width = 240, height = 240, image = 'builtin },
      status = { top = 640, size = 26, color = 'foreground },
      message = "Shutting down",
    },
  },
}
