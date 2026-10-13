# Changelog

All notable changes to FreeKerf are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions will follow
[Semantic Versioning](https://semver.org/) from the first release.

## [Unreleased]

### Added

- Workspace in layers (core, config, native, runtime, apps, testing) with
  `cargo xtask check-layers`, a CI that checks formatting, lints, tests with
  coverage thresholds, licenses, layers, generated docs, the wasm build of the
  core, Windows/macOS builds, the MSRV and the conformance suite (milestone M0).
- `freekerf-gcode`: G-code parsing, modal state, arcs, time estimate, ranges and
  quadrant, identical to LaserGRBL on every conformance golden file.
- `freekerf-protocol`: the LaserGRBL host as a sans-IO state machine: buffered,
  synchronous and repeat-on-error streaming with RX buffer accounting, status
  reports, firmware and vendor detection, `$$`/`$I`, settings writes, overrides,
  jog and continuous jog, jobs with passes, abort, STOP and resume, custom code
  with `[expr]`, issue detection, laser life counters; Grbl/grblHAL, Smoothie,
  Marlin and VigoWork.
- `freekerf-emulator`: a Grbl 1.1 device (simulator and test device).
- `freekerf-transport`: serial ports, the emulator in process and on a PTY.
- `freekerf-settings`: `settings.toml` with a JSON schema and migrations.
- `freekerf-session` and `freekerf-service`: the action registry with permission
  tiers; every front end dispatches actions.
- `freekerf-machine`: the I/O-thread driver with a priority STOP channel.
- `freekerf-geometry`: paths of lines, arcs and Bézier curves, Mono-compatible
  affine transforms, LaserGRBL's Bézier flattening and arc fitting, path ordering,
  polygon booleans and offsets (a port of Clipper 6.4.2) and the hatch filling
  patterns, identical to LaserGRBL's.
- `freekerf-document`: the project model (objects on colour layers with cut and
  raster settings) and the versioned `.fkproj` file with a generated JSON schema.
- `freekerf-planner`: documents to G-code the way LaserGRBL writes it (layer
  order, hatch fillings, pen and feed handling), the cutting, power/speed and
  shake test generators and Hershey text; .NET-compatible number formatting in
  `freekerf-gcode`.
- `freekerf-import`: SVG (colour layers, all path commands, shapes, transforms,
  units) and ASCII DXF (lines, true arcs, splines fitted with arcs, blocks,
  colours, units) import, identical to LaserGRBL except for documented fixes.
- `freekerf-raster`: bitmaps (PNG, JPEG, BMP, GIF), LaserGRBL's image operations
  (grayscale formulas, invert, threshold, whitenize, fill, outline, resize), nine
  dithering modes and Line2Line raster G-code, identical to LaserGRBL on the
  conformance goldens (GDI+ steps within the tolerance of ADR 0008).
- Vectorization: Potrace (a port of LaserGRBL's CsPotrace, outlines as biarcs,
  vector and raster fillings) and centerline through the external `autotrace`
  program, identical to LaserGRBL on the conformance goldens (ADR 0009, 0010).
- Images in jobs: the raster dialog's pipeline (edits, resolution, resize, grey,
  white clip, threshold or dithering) and its tools (line by line, dithering,
  pass-through, vectorize, centerline) in `freekerf_raster::processor` and
  `freekerf_planner::image`; `plan` now plans documents with images.
- `freekerf-materials`: material library packages of `freekerf/library` (git
  dependency), search, hazards, recipes applied to layers, LaserGRBL material import.
- `freekerf-migrate` and `freekerf migrate lasergrbl`: LaserGRBL settings and laser
  module counters (a read-only `BinaryFormatter` reader), dry run or `--apply`.
- Integration tests: SVG, DXF and images imported, planned and streamed to the
  emulator over a PTY. Fuzz targets (G-code, SVG, DXF, LaserGRBL files) run weekly
  in CI; the bugs they found are fixed (DIV-109).
