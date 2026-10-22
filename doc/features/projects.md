# Projects (`.fkproj`) and the document model

A FreeKerf project is a **document**: objects on **colour layers**, each layer with
its cut settings. Importers (SVG, DXF, images) fill it, the session edits it through
actions, and the planner turns it into G-code. The model is
`crates/core/freekerf-document` (core layer: no I/O).

## Model

| Part | What |
| --- | --- |
| `Layer` | a colour (`#RRGGBB`), a name, a **mode** (`line`, `fill`, `fill_and_line`, `cut`, `ignore`), `speed_mm_min`, `power` (the `S` value), `passes`, the hatch `fill` (pattern and lines per mm), `visible`, the `laser_mode` (`machine`: the machine's laser on command, `constant`: `M3`, `dynamic`: `M4`; left out when `machine`) |
| `Object` | an id (never reused), the layer it is on, a name, its source (file format and name), its kind, the `transform` applied since it was made (affine; the geometry already includes it, so planners ignore it: the editor reads the rotation from it) and, for text, its `text` |
| `text` | on a `shape` made by the editor's text tool (or an imported SVG `text`): the text, the capitals' height, the font, the style, alignment, spacing, effects and, for a text on a path, the path it follows ([text.md](text.md)). The letters are laid out at the origin and moved by the object's `transform`, so editing the text keeps its position, rotation and scale; a text on a path is placed by the path |
| `shape` | vector paths in millimetres, Y up: lines, circular arcs (centre and direction, so they become `G2`/`G3`) and cubic Béziers (`freekerf-geometry`) |
| `image` | a raster image (the file as imported, or RGBA pixels), its position and size on the bed, the raster settings of LaserGRBL's dialog (tool, direction, quality, grey adjustments, dithering, vectorize and centerline options, speeds, powers, laser commands) and a list of non-destructive edits (crop, rotate, flip, invert, fill, outline) |
| `unsupported` | an element of an imported file that was not converted (an SVG `text` or `image`, a polygon without commas): kept so the user sees it |

**Execution order** (`Document::execution_order`): fill layers first, then line
layers, then cut layers (a cut can free the piece before it is engraved); user order
inside each group; `ignore` layers are skipped. This is LaserGRBL's colour-layer order.

## File format

- JSON, UTF-8, pretty-printed with a stable field order; binary data (images) is
  base64.
- `"format": "freekerf-project"` and `"version": 4`. Older versions are migrated
  when read; a newer version is refused with a clear error (written by a newer
  FreeKerf).

  | Version | Change |
  | --- | --- |
  | 1 | first version |
  | 2 | objects may carry `transform` and `text` (both optional, left out when unused: a version 1 file reads unchanged) |
  | 3 | layers may carry `laser_mode` (left out when `machine`: a version 2 file reads unchanged; the version keeps an older FreeKerf from planning an `M4` layer with `M3`) |
  | 4 | texts carry `font` and their style (left out when default); a text without `font` is in `LaserGRBL Hershey`, its font until then, so a version 3 file reads unchanged; the version keeps an older FreeKerf from dropping the style when it saves |
- Reading checks the invariants: unique layer and object ids, every object on an
  existing layer, at least one pass per layer.
- Numbers round-trip exactly (shortest representation that reads back to the same
  `f64`).
- JSON schema: [`doc/schema/fkproj.schema.json`](../schema/fkproj.schema.json),
  generated from the Rust model by `cargo xtask gen-docs` and checked in CI
  (`cargo xtask check-docs`).

## API

```rust
let mut doc = Document::new();
let layer = doc.layer_for_color(Color::BLACK, 1000, 500);   // speed, power
doc.add_object(layer, ObjectKind::Shape { paths })?;
let text = doc.to_fkproj();
let back = Document::from_fkproj(text.as_bytes())?;
```

`Object::apply_transform` transforms the geometry and records the transform (images
only move and scale); `rect_path` and `ellipse_path` build the editor's shapes.
Editing actions (draw, write text, transform, align, duplicate, layers, undo/redo)
are in `freekerf-session` ([session.md](session.md), [actions.md](actions.md)).

## Limits

- Transforms are applied to the geometry when made (`transform` only records
  them): arcs stay arcs under rotations, uniform scales and mirrors; other scalings
  turn arcs into lines within 0.01 mm. Images do not rotate yet.
- One style per text ([text.md](text.md)); changing a text needs its font.
- Large images are stored as the original file; projects with many large images
  are large JSON files. A zipped container is an option for later
  ([roadmap](../roadmap.md)).

## Tests

- Unit tests in `crates/core/freekerf-document/src` (layers, objects, ids,
  execution order, version and invariant checks, the version 1 → 2, 2 → 3 and 3 → 4 migrations,
  transforms and shapes, schema).
- `tests/properties.rs`: any generated document survives `to_fkproj` →
  `from_fkproj` unchanged.
