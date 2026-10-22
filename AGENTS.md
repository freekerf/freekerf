# FreeKerf: maintenance guide

For people and coding agents working on this repository. `CLAUDE.md` only includes
this file. The plan (what to build, milestones) is [`PLAN.md`](PLAN.md); this
guide is how to work on what exists.

## Overview

FreeKerf designs jobs and drives laser machines (Grbl, grblHAL, Smoothie, Marlin,
VigoWork). It is the GPL-3.0-or-later successor of LaserGRBL, in Rust. Done so far:
M0 (workspace, CI, docs), M1 (headless machine core and the `freekerf` CLI), M2 (document,
importers, planner, session, scripts, MCP), M4a (desktop app shell in Tauri 2 + Svelte 5),
M4b (the Design mode on the real document, automation in the app) and fonts and
text effects for text objects.
The behaviour of LaserGRBL's core is specified by
[`freekerf/conformance`](https://github.com/freekerf/conformance); FreeKerf passes
its protocol and golden (G-code) layers, and its importer goldens as the importers
land (`make test-rust` there; `freekerf import-case`).

## Layout

```
crates/core/       pure logic, no I/O, builds for wasm32
  freekerf-gcode       commands, modal state, arcs, analysis, time estimate
  freekerf-geometry    paths, curves, transforms, Clipper 6.4.2 port, hatch fillings
  freekerf-document    project model (layers, objects, cut/raster settings), .fkproj
  freekerf-import      SVG / DXF bytes -> document (colour layers)
  freekerf-raster      bitmaps: image operations, dithering, Line2Line, Potrace vectorize
  freekerf-text        text: built-in single-line fonts, TrueType/OpenType (skrifa, harfrust),
                       layout, styles, text on a path, effects; Hershey text of the generators
  freekerf-planner     document -> G-code (LaserGRBL writer, images), generators, Hershey
  freekerf-protocol    sans-IO host state machine (port of GrblCore) + firmware variants
  freekerf-emulator    sans-IO Grbl 1.1 device (simulator, tests)
  freekerf-session     action registry (names, JSON schemas, tiers); the session: document,
                       selection, view, undo/transactions, change events, their actions
crates/config/     persistent data
  freekerf-settings    settings.toml model, migrations, JSON schema
  freekerf-materials   material library packages (freekerf/library by git), recipes on layers
  freekerf-migrate     LaserGRBL import, read only: BinaryFormatter reader, settings, laser counters
crates/native/     operating system and hardware
  freekerf-transport   Transport trait: serial, emulator, emulator on a PTY
  freekerf-platform    directories, atomic writes, running external programs, installed fonts (fontdb)
  freekerf-ipc         single instance, local endpoint (Unix socket / named pipe), token, JSON Lines
crates/runtime/    orchestration
  freekerf-machine     I/O thread: host + link + clock, priority STOP
  freekerf-script      embedded JavaScript (QuickJS): the registry as a sandboxed API, limits, api.d.ts
  freekerf-service     the facade: every action of every front end; script runs (transactions, dry run),
                       permissions and grants, the local endpoint server (remote)
  freekerf-mcp         MCP server (fixed tools, state resources) proxied to the app over freekerf-ipc
crates/testing/
  freekerf-testkit     simulated clock, host+device bench, fixtures (dev-dependency only)
apps/cli/          the `freekerf` binary (thin shell over the service)
apps/desktop/      the Tauri 2 app (`src-tauri` = crate freekerf-desktop, thin shell over the service)
ui/web/            web front end (Svelte 5 + TypeScript + Vite): src/ipc (Tauri), src/state,
                   src/gen (generated: api.ts, icons.ts), src/dev (fake service, never shipped),
                   src/shell, src/produce, src/design (Design mode on the session's document actions)
ui/tokens/         fk-tokens.css generated from freekerf/design (commit in design.lock)
ui/i18n/           Fluent translations: <locale>/freekerf.ftl (en is the reference),
                   <locale>/lasergrbl.ftl generated from LaserGRBL (lasergrbl.toml)
packaging/         Linux packages: linux/ (desktop entry template, AppStream metainfo, screenshots),
                   flatpak/ (manifest, offline cargo/npm sources, update-sources.sh, check-sources.py)
tests/             cross-crate integration tests (crate freekerf-it)
fuzz/              cargo-fuzz targets (own workspace, nightly; doc/fuzzing.md)
tools/xtask/       cargo xtask check-layers | gen-docs | gen-ts | check-docs | sync-tokens | resx2ftl
doc/               documentation (below)
```

## Commands

Toolchains come from [mise](https://mise.jdx.dev): `mise.toml` pins Rust (same as
`rust-toolchain.toml`) and Node (same as `NODE` in CI). `mise install` once; without
mise activated in the shell, prefix commands with `mise exec --`.

```bash
cargo test --workspace                       # all tests (no hardware, no network)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo xtask check-layers                     # layer rules
cargo xtask gen-docs                         # regenerate doc/features/actions.md, doc/schema/*, doc/scripting/api.d.ts, ui/web/src/gen/api.ts
cargo xtask check-docs                       # CI: generated docs and TypeScript types are committed
cargo xtask sync-tokens                      # tokens, icons, fonts from ../design at ui/tokens/design.lock
cargo xtask resx2ftl --lasergrbl <checkout>  # LaserGRBL translations that apply → ui/i18n/*/lasergrbl.ftl
cargo llvm-cov --workspace --exclude xtask --exclude freekerf-testkit --exclude freekerf-desktop --summary-only
cargo bench -p freekerf-gcode                # 1M-line analysis (doc/benchmarks.md)
cargo build --target wasm32-unknown-unknown -p freekerf-gcode -p freekerf-protocol -p freekerf-emulator -p freekerf-session -p freekerf-geometry -p freekerf-document -p freekerf-planner -p freekerf-import -p freekerf-raster -p freekerf-text
cargo deny check
cd fuzz && cargo +nightly fuzz run svg       # fuzz targets: gcode, svg, dxf, nrbf (doc/fuzzing.md)

cargo run -p freekerf-cli -- analyze file.nc --json
cargo run -p freekerf-cli -- send file.nc --emulator
cargo run -p freekerf-cli -- emulate --pty   # prints a /dev/pts path any program can open
```

Desktop app (Linux needs WebKitGTK 4.1: `libwebkit2gtk-4.1-dev`, see
`.github/actions/tauri-deps`; `freekerf-desktop` needs Rust ≥ 1.90):

```bash
cd ui/web && npm ci                          # once
npm run dev                                  # Vite on :5173 (dev.html = UI on the fake service, in a browser)
cargo run -p freekerf-desktop                # the app, loading the Vite dev server
npm run build && cargo run -p freekerf-desktop --features custom-protocol   # embedded front end
npm run lint && npm run check && npm test    # ESLint (architecture rules), svelte-check, Vitest
npm run coverage                             # Vitest + V8 coverage (report in ui/web/coverage)
npm run test:web                             # Playwright in Chromium on dev.html (PW_CHROMIUM=<path> to reuse a browser)
xvfb-run -a npm run test:e2e                 # the app end to end on the emulator (tauri-driver; after build + custom-protocol build)
npm run format                               # Prettier
npm run check:bundle                         # after build: the fake service is not in dist/
npm run licenses                             # npm license allow-list (GPL-3.0 compatible)
```

To check a UI flow by hand, `npm run dev` and open http://localhost:5173/dev.html:
the real UI on the fake service (`src/dev/main.ts`), which is exposed as
`window.fkFake` in the browser console. Reproduce the flow there with real clicks and
keys before changing it: the end-to-end tests (`test:e2e`) usually only run on CI.

**CI and branches** ([ADR 0018](doc/adr/0018-release-train.md)): pull requests go
to `main`, which runs the quick Linux checks (`ci.yml`). The Windows and macOS
runners and the app end to end (`native.yml`) run every night on `main`; the
installers (`release.yml`) run on pull requests from `main` to `stable`, the
release train, and a push to `stable` with a new version tags it and drafts the
release. A change to those jobs or to `packaging/` is tried with a manual run of
the workflow on its branch.

**Windows**: `.gitattributes` keeps every text file LF in the checkout, whatever
`core.autocrlf` says (binaries are listed there as `binary`). A CRLF checkout makes
`cargo xtask check-docs` report every generated file as stale and Prettier flag
unchanged `.ts`, `.svelte` and `.css` files. A clone made before `.gitattributes`
existed is converted once, with no local changes, by `git rm --cached -r -q .` then
`git reset --hard`. A new binary extension goes in `.gitattributes`.

Conformance (in a checkout of `freekerf/conformance`):
`FREEKERF_BIN=/path/to/target/debug/freekerf make test-rust`.

## Architecture on one screen

```
 front ends:  CLI · host --stdio · desktop (Tauri: dispatch/catalog/subscribe) · (scripts, MCP later)
                   │ dispatch(action, json)          ▲ ServiceEvent
 runtime:     freekerf-service ── Registry<Inner> (tier check on every call)
                   │ Machine::call(|host, now| ...)   ▲ MachineEvent
              freekerf-machine ── I/O thread: select(commands, rx bytes, deadline); STOP channel first
                   │ bytes                             ▲ bytes
 native:      freekerf-transport ── serial (serialport) · emulator thread · PTY
 core:        freekerf-protocol::Host (sans-IO): receive(bytes, now) / tick(now) → Output + Event
              freekerf-gcode (Program, MachineState) · freekerf-emulator (Device) · freekerf-session (Registry)
```

Time is always injected (`now` in milliseconds, `Clock` trait): the protocol is
tested step by step with a simulated clock (`freekerf_testkit::Bench`). Details:
[`doc/architecture.md`](doc/architecture.md).

## SOLID, as applied here

- **S**: one crate per concern; apps only wire the service to a front end.
- **O**: new firmware = new `impl Firmware` (`freekerf-protocol/src/firmware.rs`);
  new transport = new `impl Transport`; new action = one `register` call in
  `freekerf-service`. Existing implementations are not edited for a new case.
- **L**: any `Transport`, `Firmware` or `Clock` can replace another: no panics on
  valid input, errors as values.
- **I**: small traits (`Transport::open`, `LinkReader::read`, `LinkWriter::write_all`,
  `Clock::now_ms`, `SettingsStore::load/save`). Add a trait rather than optional methods.
- **D**: the host takes time as a parameter; the machine takes a `Clock`; the
  service takes stores. Tests use `SimClock`, `Bench`, `MemoryStore`.

## Invariant rules

**Safety** (PLAN §11)
- No automated test opens a real serial device. Use the emulator, a PTY
  (`freekerf_transport::pty::serve_emulator`) or the conformance fake Grbl.
- STOP (`machine.stop`, soft reset) is tier `safety`, always allowed, and goes
  through the machine's priority channel; it also works while connecting.
- Anything that can turn the laser on is tier `laser` (`job.run`, `job.resume`,
  `machine.send`, `machine.custom_code`).
- `automation.*` settings are user-only: actions (and later scripts and agents)
  cannot change them.

**Layers** (checked by `cargo xtask check-layers`)

| Layer | May depend on |
| --- | --- |
| core | core, pure libraries (no serial, nix, libc, directories, channels, tokio) |
| config | core |
| native | core |
| runtime | core, config, native |
| apps | runtime (`freekerf-service`), config |
| ui/web | only the `AppService` contract (`src/service.ts`); Tauri only in `src/ipc/`; `src/dev/` only in tests and `dev.html`; app state read-only outside `src/state/`, changed only by `dispatch` (ESLint `freekerf/no-app-state-mutation`) |
| testing | anything, but only as a dev-dependency of others |

The core builds for `wasm32-unknown-unknown` (CI). `unsafe_code` is forbidden in
the whole workspace (no crate needs it today).

**Licenses**: GPL-3.0-or-later; `deny.toml` is an allow-list (`cargo deny check`);
npm packages pass `ui/web/scripts/check-licenses.mjs` (`npm run licenses`).
Porting LaserGRBL code is allowed (GPLv3, credit in `NOTICE`).

**Errors**: libraries return errors (`thiserror`), `anyhow` only in apps; no
`unwrap`/`expect`/`panic!` outside tests (clippy enforces it).

## Living documentation

A change is done when its tests and docs are updated in the same PR.

| Changed… | Update |
| --- | --- |
| a public API (any crate) | rustdoc (`missing_docs` is on), the feature page |
| protocol / streaming / firmware behaviour | `doc/features/<feature>.md`, `doc/divergences.md` if it differs from LaserGRBL |
| the `host --stdio` protocol | `doc/features/host-protocol.md` and conformance's `RustHost` |
| an action (added, renamed, new parameter, new tier) | `cargo xtask gen-docs` (`doc/features/actions.md`, checked in CI) |
| settings format | `doc/features/settings.md`, a migration in `freekerf-settings` and its test, `cargo xtask gen-docs` |
| project (`.fkproj`) format | `doc/features/projects.md`, bump `freekerf_document::VERSION` with a migration in `from_fkproj` and its test, `cargo xtask gen-docs` |
| crates, traits, data flow, layers | `doc/architecture.md`, this file, `tools/xtask/src/layers.rs` |
| a type crossing to the UI (result, event, settings) | `cargo xtask gen-ts` (`ui/web/src/gen/api.ts`, checked in CI) |
| an action the UI uses | the fake service (`ui/web/src/dev/fake-service.ts`, `fake-document.ts`): `dev.html` and the UI tests run on it |
| a new action of tier `laser` | the guard lists in `ui/web/src/produce/fire.test.ts` ("knows the laser actions from the generated catalogue") and `crates/runtime/freekerf-service/tests/service.rs` (`laser_actions_run_only_after_a_confirmed_countdown`) |
| visible UI behaviour (what opens, where the focus goes, shortcuts) | search `ui/web/tests/e2e/specs` for the old behaviour (when text stopped opening the Object panel, flow 5 of `design-flows.e2e.ts` broke) |
| `Cargo.lock` or `ui/web/package-lock.json` | `packaging/flatpak/update-sources.sh` (the Flatpak's offline sources; `check-sources.py` there fails in CI when they are stale) |
| packaging (`tauri.conf.json` bundle, `packaging/`, a smoke script) | `doc/features/desktop-app.md` (Releases) |
| the design commit (tokens, icons, fonts) | `ui/tokens/design.lock`, `cargo xtask sync-tokens` |
| a decision | a new ADR in `doc/adr/` |
| something postponed | `doc/roadmap.md` |
| a milestone item | §13 of `PLAN.md` |

## Testing rules

- Unit tests next to the code; integration tests in `crates/<crate>/tests/`;
  cross-crate in `tests/it/`; CLI end to end in `apps/cli/tests/`.
- Protocol behaviour: drive `freekerf_testkit::Bench` (host + emulator in memory,
  simulated clock). Never sleep for time to pass: advance the simulated clock.
  Tests on real threads wait on conditions with a timeout.
- Golden files (`crates/core/freekerf-gcode/tests/fixtures/`) come from
  `conformance`; when they change there, copy them again.
- Property tests (`proptest`) for parsers and invariants.
- Coverage gates: 90 % lines for the workspace, 95 % for `freekerf-gcode` and
  `freekerf-protocol` ([`doc/coverage.md`](doc/coverage.md)).
- A flaky test is a bug: fix it, never retry blindly.

## Divergences from LaserGRBL

LaserGRBL bugs listed in `conformance/FINDINGS.md` do not have to be reproduced;
each one has an entry `DIV-NNN` (the number of the finding) in
[`doc/divergences.md`](doc/divergences.md): kept, fixed or not applicable. Code
comments cite the id. When a conformance test fails because of an intentional
divergence, mark it `@pytest.mark.rust_divergence("DIV-NNN")` in `conformance`;
never delete it or loosen it for the C# host.

## Conventions

- Crates are `freekerf-<name>`; all inherit `license = "GPL-3.0-or-later"`.
- Units in names where ambiguous: `speed_mm_min`, `step_mm`, `*_ms`.
- Deterministic outputs: the same input and settings give the same G-code and report.
- Logging with `tracing`; no `println!` in libraries.
- No telemetry.
- Documentation, commit messages and PR descriptions in English; UI text goes
  through i18n: `t('message-id')` with the id in `ui/i18n/en/freekerf.ftl` (a test
  checks every id used in the code is there) and in the complete locales `pt-BR` and
  `de` (`COMPLETE` in `ui/web/src/i18n/i18n.test.ts` checks they have every English
  id); the other locales may miss ids and fall back to English.
- Svelte 5: in a handler that changes state and then uses values derived from it,
  read those values first. A `{@const}` or `$derived` of state that was just cleared
  is already `null` (the context menu read `menu.at` after `menu = null`).
