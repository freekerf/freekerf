# Features

One page per feature: what it does, how to use it, configuration, limits, safety,
tests and divergences from LaserGRBL. The action catalogue is generated.

| Page | Content | Since |
| --- | --- | --- |
| [connection.md](connection.md) | transports, connect sequence, status polling, `$$`/`$I`, timeouts | M1 |
| [streaming.md](streaming.md) | buffered / synchronous / repeat-on-error, RX buffer accounting | M1 |
| [job-lifecycle.md](job-lifecycle.md) | run, passes, abort, STOP, resume, issues, progress | M1 |
| [custom-code.md](custom-code.md) | header, footer, passes code, `[expr]`, immediate characters | M1 |
| [jog.md](jog.md) | step jog, jog to position, continuous jog | M1 |
| [overrides.md](overrides.md) | feed, rapid and power overrides | M1 |
| [machine-settings.md](machine-settings.md) | the board's `$$` table, writing settings, Wi-Fi | M1 |
| [firmware-variants.md](firmware-variants.md) | Grbl, grblHAL, Smoothie, Marlin, VigoWork, vendor detection | M1 |
| [gcode-analysis.md](gcode-analysis.md) | parsing, time estimate, ranges, quadrant, `analyze --json` | M1 |
| [laser-life.md](laser-life.md) | laser module life counters | M1 |
| [emulator.md](emulator.md) | the built-in Grbl 1.1 emulator, on a PTY | M1 |
| [settings.md](settings.md) | `settings.toml`, schema, migrations, `freekerf config` | M1 |
| [cli.md](cli.md) | the `freekerf` command line | M1 |
| [host-protocol.md](host-protocol.md) | `freekerf host --stdio` (JSON Lines) | M1 |
| [actions.md](actions.md) | the action registry (generated) | M1 |
| [projects.md](projects.md) | the document model and the `.fkproj` file | M2 |
| [planner.md](planner.md) | document → G-code: layer order, fillings, the G-code writer | M2 |
| [test-patterns.md](test-patterns.md) | cutting, power/speed and shake tests, Hershey text | M2 |
| [svg-import.md](svg-import.md) | SVG files: colour layers, shapes, transforms, curves | M2 |
| [dxf-import.md](dxf-import.md) | DXF files: lines, true arcs, splines, blocks, colours | M2 |
| [raster.md](raster.md) | bitmaps: image operations, dithering, Line2Line | M2 |
| [vectorize.md](vectorize.md) | Potrace vectorize (outlines, fillings) and centerline (autotrace) | M2 |
| [session.md](session.md) | the session: document, selection, view, undo, transactions, events | M2 |
| [mcp.md](mcp.md) | AI agents: `freekerf mcp`, tools, resources, tiers, safety | M2 |
| [ipc.md](ipc.md) | the local endpoint: single instance, token, protocol | M2 |
| [scripting.md](scripting.md) | JavaScript scripts: the generated `app` API, transactions, dry run, limits, `freekerf run` | M2 |
| [materials.md](materials.md) | the material library: packages, search, recipes on layers | M2 |
| [migration.md](migration.md) | LaserGRBL migration: settings, laser counters, materials | M2 |
| [shell.md](shell.md) | app shell: view state, layouts, STOP, dialogs, keyboard, dispatch-only | M4a |
| [fire-confirmation.md](fire-confirmation.md) | the human confirmation before any laser action | M4a |
| [produce-mode.md](produce-mode.md) | machine panel, jog, frame, overrides, console, recovery | M4a |
| [i18n.md](i18n.md) | Fluent translations, language setting, LaserGRBL translations (`resx2ftl`) | M4a |
| [theming.md](theming.md) | themes, high contrast, density, units | M4a |
| [design-mode.md](design-mode.md) | Design mode on the document: layers, palette, inspector, tools, handles, import, welcome | M4a, M4b |
| [text.md](text.md) | text objects: fonts (single-line and installed), style, alignment, effects, text on a path, SVG text | M5 |
| [canvas.md](canvas.md) | the canvas: renderer, gestures, G-code preview, head, performance | M4a |
| [desktop-app.md](desktop-app.md) | the Tauri app, its boundary with the service, generated types, design system | M4a |
