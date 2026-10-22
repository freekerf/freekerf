# SVG import

`freekerf_import::svg::import(bytes, options)` reads an SVG file into objects grouped
by **colour layer**, as LaserGRBL's colour-layer import does (`SvgColorLayer`,
`GCodeFromSVG`). The planner then writes the job the way LaserGRBL writes it: with
the same layers, settings and file, the G-code is identical (conformance goldens
`svg_*`).

## What is read

- **Elements:** `path` (all commands: `M L H V C S Q T A Z`, absolute and relative),
  `rect` (rounded corners as arcs), `circle` (a `G3` circle), `ellipse`, `line`,
  `polyline`, `polygon`, in groups `g` nested up to 9 levels. `image` is not
  converted: an *unsupported* object (a comment in the G-code). `text` is read as
  editable text when the import has fonts (`SvgOptions::fonts`, FreeKerf's own
  import: [text.md](text.md)), else kept as unsupported like LaserGRBL does (the
  conformance `import-case`).
- **Order:** per container, LaserGRBL's order: rects, circles, ellipses, lines,
  polylines, polygons (texts, images), then paths, then groups.
- **Colour of an element:** its stroke if it has one, otherwise its fill, inherited
  from the groups around it, black by default. Values: named colours (as .NET names
  them, system colours included), `#RGB`, `#RRGGBB`, `rgb()`/`rgba()` with numbers or
  percentages; `none`, gradients (`url(...)`) and `currentColor` do not count. The
  colours found, in order of first appearance, with their element count, are the
  layers offered to the user.
- **Size:** `width`/`height` with units (`mm`, `cm`, `in`, `pt`, `pc`, `px`, `%`) and
  `viewBox` (four numbers separated by spaces); Y is flipped so the drawing is in
  machine coordinates (mm, Y up).
- **Transforms:** `translate`, `scale`, `rotate` (with and without a centre),
  `matrix`, and `skewX`/`skewY`, on groups and elements.
- **Curves:** Béziers are flattened with LaserGRBL's adaptive algorithm (tolerance
  0.2 px), or with 12 equal steps when *smart Bézier* is off; elliptical arcs become
  Béziers first.

## Layers

Each colour gets a layer with a mode (`line`, `fill`, `fill_and_line`, `cut`,
`ignore`), speed, power, passes and a hatch filling; layers run fill first, then
lines, then cuts ([planner.md](planner.md)). Fillings use the even-odd rule: a shape
inside another is a hole (the inside of an "O").

## API and CLI

```rust
let imported = freekerf_import::svg::import(&bytes, &SvgOptions::default())?;
let document = imported.into_document_with_defaults(1000, 1000, Some(source));
```

`freekerf import-case` runs the conformance `svg` cases ([cli.md](cli.md)).

## Limits

- No CSS stylesheets (`<style>`), `use`/`symbol`, clip paths or masks. A text has
  one style (that of its first `tspan`); `dx`/`dy` shifts and rotated letters are
  not read.
- Numbers in path data with an upper-case exponent (`1E3`) are read as LaserGRBL
  reads them (the `E` starts a new, unknown command).
- An arc with a zero radius draws nothing, as in LaserGRBL (DIV-035).
- Curves with control points beyond a kilometre (or not finite) are kept as one
  piece instead of being subdivided to LaserGRBL's limit (DIV-109).

## Divergences

FreeKerf reads compact numbers (`-4.5.5`, DIV-031), skips a `path` without `d`
instead of failing (DIV-032), never loses the current document when an import fails
(DIV-033), reads every `points` pair (DIV-034), applies `skewX`/`skewY` and reads a
transform without its `)` (DIV-036), starts path data without a moveto at its first
point when it is the first element of its colour (DIV-107). See
[divergences](../divergences.md#importers-and-generators).

## Tests

Unit tests in `crates/core/freekerf-import/src/svg/`; `tests/properties.rs` (never
panics); `freekerf-service/tests/import_cases.rs` (conformance cases copied here);
conformance `svg_*` goldens (`make test-rust`).