- The session (`freekerf-session`): document, selection, view state, undo/redo,
  transactions and change events, with their actions (select, import, move, layers,
  history, view) and the service's file actions (`document.open`, `document.save`,
  `document.import`, `job.from_document`).
- Scripts (`freekerf-script`, ADR 0011): JavaScript (QuickJS) with an `app` API
  generated from the action registry, limits, STOP cancellation, one undo step per
  run with rollback on error, dry runs, laser actions refused without a person to
  confirm them, `api.d.ts`, and `freekerf run <script.js>`.
- Agents and clients: `freekerf-ipc` (one instance per user, Unix socket / named
  pipe, token), `freekerf serve --headless`, permission tiers per caller with grants
  (this run, this session, always), and the MCP server `freekerf mcp` (attach or
  launch, disabled by default; agents save only new `.fkproj` files and never
  run laser actions; tools `run_script`, `get_state`, `screenshot`,
  `search_api`, `stop`; state resources with notifications; ADR 0012).
- LaserGRBL custom buttons (`CustomButtons.bin`, `.zbn` exports) migrated to
  scripts in the macro folder (`freekerf migrate lasergrbl`, `freekerf migrate
  lasergrbl-buttons`).
- `freekerf import-case`: runs `conformance` importer golden cases.
- `freekerf` CLI: `analyze`, `send`, `console`, `emulate --pty`, `host --stdio`,
  `config`, `actions`, `run`.
- Setting `machine.accept_broken_ok` (default on): a garbled line containing `ok`
  counts as an `ok`, as in LaserGRBL; off, only lines starting with `ok`/`error` do.
- Action `machine.safety_door` (tier `safety`): the Grbl 1.1 door command (0x84).
- Documentation: `AGENTS.md`, architecture, ADRs 0001-0005, feature pages,
  divergences from LaserGRBL, coverage, benchmarks, roadmap.
- Desktop app: the dock is resizable in the wide layout: drag its left edge for
  its width and, in Design mode, the separator under Layers for their height
  (keyboard: arrows, Home/End; double-click: default size); kept in
  `ui.dock_width_px` and `ui.dock_layers_pct`.

- Desktop app, Design mode: copy, cut and paste (Ctrl+C, Ctrl+X, Ctrl+V) and a
  context menu on the canvas (copy, cut, paste where it opened, duplicate,
  delete). Actions `document.copy`, `document.cut`, `document.paste`: the
  session's clipboard keeps the objects and their layers across documents (a
  missing layer is made with the copied settings); not an undo step.

- Design mode: join (Ctrl+G, context menu) the selected shapes into one object
  per layer, so an imported drawing moves as one piece; open paths whose ends
  touch become continuous (closed when they come back to their start), so a DXF
  outline of loose lines can be filled and is cut in one move. Split
  (Ctrl+Shift+G) gives one object per path. Actions `document.combine`,
  `document.split`; `freekerf_geometry::join_paths`, `Path::reversed`.

- Layers panel: the hatch pattern (12 patterns) and the lines per mm of fill
  layers (fill density, with the spacing between hatch lines in its tooltip).

- Laser mode per layer: `M3` constant or `M4` dynamic power (Grbl laser mode),
  or the machine's command (default). The planner writes each layer's command
  when the layer begins (images included); the layers panel has an M3 / M4
  choice. Project files are version 3 (version 2 files read unchanged).

### Fixed

- Design mode: a layer set to fill showed only the outlines on the canvas, as if
  it would not be filled; the canvas now draws the hatch lines the job engraves.
- Custom code: `[jogstep]` and `[jogspeed]` are evaluated (the jog settings), as in
  LaserGRBL; they were left unevaluated.
- A startup block report (`>G54G20:ok`) or a well-formed `[...]` message containing
  "ok" no longer acknowledges a pending line (DIV-051).
