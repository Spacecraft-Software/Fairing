# SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The Fairing theme contract (FRN-SRS-040).
#
# A theme is a Nickel record that satisfies this contract. Fairing applies the
# contract itself (`fairing theme check`, `fairing theme compile`), so a theme
# file does not need to import it; importing it as well is harmless and lets an
# editor check the theme while it is written.
#
# Geometry is in the 1920x1080 reference frame and is scaled to the active mode
# at boot (FRN-SRS-007). Colours are §11.1 roles of the Steelbore palette the
# theme declares, or the name of a token of that palette; never a literal
# colour (FRN-SRS-043, FRN-SRS-044). `fairing theme compile` checks what a
# contract cannot: that a rectangle lies inside the frame, that a token belongs
# to the declared palette and binds a role, that text is not drawn in a surface
# colour, and that the bar's fill has 3:1 contrast with its track.
let NonNegative =
  std.contract.from_validator
    (fun value =>
      if std.is_number value && value >= 0 then
        'Ok
      else
        'Error { message = "expected a number of reference pixels, zero or more" }
    )
in
let Positive =
  std.contract.from_validator
    (fun value =>
      if std.is_number value && value > 0 then
        'Ok
      else
        'Error { message = "expected a number of reference pixels greater than zero" }
    )
in
let Role
  | doc "A Steelbore §11.1 role."
  = [|
    'background,
    'surface,
    'surface_alt,
    'foreground,
    'accent,
    'structure,
    'success,
    'error,
    'warning,
    'focus,
    'border
  |]
in
let Token
  | doc "A named token of the theme's own palette, e.g. `{ token = \"Plasma Orange\" }`."
  = { token | String | doc "The token's name exactly as the Steelbore palette file spells it." }
in
let Color
  | doc "A role (`'accent`) or a token of the declared palette (`{ token = \"Plasma Orange\" }`)."
  = std.contract.any_of [ Role, Token ]
in
let Image
  | doc "Fairing's built-in vector mark, or a PNG file relative to the theme file."
  = [| 'builtin |]
in
let ImageFile = { file | String | doc "A PNG path, relative to the theme file." } in
let Logo = {
  x | NonNegative | doc "Left edge of the box the logo is fitted into.",
  y | NonNegative | doc "Top edge of the logo box.",
  width | Positive | doc "Width of the logo box.",
  height | Positive | doc "Height of the logo box.",
  image
    | std.contract.any_of [ Image, ImageFile ]
    | doc m%"
      `'builtin` for Fairing's vector mark, or `{ file = "logo.png" }`. Every
      opaque pixel of a PNG must be a colour of the declared palette, unless
      `tint` is set.
    "%,
  tint
    | Color
    | optional
    | doc "Draw the PNG's alpha channel alone, in this colour (a mask).",
}
in
let Bar = {
  x | NonNegative | doc "Left edge of the track.",
  y | NonNegative | doc "Top edge of the track.",
  width | Positive | doc "Width of the track.",
  height | Positive | doc "Height of the track.",
  radius | NonNegative | doc "Corner radius; at most half the height.",
  outline | NonNegative | doc "Outline width; 0 for none.",
  fill | Color | doc "The filled part.",
  track | Color | doc "The empty part.",
  border | Color | doc "The outline.",
}
in
let Percent = {
  gap | NonNegative | doc "Gap between the bar's right edge and the percentage (FRN-SRS-053).",
  size | Positive | doc "Text size.",
  color | Color | doc "Text colour; a foreground role, never a surface.",
}
in
let Status = {
  show | Bool | doc "Whether the status line is drawn (FRN-SRS-050)." | default = true,
  top | NonNegative | doc "Top of the status line; it is centred horizontally.",
  size | Positive | doc "Text size.",
  color | Color | doc "Text colour; a foreground role, never a surface.",
}
in
let Prompt = {
  x | NonNegative | doc "Left edge of the password prompt region.",
  y | NonNegative | doc "Top edge of the prompt region.",
  width | Positive | doc "Width of the prompt region.",
  height | Positive | doc "Height of the prompt region.",
  size | Positive | doc "Text size of the message and the input field.",
}
in
let Sequence = {
  x | NonNegative | doc "Left edge of the box frames are fitted into.",
  y | NonNegative | doc "Top edge of the box.",
  width | Positive | doc "Width of the box.",
  height | Positive | doc "Height of the box.",
  fps | Positive | doc "Frames per second; at most 30 (FRN-SRS-051).",
  frames | Array String | doc "PNG paths, relative to the theme file, all the same size.",
  tint | Color | optional | doc "Draw each frame's alpha channel alone, in this colour.",
}
in
{
  schema_version
    | std.contract.Equal 1
    | doc "Version of this contract the theme was written against."
    | default
    = 1,
  name | String | doc "The theme's name, shown in reports.",
  palette
    | String
    | doc m%"
      The registered Steelbore palette slug the theme is drawn in, e.g.
      `steelbore` or `tokyonight` (FRN-SRS-043). It is the theme's declared
      default: an operator may still pick another palette at boot (§11.6).
    "%,
  layouts
    | doc "The complete layout set (FRN-SRS-046)."
    | {
      boot
        | doc "What the splash draws while the system boots."
        | {
          logo | Logo | doc "The logo.",
          bar | Bar | doc "The progress bar.",
          percent | Percent | doc "The percentage beside the bar.",
          status | Status | doc "The status line.",
          prompt | Prompt | doc "Where a password prompt is drawn.",
          sequence | Sequence | optional | doc "A frame sequence played while booting.",
        },
      shutdown
        | doc "What the splash draws while the system shuts down."
        | {
          logo | Logo | doc "The logo.",
          status | Status | doc "The line under the logo.",
          message | String | doc "What the line says, e.g. \"Shutting down\".",
          sequence | Sequence | optional | doc "A frame sequence played while shutting down.",
        },
    },
}
