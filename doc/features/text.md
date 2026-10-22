# Text

Editable text objects: a font (built-in single-line fonts or the installed
TrueType / OpenType fonts), a style, lines, alignment and spacing, effects, and
text that bends on a circle or follows a path. The text stays editable; the
geometry is laid out again when it changes. Crate `freekerf-text` (core layer),
installed fonts in `freekerf-platform`.

## Fonts

| Kind | Families | Drawn as |
| --- | --- | --- |
| Built-in, single-line | Relief SingleLine (default for new texts), Hershey Sans, Hershey Serif, Hershey Script, Hershey Gothic English, EMS Readability, EMS Nixish, EMS Allure, EMS Tech | open strokes: the laser draws each stroke once (fast engraving) |
| Built-in, LaserGRBL | LaserGRBL Hershey: the font of the test generators; texts made before fonts could be chosen keep it, stroke for stroke | open strokes |
| Installed | every TrueType / OpenType family of the system (fontconfig's folders on Linux, read the first time a font is asked for) | closed outlines (engraved filled, or cut) |
| User folder | `<data dir>/fonts`: TrueType, OpenType and single-line SVG fonts (`<font>` with `<glyph d>`, as Inkscape's Hershey Text writes them) | by kind |

The built-in fonts are SVG fonts compiled into the core
(`crates/core/freekerf-text/fonts`): Relief SingleLine (OFL-1.1), the EMS fonts
(OFL-1.1) and the Hershey fonts (the Hershey licence) of Inkscape's Hershey Text
extension; credits in `NOTICE`. Characters a single-line font lacks (accents of
the Hershey fonts, `é` in LaserGRBL's) are taken from Relief SingleLine, sized to
match.

`fonts.list` gives the families (built-in first); `fonts.preview` a sample as SVG
path data for font pickers.

## A text

`document.add_text` and `document.set_text` take every field; `set_text` changes
only the fields given ([actions.md](actions.md)).

| Field | Meaning |
| --- | --- |
| `text` | the text; `\n` starts a line |
| `height_mm` | height of the capitals (measured on `H`) |
| `font` | the family |
| `bold`, `italic` | the family's bold / italic face; drawn when it has none (single-line: each stroke again 5 % of the height to the right; outlines: grown by 3 %; italic: slanted 12°) |
| `underline`, `strikethrough` | at the font's positions; a stroke for single-line fonts, a band welded with the letters for outline fonts |
| `align` | `left`, `center`, `right` around the origin, or `justify` (lines but the last as wide as the widest; on a path: the text fills it) |
| `line_spacing` | times the font's line height |
| `letter_spacing_mm` | added between letters (negative: tighter) |
| `width_scale` | horizontal scale of the letters |
| `weld` | outline fonts: overlapping letters become one shape (cutting script fonts) |
| `outline_mm` | an outline added this far around the letters (negative: inside); single-line fonts: around the strokes |
| `stencil_mm` | outline fonts: a bridge this wide through the middle of every hole (the island of an O stays held) |
| `arc_radius_mm` | the text bends on a circle of this radius, centred on its top (negative: on its bottom, reading along it) |

Shaping (kerning, ligatures, marks, right-to-left scripts) comes from the fonts'
OpenType tables (`harfrust`, outlines and metrics with `skrifa`); SVG fonts are kerned with their `hkern` pairs.
Effects of outline fonts work on polygons within 0.01 mm; without effects the
letters keep their curves.

## Text on a path

`document.text_on_path {id, path}` puts a text on the first path of a shape. The
text **follows** the shape: after every edit (one undo step with it), a text whose
shape changed is laid out again; moving the text alone puts it back on the path,
moving both moves it once. Where it sits: `path_offset_mm` (start along the path,
with the alignment), `path_distance_mm` (from the path), `path_flip` (the other
side, backwards), all through `document.set_text`. Deleting the shape leaves the
text where it was, with a copy of the path (it can then move on its own);
`document.text_off_path` makes it straight again from where the path started.
Copies (duplicate, array, paste) follow the copy of their shape when it was copied
with them, else nothing.

`document.text_to_paths` turns texts into plain shapes.

## SVG import

FreeKerf's import (`document.import_data`, opening a file) reads `text` as editable
text: lines (`tspan`s that start a line), `x`/`y`, `font-family` (an unknown
family becomes Relief SingleLine), `font-size` (the em; the capitals are sized by
the font's proportions), `font-weight`, `font-style`, `text-decoration`,
`text-anchor`, `letter-spacing`, inherited from groups and `style`; a `textPath`
follows the imported path it names (`startOffset` in length or percent, `side`),
or a copy of a path in `defs`. The LaserGRBL-compatible import (conformance
`import-case`) keeps LaserGRBL's behaviour: a "not supported" comment.

## Projects

`.fkproj` version 4 ([projects.md](projects.md)): a text records its font and
style; a text without `font` (version 3 and older) is in LaserGRBL Hershey.
Geometry is stored with the object, so a project opens anywhere; changing a text
needs its font installed (the inspector names a missing one).

## Desktop app

The inspector (Object panel) of a text: the text (several lines), size, font with
a sample, B I U S, alignment, line and letter spacing, width, weld, outline,
stencil bridges, arc radius; on a path: start, distance, other side, *Take off the
path*; *Convert to paths*. With a text and a shape selected: *Put text on path*. On
the canvas, Shift+Enter starts a new line ([design-mode.md](design-mode.md)).

## Limits

- One style per text (no bold word inside a sentence yet: [roadmap](../roadmap.md)).
- Weld and stencil are for outline fonts; a single-line font has no area.
- Variable fonts use their default instance; colour fonts their outlines.
- A text on a path is laid out with each letter upright on the path at its
  middle (letters do not bend).

## Tests

- `freekerf-text`: SVG path data and SVG fonts (glyphs, ligatures, kerning), every
  built-in font reads and covers Portuguese, styles pick a face or are drawn,
  the LaserGRBL font keeps its strokes, alignment, justify, spacing, decorations,
  outline-font effects on a TrueType font built in memory (`testfont.rs`), text
  on a path and on an arc, following a path, copies.
- `freekerf-import`: SVG texts and `textPath` (`svg/mod.rs`, `svg/text.rs`), a
  property test that any text never panics.
- `freekerf-session`: `text_styles_paths_and_fonts` (actions, following through
  transforms and undo, copies, deletion, fonts).
- `freekerf-platform`: user SVG fonts.
- UI: `design.test.ts` (text inspector), `text-editor.test.ts` (Shift+Enter).
