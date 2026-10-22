# Roadmap

What was postponed, and open decisions. Milestones are in [PLAN.md](../PLAN.md) §10.

## Postponed from M0/M1

| Item | Why | When |
| --- | --- | --- |
| Telnet and WebSocket transports, network discovery | M5 scope in the plan | M5 |
| Fuzzing (`cargo-fuzz` targets for G-code) | planned with the importers' parsers | M2 |
| The permission dialog (`Prompter`), "agent connected" and running-script indicators in the desktop app (the fire confirmation is done: M4a) | need the desktop UI; headless refuses permission requests today | M4b |
| Sounds, notifications | UI concerns (hotkeys and the safety countdown are done in M4a) | M5 |
| Head interpolation between status reports along the tool path | status reports arrive a few times per second; the head jumps between them | M4b |
| Thick lines in the canvas (selection emphasis, kerf width) | WebGL lines are 1 px; needs quads in the vertex shader | M4b |
| Drop a file on the window to open it | Tauri intercepts drops; done with the import flow | M4b |
| Material picker in the layer editor, z-order, node editing, trace | need `freekerf-materials` and more of the geometry | M4b |
| Macro editor with `api.d.ts` completion, disconnecting one agent from the window | the editor needs a code-editor component (licence, bundle size); agents detach when their MCP client ends | M5 |
| Material library in the Design mode ("Apply material…" on a layer) and "save as material" from a material test cell | no library package is published yet, and user materials need a store in the data folder | v1 (with the library packages) |
| Gestures: no hover cursor feedback over handles; the rotation handle sits above the box and can leave the view on small selections near the top edge; trackpad pinch on WebKitGTK arrives as Ctrl+wheel only | found while building the Design mode | M4b |
| App end-to-end tests on Windows (WebView2 driver) and macOS (no WKWebView driver for Tauri) | Linux runs them in CI; PLAN §6 asks Linux and Windows | M5 |
| Coverage of the browser-only canvas code (Playwright/E2E instrumentation) and a frontend coverage gate | Vitest in jsdom cannot run WebGL2 | M4b |
| WebGPU renderer | not in WebKitGTK yet; same interface as the WebGL2 renderer (ADR 0014) | later |
| Tauri dependencies flagged unmaintained (gtk-rs 0.18 macros, `unic-*` via `urlpattern`) | ignored in `deny.toml` (no vulnerability); upstream Tauri must move | when Tauri updates |

## Postponed from M2

| Item | Why | When |
| --- | --- | --- |
| Image rotation and skew in the document (objects record their transform since M4b, but images only move and scale) | the raster pipeline takes axis-aligned images | M5 |
| Mixed styles inside one text (a bold word in a sentence: text spans) | one style per text covers the laser cases first ([text.md](features/text.md)) | v1 |
| Letters that bend with a path (warped, not only turned), envelope distortion | upright letters on the path are what laser texts need most | v1 |
| SVG text: `dx`/`dy`/`rotate` per letter, several styles per text | rare in laser files; one style per text for now | v1 |
| Bundling `autotrace` with the desktop app (built per platform from a pinned commit, source published with each release) | needs the installers (Windows and Linux ones exist since M5: `release.yml`); M2 runs it from the `PATH` (ADR 0010) | M5 |
| Signed Windows installers (PLAN M5: signed MSI) | needs a code-signing certificate (for example SignPath Foundation for open source) kept as a CI secret; until then Windows SmartScreen warns on first run | M5 |
| macOS disk image signed with a Developer ID and notarized (`release.yml` builds a universal DMG signed ad hoc since M5, so Gatekeeper asks the user to allow it once) | needs an Apple Developer ID (the Apple Developer Program) kept as CI secrets: the certificate (`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`) and notarization (`APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`), which the Tauri CLI reads; then drop `signingIdentity: "-"` | M5 |
| Flathub submission of `io.github.freekerf.FreeKerf` | the Flatpak builds and is smoke-tested in `release.yml`; publishing needs a maintainer: a PR to `flathub/flathub` (branch `new-pr`) with the manifest pointing at a release tag (`type: git`, `tag`, `commit` instead of `type: dir`) plus `cargo-sources.json` and `node-sources.json`, and the `freekerf` GitHub organisation verified on Flathub (ADR 0016) | after the first tagged release |
| The CLI in the AppImage needs `--appimage-extract` to run (`freekerf mcp`) | the AppImage runs only its main binary; a multi-call entry (`freekerf-desktop mcp`) would avoid extracting | when AppImage users ask |
| Keeping imported LaserGRBL user materials; downloading library packages | come with the material library UI | v1 |
| MCP: Streamable HTTP transport (`127.0.0.1`, token, `Origin` check), screenshots | optional transport; screenshots need the desktop window | M4/M5 |
| Zipped project container (`.fkproj` with images as separate entries) | JSON with base64 images is enough until projects with many large images appear | when needed |

## Decided

- UI toolkit: Tauri 2, desktop first (ADR 0006). Multi-touch problems found on Linux are listed here when they appear and handled later.

- No auto-update for now (PLAN §14).
- No MCP prompts for now (PLAN §14).
- Agents never run `laser` actions; there is no `allow_laser` (2026-10-10).
- No Python client: scripts are JavaScript in the app, `freekerf run` covers the
  command line, and the local endpoint protocol (`doc/features/ipc.md`) stays open
  for an external client if one is ever needed (2026-10-10).

## Open

None right now.
