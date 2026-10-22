# 0019. Text: fonts, shaping and text that follows a path

- Status: accepted (M5)
- Date: 2026-10-22

## Context

Until now text objects had one font, the ASCII-only Hershey table of LaserGRBL's
test generators. Users need other fonts (installed ones, and single-line fonts
that engrave in one pass), bold, italic, underline, effects for cutting (weld,
stencil), text bent on a circle or along a path, and SVG files whose text stays
text. The core must keep building for wasm and must not do I/O.

## Decision

- A core crate, `freekerf-text`: fonts, shaping, layout, effects, text on a path.
  The Hershey code of the generators moves there (the planner re-exports it).
- **Libraries:** `skrifa` (outlines and metrics, Google Fonts' fontations) and
  `harfrust` (shaping: kerning, ligatures, marks, right-to-left; the HarfBuzz
  project's port) in the core, on the same `read-fonts`, pure Rust, MIT/Apache,
  wasm-ready; `fontdb` (installed fonts through fontconfig's folders, no memory
  maps) in `freekerf-platform`, the native layer. The core gets fonts through a
  `FontSource` trait, so the session takes them from the host like it takes stores.
- **Built-in single-line fonts** as SVG fonts compiled in: Relief SingleLine
  (OFL-1.1, the default for new texts), the Hershey families and the EMS fonts of
  Inkscape's Hershey Text (OFL-1.1 and the Hershey licence). Missing characters
  come from Relief SingleLine.
- **One style per text** (font, bold, italic, decorations, alignment, spacing,
  effects): what laser texts need first; spans can come later.
- **Height is the capitals' height**, measured on `H`, as laser users measure text.
- **Text on a path is a live link** to the shape (`TextPath.object`), with a copy of
  the path kept in the text: after every document change the session lays out again
  the texts whose shape changed, in the same undo step. Moving the text alone puts
  it back on the path; deleting the shape unlinks the text, which keeps the copy.
- **Bold and italic** use the family's faces; when there is none they are drawn
  (single-line: strokes drawn again beside themselves; outlines: grown; italic:
  slanted 12°).
- **`.fkproj` version 4**: a text records its font and style. Texts without `font`
  are in `LaserGRBL Hershey` and keep their strokes exactly.
- **SVG import** reads `text`, `tspan` and `textPath` as editable text in
  FreeKerf's import; the LaserGRBL-compatible import (conformance) still writes the
  "not supported" comment.

## Alternatives

- `ttf-parser` + `rustybuzz`: the first choice, dropped before merging when both
  were declared unmaintained (RUSTSEC-2026-0192, RUSTSEC-2026-0206, which name
  `skrifa` and `harfrust` as their successors).
- `usvg` for SVG text: it lays text out but would replace the LaserGRBL-exact SVG
  converter; only the text part was needed.
- A copied path instead of a live link: simpler, but the user expects the text to
  move with its path, as in other laser software.
