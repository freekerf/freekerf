# FreeKerf: monorepo plan

FreeKerf is free software (**GPL-3.0-or-later**) for designing jobs and driving laser machines (Grbl, grblHAL, Smoothie, Marlin…). It is the successor of LaserGRBL (© 2016 Diego Settimi), rewritten in Rust, cross-platform (Linux, Windows, macOS; tablets later), and designed for mouse, keyboard **and** touch.

> **Status: M0 and M1 done.** This document defines **what to build, in which order, and under which rules** (architecture, tests, documentation). The maintenance guide is [`AGENTS.md`](AGENTS.md) and this file tracks progress in §13 *Where each item lives*.

Related repositories in the [`freekerf`](https://github.com/freekerf) organization:

| Repository | Role for this monorepo |
| --- | --- |
| [`conformance`](https://github.com/freekerf/conformance) | Executable specification of the current LaserGRBL core (Python). Its *protocol* and *golden* layers run against the Rust build with `LASERGRBL_HOST=rust`. |
| [`library`](https://github.com/freekerf/library) | Materials and machine profiles. Consumed by `freekerf-materials` as a git dependency on the `freekerf-library` crate (model + `read_archive`). |
| [`design`](https://github.com/freekerf/design) | UX research, brand, design system (`fk-` tokens), clickable prototype and the UI toolkit analysis (`tech-notes.md`). |
| [`website`](https://github.com/freekerf/website) | Public site and user documentation. |

---

## 1. Principles

1. **Layers are separate and machine-checked.** Business logic knows nothing about I/O, the operating system or the UI (§3).
2. **Sans-IO protocol.** The Grbl state machine takes bytes, lines and time, and returns bytes to send and events. It is tested without threads, real clocks or serial ports.
3. **One service API for every front end.** The CLI, the desktop app and the remote (tablet) client all use the same command/event API (`freekerf-service`).
4. **Observable behaviour is checked by the `conformance` suite.** LaserGRBL bugs do not have to be reproduced (§8), but every difference is documented.
5. **Operator safety over convenience.** STOP is always available, nothing fires the laser without confirmation, and machine states are explicit (§11).
6. **Code, tests and documentation ship together.** A change is not done until its tests and docs are updated in the same PR (§6, §7).
7. **Every action is a registered command.** Anything a user can do (edit the document, select, open a panel, jog, run a job) is an *action* in one registry, with a generated JSON schema. The web UI, the CLI, scripts and AI agents (through MCP) all go through the same `dispatch(action)`; the UI has no private shortcuts (§9).

## 2. Repository layout

```
freekerf/
├── AGENTS.md                  maintenance guide for people and agents (M0)
├── CLAUDE.md                  contains only "@AGENTS.md"
├── PLAN.md                    this file
├── Cargo.toml                 virtual workspace (members per layer)
├── deny.toml                  license and source policy (already configured)
├── rust-toolchain.toml        pinned toolchain + MSRV
├── rustfmt.toml               max_width = 110 (same as `library`)
├── crates/
│   ├── core/                  ── CORE: pure logic, no I/O
│   │   ├── freekerf-gcode       G-code parser/model, modal state, arcs, analysis (ranges, quadrant), time estimate
│   │   ├── freekerf-geometry    paths, curves, flattening, booleans, offset, transforms
│   │   ├── freekerf-raster      image operations, 12+ dithers, line-to-line, vectorization preparation
│   │   ├── freekerf-document    project model: objects, colour layers, cut settings, .fkproj format
│   │   ├── freekerf-import      SVG / DXF / image / G-code → document (reads bytes, not files)
│   │   ├── freekerf-planner     document → job: ordering, path optimization, generators (material test, Hershey), G-code
│   │   ├── freekerf-protocol    sans-IO state machine: Grbl/grblHAL, Smoothie, Marlin, VigoWork
│   │   ├── freekerf-emulator    model of a Grbl 1.1 device (tests and simulator mode)
│   │   └── freekerf-session     app state as data: document + view state (mode, panels, selection, viewport, dialogs),
│   │                            action registry and dispatcher, undo/redo, state snapshots and change events
│   ├── config/                ── CONFIG: persistent data and schemas
│   │   ├── freekerf-settings    app and machine settings in TOML (serde, versioned schema, migrations)
│   │   ├── freekerf-materials   material library access (wraps `library`'s `freekerf-library`)
│   │   └── freekerf-migrate     LaserGRBL import: Settings.bin, .zbn buttons, materials, usage counters
│   ├── native/                ── NATIVE: everything that depends on the OS or hardware
│   │   ├── freekerf-transport   Transport trait + serial, Telnet, WebSocket (ESP8266), emulator; Android USB later
│   │   ├── freekerf-discovery   machine discovery on the network (mDNS/scan)
│   │   ├── freekerf-ipc         local single-instance endpoint (Unix socket / Windows named pipe) + auth token
│   │   └── freekerf-platform    directories, atomic writes, sounds, notifications, OS info
│   ├── runtime/               ── RUNTIME: orchestration
│   │   ├── freekerf-machine     driver: protocol + transport + clock on an I/O thread; job lifecycle
│   │   ├── freekerf-service     application facade: session + machine behind one action registry; events/snapshots;
│   │   │                        the API used by every front end
│   │   ├── freekerf-script      embedded JavaScript engine; API generated from the action registry (§9.2)
│   │   ├── freekerf-mcp         MCP server (official `rmcp` SDK): fixed tool set (run_script, get_state…), state → resources
│   │   └── freekerf-remote      WebSocket server for the remote (tablet) client — M6
│   └── testing/               ── TEST SUPPORT (dev-dependencies only, never shipped)
│       └── freekerf-testkit     fixtures, fake devices, simulated Clock, PTY helpers, golden/snapshot helpers
├── ui/                        ── UI (not Rust)
│   ├── web/                     front end (TypeScript) of the desktop/tablet app, grown from `design`'s prototype
│   ├── tokens/                  `fk-` tokens generated from `design/tokens.json`
│   └── i18n/                    Fluent (.ftl) translations shared by Rust and web
├── apps/                      ── APPS: final binaries (thin shells)
│   ├── cli/                     `freekerf` binary (headless), including `freekerf mcp` (attach-or-launch) and `freekerf run <script.js>` (§9)
│   └── desktop/                 Tauri 2 app (`src-tauri` = crate `freekerf-desktop`)
├── clients/python/           external Python client over `freekerf-ipc` (§9.1)
├── tests/                     cross-crate integration tests (workspace member `freekerf-it`)
├── tools/xtask/               automation: check-layers, resx2ftl, sync-tokens, release
└── doc/                       documentation (§7)
```

Crate names are always `freekerf-<name>`; every crate inherits `license = "GPL-3.0-or-later"` from `[workspace.package]`.

## 3. Layer dependency rules

| Layer | May depend on | Must not |
| --- | --- | --- |
| **core** | other core crates and pure libraries | `std::fs`, `std::net`, threads, the system clock, OS/serial/UI crates. Must build for `wasm32-unknown-unknown`. |
| **config** | core | native, runtime, apps (file I/O arrives through a trait / `platform`) |
| **native** | core | config, runtime, apps |
| **runtime** | core, config, native | apps, UI |
| **apps** | runtime (through `freekerf-service`), config, ui | `protocol`/`transport` directly |
| **testing** | anything | being a normal (non-dev) dependency of any crate |
| **clients/** (Python…) | only the `freekerf-ipc` protocol and the generated API | importing Rust crates or duplicating app logic |
| **ui/web** | only the API exposed by `freekerf-service` (Tauri IPC or remote WebSocket) | its own machine or G-code logic; changing app state other than through `dispatch(action)` (purely visual state such as hover or animation stays local) |

Automatic checks:
- `cargo xtask check-layers` reads `cargo metadata` and fails on any edge that breaks the table. It runs in CI.
- `cargo build -p <core crate> --target wasm32-unknown-unknown` in CI proves the core does no I/O.
- `unsafe_code = "forbid"` through workspace lints, except in `freekerf-transport` and `freekerf-platform` where each `unsafe` block has a documented reason.

## 4. Licensing (configured in `deny.toml`)

- Project: **GPL-3.0-or-later**; credit in `NOTICE` ("Derivado do LaserGRBL, Copyright (c) 2016 Diego Settimi"). Porting the C# code directly is allowed.
- `cargo deny check` runs in CI. `deny.toml` is an **allow-list**: GPL-2.0-only, AGPL, SSPL, BUSL, EPL, CDDL, NC/proprietary licenses and unlicensed crates are rejected. Third-party crates come only from crates.io; the `library` git dependency is the only allowed git source (add it to `allow-git` in M2).
- Potrace (GPL-2.0-or-later) is accepted: the combined work is GPL-3.0.
- Avoid `GPL-3.0-only` dependencies (they would pin the project to v3).
- Assets (fonts, icons) follow `design/CREDITS.md`.

## 5. Engineering practices

SOLID, applied to this codebase (same spirit as `library/AGENTS.md`):

- **S — single responsibility.** One crate per concern (§2); inside a crate, one module per concept. Apps only wire the service to a front end: no business logic in `apps/`.
- **O — open/closed.** New firmware = new `impl Firmware` in `freekerf-protocol`; new transport = new `impl Transport`; new importer = new `impl Importer`; new streaming mode = new `impl StreamingStrategy`. Registered in one place; existing implementations are not edited to fit a new case.
- **L — Liskov.** Any `Transport`, `Firmware`, `Importer` or `Clock` can replace another without surprises: no panics on valid input, errors as values.
- **I — interface segregation.** Small traits (one essential method). Do not add "optional" methods to an existing trait; add a new trait.
- **D — dependency inversion.** The protocol, machine and service receive `Clock`, `Transport`, `SettingsStore` and the like by injection; tests use the doubles from `freekerf-testkit`.

Other rules:
- Libraries return errors (`thiserror`); `anyhow` only in apps. No `unwrap`/`expect`/`panic!` on production paths.
- Logging with `tracing`; no `println!` in libraries.
- Deterministic outputs: stable ordering, reproducible G-code for the same input and settings.
- Units in names where ambiguous (`speed_mm_min`, `power_pct`), as in `library`.
- No telemetry (project decision, same as `library`).

## 6. Testing

New code comes with tests. CI fails when a gate below fails.

| Level | Where | What it proves | Tooling |
| --- | --- | --- | --- |
| **Unit** | next to the code (`#[cfg(test)] mod tests`) | one function/type in isolation; protocol driven step by step with a simulated `Clock` | `cargo test`, `freekerf-testkit` |
| **Property** | `tests/` of the crate | the parser never panics; parse → format → parse is stable; the RX buffer never exceeds its limit; geometry invariants | `proptest` |
| **Snapshot / golden** | `tests/` of the crate | G-code and analysis output for fixed inputs; reviewed diffs | `insta` |
| **Crate integration** | `crates/**/tests/*.rs` | the public API of one crate end to end (e.g. `freekerf-machine` streaming a file to `freekerf-emulator` through a PTY) | `cargo test`, PTY helpers |
| **Cross-crate integration** | `tests/` (`freekerf-it`) | real compositions: service + machine + transport + emulator; settings migration; import → plan → stream | `cargo test` |
| **CLI end to end** | `apps/cli/tests/` | the `freekerf` binary: `analyze --json`, `send`, `host --stdio`, `emulate --pty`, exit codes and errors | `assert_cmd`, `predicates` |
| **Conformance** | external repo `conformance` | observable behaviour vs. LaserGRBL (protocol + golden; importers from M2) with `LASERGRBL_HOST=rust` | `make test-rust` in CI (checkout of `conformance`) |
| **MCP integration** | `crates/runtime/freekerf-mcp/tests/`, `tests/` | an MCP client (`rmcp` client) against a headless app + emulator: fixed tool set, `run_script` reaching every registry namespace, sandbox and limits, resources and change notifications, permission tiers per call, disabled-by-default, attach vs. launch (§9.6) | `cargo test`, `rmcp` client |
| **Fuzzing** | `fuzz/` | G-code, SVG, DXF and host-protocol parsers (from M2; scheduled CI job) | `cargo-fuzz` |
| **UI end to end** | `ui/web/tests/`, `apps/desktop/tests/` | the 8 flows of `design/ux.md` against the emulator (from M4) | Playwright + Tauri WebDriver |
| **Benchmarks** | `benches/` | load/analyze/plan large files; results in `doc/benchmarks.md` | `criterion` |

Rules:
- **Coverage** with `cargo llvm-cov`: CI fails below **90 % lines for the workspace** (same bar as `library`), and below **95 %** for `freekerf-gcode` and `freekerf-protocol`. Excluded code is listed in `doc/coverage.md` with a reason.
- **No real hardware** in automated tests (§11). Serial tests use PTYs, the emulator or the `conformance` fake Grbl.
- **No sleeps** in tests: waits on conditions with timeouts, or the simulated `Clock`. A flaky test is a bug: fix it or quarantine it with an issue link, never retry blindly.
- **Test data** lives in `crates/<crate>/tests/fixtures/` or in `freekerf-testkit`; large generated inputs are created at test time.

## 7. Documentation

All documentation is in **English**: this file, `README.md`, `AGENTS.md`, everything under `doc/`, rustdoc, commit messages and PR descriptions. Product UI text goes through i18n (`ui/i18n/`), with at least `en` and `pt-BR`.

Structure (created in M0, maintained afterwards):

| File | Content |
| --- | --- |
| `AGENTS.md` | Maintenance guide for people and agents: overview, layout, commands, architecture on one screen, SOLID as applied here, invariant rules (safety, layers, licenses), the living-documentation table, testing rules, conventions. `CLAUDE.md` contains only `@AGENTS.md`. |
| `doc/architecture.md` | Layers, crates, data flow (Mermaid diagrams), extension points table ("I want to… → do…"), key decisions. |
| `doc/features/` | **One page per feature** (e.g. `streaming.md`, `jog.md`, `overrides.md`, `firmware-variants.md`, `gcode-analysis.md`, `svg-import.md`, `material-library.md`, `host-protocol.md`), plus `README.md` as the index. Each page: purpose, user-visible behaviour, API/CLI entry points, configuration, limits, safety notes, tests that cover it, related divergences. |
| `doc/adr/NNNN-title.md` | Architecture decision records (threads vs. tokio, numeric representation, injectable `Clock`, UI toolkit, settings format…). |
| `doc/divergences.md` | Every intentional difference from LaserGRBL (§8). |
| `doc/coverage.md`, `doc/benchmarks.md` | Current numbers and exclusions. |
| `doc/roadmap.md` | What was postponed and open decisions. |
| `CHANGELOG.md` | Keep a Changelog format, from the first release. |

**Living documentation:** every behaviour change updates the docs **in the same PR**. `AGENTS.md` keeps this table up to date:

| Changed… | Update |
| --- | --- |
| a public API (any crate) | rustdoc (`missing_docs` is on), the feature page |
| protocol / streaming / firmware behaviour | `doc/features/<feature>.md`, `doc/divergences.md` if it differs from LaserGRBL |
| the `host --stdio` protocol | `doc/features/host-protocol.md` and `conformance`'s `RustHost` |
| an action (added, renamed, new parameter, new permission tier) | regenerate `doc/features/actions.md` and `api.d.ts` (both checked in CI); `doc/features/scripting.md` / `mcp.md` if behaviour changes |
| settings format | `doc/features/settings.md`, a migration and its test |
| crates, traits, data flow, layers | `doc/architecture.md`, `AGENTS.md`, `xtask check-layers` |
| a decision | a new ADR |
| something postponed | `doc/roadmap.md` |
| a milestone item | §13 of this file |

Generated documentation (JSON schemas of settings and the host protocol, CLI `--help` captured in docs) is checked in CI: committed == generated.

## 8. Divergences from LaserGRBL

- LaserGRBL bugs listed in `conformance/FINDINGS.md` **do not have to be reproduced**. FreeKerf may behave differently, as long as it is **documented**.
- Every relevant FINDINGS item gets an entry `DIV-NNN` in `doc/divergences.md`: *kept* / *fixed* / *not applicable*, with the reason and the affected test.
- When a `conformance` test fails because of an intentional divergence, mark it **for the Rust host only** (`rust_divergence("DIV-NNN")` marker in `conformance`). Never delete the test or loosen the assertion for the C# host.

## 9. Automation: scripting and AI agents (MCP)

FreeKerf can be automated by **scripts** (user macros, batch jobs) and driven by **AI agents** through the [Model Context Protocol](https://modelcontextprotocol.io). An agent can read everything the app knows (including what is on screen) and trigger **every action a user can trigger in the UI**: select objects, open or close panels, edit the document, import files, connect, jog, plan and run jobs. **MCP is disabled by default.**

Agents do not get one MCP tool per action. They get a **small fixed set of tools**, and the main one runs a **JavaScript script** against an API generated from the action registry (the same approach as FreeCAD or Blender, but sandboxed). This keeps the agent's context small (no hundreds of tool definitions), lets one call do what would take dozens of round trips (loops, conditions, batch edits), and gives agents and users the same power.

### 9.1 Where it lives

| Piece | Crate / layer | Responsibility |
| --- | --- | --- |
| Action registry | `freekerf-session` (core) + `freekerf-service` (runtime) | Every action is a typed command with a name (`view.open_panel`, `document.select`, `machine.jog`…), a JSON schema (schemars), a description, a **permission tier** and undo support where it applies. The session registers document/view actions; the service registers machine actions into the same registry. **The registry is the only way to change app state**, for the UI, the CLI, scripts and agents. |
| App state as data | `freekerf-session` | Document, selection, mode (Design/Produce), open panels, viewport, dialogs, toasts and the machine snapshot are Rust state. The web UI renders it and only changes it through `dispatch(action)`. |
| Script engine | `freekerf-script` (runtime) | Embedded JavaScript (§9.2). Its API (`app.view.openPanel(...)`, `app.document.select(...)`, `app.machine.jog(...)`) is **generated from the registry**: every call is a `dispatch`. Shared by MCP, user macros and `freekerf run <script.js>`. |
| Single instance + local endpoint | `freekerf-ipc` (native) | One running instance per user. A Unix domain socket (`$XDG_RUNTIME_DIR/freekerf/ipc.sock`, mode `0600`) or a Windows named pipe restricted to the current user; a per-run token in a user-only file. Used by `freekerf mcp`, by the Python client, by "open file in the running app", and by tests. |
| MCP server | `freekerf-mcp` (runtime) | Exposes the fixed tool set of §9.3 and app state as MCP **resources** (`freekerf://state/view`, `…/document`, `…/machine`, `…/console`, `…/screenshot`, `…/api.d.ts`), with change notifications through resource subscriptions. No logic of its own. |
| Entry point | `apps/cli` (`freekerf mcp`) | stdio MCP server that an agent configures as a command. **Attach or launch**, see §9.4. |
| Python client | `clients/python` (`freekerf` package on PyPI later) | For people who prefer Python: an external library that talks to the running app over `freekerf-ipc`, with an API generated from the registry. It runs **outside** the app with the user's own permissions, so it needs no sandbox; tiers still apply on the app side. |
| Screen capture | `apps/desktop` (+ `freekerf-platform`) | A screenshot of the app window as an MCP image resource. The **semantic snapshot** (`freekerf://state/view`) is the primary way to "see the screen"; the screenshot is a complement. |

`freekerf mcp`, the Python client and the `host --stdio` test protocol are all thin adapters over `freekerf-service`; none adds behaviour.

### 9.2 Script engine

- **Language: JavaScript, embedded.** QuickJS through `rquickjs` (MIT) is the default choice; Boa (pure Rust, MIT/Unlicense) is the alternative. Decide in an ADR in M2 with a small benchmark (startup time, a 10,000-object batch edit, memory). CPython is **not** embedded: it cannot be sandboxed, and shipping it per platform (including tablets) is heavy. Python users get the external client instead.
- **Sandbox by construction.** The script sees only the generated `app` API and a few pure helpers (math, geometry, `console.log` to the app console). No file system, network, processes, timers that outlive the script, or raw serial/G-code access. File import goes through `document.import` actions, which apply their own rules.
- **Limits.** Wall-clock timeout, memory cap and an instruction-count interrupt, all configurable. STOP **cancels any running script** before stopping the machine.
- **Transactions.** A script's document and view changes form **one undo step**. On an uncaught error, the changes made so far are rolled back (machine actions cannot be rolled back and are never retried automatically).
- **Dry run.** `dry_run: true` executes against a copy of the session and returns the list of actions that would be dispatched, without applying anything. Machine and laser actions are always simulated in a dry run.
- **Types and discovery.** A TypeScript declaration file (`api.d.ts`) is generated from the registry (names, parameters, docs, tiers). It is published as an MCP resource, used for editor autocomplete in the macro editor, and checked in CI (committed == generated).
- **User macros.** The same engine powers user scripts in the app (macro editor, buttons that run a script, `freekerf run`). LaserGRBL custom buttons and their `[expr]` expressions are migrated to scripts by `freekerf-migrate` where possible.

### 9.3 MCP tool surface

| Tool | Tier | Why it exists |
| --- | --- | --- |
| `run_script(code, dry_run?)` | per call inside the script | The main path: full access to the app through the registry |
| `get_state(path?)` | `read` | Reading the screen is the most frequent need; it must not require writing code |
| `screenshot()` | `read` | Image of the app window (desktop only) |
| `search_api(query)` | `read` | Find actions and their signatures without loading the whole `api.d.ts` |
| `stop()` | always allowed | **STOP never depends on a script**: it must work even while a script is stuck |

New actions never add MCP tools: they appear in the script API, in `search_api` and in `api.d.ts` automatically. MCP *prompts* are not needed for now (§14).

### 9.4 Attach or launch

```mermaid
sequenceDiagram
  participant A as AI agent (MCP client)
  participant C as freekerf mcp (stdio)
  participant I as freekerf-ipc endpoint
  participant App as FreeKerf (desktop or headless)
  A->>C: start process, MCP initialize
  C->>I: connect to the running instance?
  alt app already running
    I-->>C: connected (token OK, MCP enabled)
  else not running
    C->>App: launch (desktop; --headless when no display or when requested)
    App->>I: listen
    C->>I: connect (bounded retry)
  end
  C-->>A: tools + resources (proxied to the app)
  A->>C: tools/call run_script { app.view.openPanel("layers"); ... }
  C->>App: run in sandbox, each call = dispatch(action) with tier check
  App-->>A: result + action log + resource change notification
```

- **Disabled by default.** Setting `automation.mcp.enabled = false`. When it is off, `freekerf mcp` exits with a clear error telling the user where to enable it; a running app refuses MCP connections on the IPC endpoint. The setting is changed **by the user** in the app settings or with `freekerf config set`; it is **not** an action available to scripts or MCP, and it is not overridable by a launch flag.
- If the app is running, the agent attaches to it and sees the same session the user sees. If not, `freekerf mcp` starts it (desktop when a display is available, otherwise headless) and attaches.
- Optional second transport (off by default): MCP Streamable HTTP on `127.0.0.1` only, with the token and an `Origin` check (DNS-rebinding protection, as the MCP spec requires). Never bound to other interfaces.

### 9.5 Permission tiers and laser safety

Each action declares one tier. **Tiers are checked on every `dispatch`, including each call made inside a script**, so a script cannot do more than its caller is allowed to. Settings choose the baseline (highest tier allowed without asking) per caller kind (agent via MCP, user macro, Python client); above it, the human can grant more (see *Permission grants* below):

| Tier | Examples | Default for agents when MCP is enabled |
| --- | --- | --- |
| `read` | read state, document, console, screenshot | allowed |
| `ui` | select, open/close panels, change mode, zoom/pan, open dialogs | allowed |
| `edit` | change the document, import files, change layer settings, save | allowed |
| `machine` | connect, home, jog, unlock, `$$` changes, overrides | **off** (`automation.mcp.allow_machine`) |
| `laser` | frame with laser on, test fire, run/resume a job | **off** (`automation.mcp.allow_laser`) |

- **STOP / feed hold are always allowed** (they only make things safer), and STOP cancels running scripts first.
- A `laser` action from an agent or a script never skips the fire confirmation of the UI: the job goes to *awaiting confirmation* and a **human confirms on the machine's screen**. In headless mode with no human present, `laser` actions are refused. Dropping the human confirmation is out of scope.
- The app shows a persistent **"Agent connected"** indicator with the client name, a one-click disconnect, a **running-script indicator with a cancel button**, and an **action log** (who did what, when, from which script) in the console.
- Materials flagged `block` by `library` stay blocked for scripts and agents too.

**Permission grants.** The tiers above are the baseline from settings. When a caller (a user macro, an agent through MCP, the Python client) needs a tier above its baseline, the call pauses and the app asks the human, naming the caller, the tier and the action. The human chooses the **scope** of the grant:

| Scope | Lasts | Stored |
| --- | --- | --- |
| **This run** | until the current script / MCP call finishes | nowhere |
| **This session** | until the app is closed | in memory |
| **Always** | until revoked in settings | in `freekerf-settings`, per caller |

- Callers are identified by kind and name; a user macro also by a hash of its code, so **editing a macro invalidates its grants**. Agent names come from the MCP client and can be spoofed, so "always" grants to agents show that warning in the dialog.
- Denying (or closing the dialog) fails the call with a permission error that the script or agent can read; nothing is retried automatically.
- Grants are listed in the automation settings page and in the action log, with one-click revoke. Changing grants is a user-only setting, never an action available to scripts or MCP.
- With no human present (headless), there is no dialog: only baseline tiers and **always** grants apply; anything else is refused.
- A grant never removes the human fire confirmation of §9.5 for `laser` actions, whatever its scope.

### 9.6 Tests and docs

- Unit: registry invariants (unique names, every action has a schema, a description and a tier); generated script API == registry; `api.d.ts` committed == generated.
- Script engine: sandbox escape attempts (no `fs`, network, `eval` of host objects, prototype tampering), timeout, memory cap and instruction interrupt, STOP cancels a running script, one undo step per script, rollback on error, dry run applies nothing, tier denial inside a script.
- MCP integration (§6): the fixed tool set; `run_script` reaching every registry namespace against a headless app + emulator; resources and notifications; tiers enforced per call; disabled-by-default; attach to a running instance vs. launch; stale socket and wrong-token cases.
- Permission grants: each scope (this run / this session / always) expires when it should; editing a macro invalidates its grants; revoke works immediately; denial returns a readable error; headless refuses anything needing a prompt; no grant skips the laser fire confirmation.
- Python client: integration tests against a headless app over `freekerf-ipc`.
- UI end to end (from M4): a test agent runs a script that opens a panel, selects an object and changes a layer, and Playwright checks the UI reflects it; and the opposite (a UI click shows up in `get_state`).
- **No UI-only actions:** a lint test fails if the web UI mutates app state outside `dispatch`, and the generated `doc/features/actions.md` must match the registry.
- Docs: `doc/features/mcp.md` (setup for common agents, tools, tiers, safety, attach/launch), `doc/features/scripting.md` (engine, sandbox, limits, transactions, macros, examples), `doc/features/actions.md` (generated catalogue), `clients/python/README.md`, and ADRs for the action registry, the script engine and the MCP transport/security model.

## 10. Milestones

Each milestone has objective acceptance criteria, **including tests and docs**. Do not start a milestone until the previous one is green in CI.

### M0 — Bootstrap (1–2 weeks)
- Virtual workspace, `rust-toolchain.toml` (current stable + pinned MSRV), `rustfmt.toml`, workspace lints (`missing_docs`, `unsafe_code = "forbid"`, selected `clippy::pedantic`).
- `freekerf-testkit` skeleton; `tools/xtask` with `check-layers`.
- CI (skeleton in `.github/workflows/ci.yml`): fmt, clippy `-D warnings`, tests with `cargo llvm-cov` and the thresholds of §6, `cargo deny check`, `check-layers`, wasm build of the core, `cargo check` for Windows (`x86_64-pc-windows-gnu`) and macOS (`aarch64-apple-darwin`). Tests run on Linux.
- Docs: `AGENTS.md`, `CLAUDE.md`, `doc/architecture.md`, `doc/features/README.md`, `doc/roadmap.md`, ADRs 0001–0004 (threads + `crossbeam-channel` vs. `tokio`; numeric representation — LaserGRBL uses `decimal`, so `rust_decimal` where wire formatting requires it; injectable `Clock`; the action registry of §9, which the script API and MCP are generated from).

**Acceptance:** CI green with one minimal crate per layer; a test PR that breaks a layer rule fails `check-layers`; docs skeleton in place.

### M1 — Headless machine core (6–10 weeks)
Crates: `gcode`, `protocol`, `emulator`, `transport` (serial + emulator), `machine`, `settings`, `service`, `testkit`, `apps/cli`.

- Functional parity with the "core" tier of `conformance/SCOPE.md`: streaming modes, RX buffer accounting, `ok`/`error:N`/`ALARM`/`<status>`/`[MSG]`, `$$`/`$I`, real-time commands, overrides, jog and continuous jog, job lifecycle, resume, custom code with `[expr]`, firmware detection, laser life counters, emulator.
- `freekerf` CLI:
  - `analyze <file.nc> --json`: the same JSON as `conformance/src/lasergrbl_harness/golden.py`;
  - `send`, `console`;
  - `emulate --pty`: exposes the emulator on a PTY (also useful to test the old LaserGRBL);
  - `host --stdio`: JSON Lines protocol (`doc/features/host-protocol.md`) implementing `conformance`'s `HostAdapter`.
- In `conformance`: a PR adding `RustHost`, the Rust analyzer, the `rust_divergence` marker and a `make test-rust` target that needs neither Mono nor `LASERGRBL_REPO`.
- Large files: streaming load and parallel analysis (`rayon`); benchmark with a synthetic ~1M-line raster.
- `freekerf-service` is built on the action registry from day one: every machine operation is a registered action with a schema and a permission tier (§9.1), even before MCP exists.

**Acceptance:**
- `conformance` *protocol* and *golden (G-code)* layers green with `LASERGRBL_HOST=rust`, 3 consecutive runs without flakiness (or the divergence recorded, §8);
- coverage thresholds of §6; property tests for the parser and the buffer accounting; integration tests streaming through a PTY to the emulator; CLI end-to-end tests;
- feature pages for every M1 feature, `doc/divergences.md`, `doc/benchmarks.md` (Rust vs. C#).

### M2 — Importers and planner (6–10 weeks)
Crates: `geometry`, `raster`, `document`, `import`, `planner`, `materials`, `migrate`.

- SVG (colour layers, fill/cut, per-layer settings), DXF (true arcs, splines → arcs within a tolerance), images (adjustments, dithers, line-to-line), vectorization (Potrace and centerline), generators (power × speed grid, shade test) and Hershey fonts.
- `freekerf-materials` on top of `library`'s `freekerf-library` (`read_archive` of a release package).
- `freekerf-migrate`: LaserGRBL Settings.bin (BinaryFormatter, read-only), `.zbn` buttons, materials and counters.
- Versioned `.fkproj` project format with a JSON schema generated from the model (same approach as `library`).
- **Automation, headless (§9):** `freekerf-session` (document actions), `freekerf-script` (engine ADR, sandbox, limits, transactions, dry run, `api.d.ts`), `freekerf run`, `freekerf-ipc`, `freekerf-mcp` and `freekerf mcp` with attach-or-launch of a **headless** instance; tiers `read`, `edit`, `machine`; `laser` refused headless; disabled by default; Python client.

**Acceptance:** `conformance` *golden (importers)* green with `LASERGRBL_HOST=rust`; `platform_dependent` (GDI+) cases compared with a documented tolerance. Script engine and MCP integration tests of §9.6 green for the headless scope; `doc/features/scripting.md`, `doc/features/mcp.md`, the generated `doc/features/actions.md` and `api.d.ts`. Fuzz targets for SVG/DXF/G-code running in a scheduled CI job. Integration test import → plan → stream to the emulator. Feature pages for every importer and generator.

### M3 — UI toolkit decision (done)
The code spike was replaced by desk research ([`doc/research/ui-toolkit.md`](doc/research/ui-toolkit.md)). **Decision: Tauri 2** ([ADR 0006](doc/adr/0006-ui-toolkit.md)). The first goal is a **desktop app**; tablets stay at M6. Multi-touch problems found during implementation are recorded in `doc/roadmap.md` and handled later; they do not block milestones. Bulk data crosses the IPC as raw payloads (`tauri::ipc::Response`, raw channels), never JSON arrays.

### M4 — Desktop MVP (10–16 weeks)
- `ui/web` grown from `design/prototypes/app`: Design/Produce modes, canvas (pan, zoom, pinch, selection, handles), colour layers, inspector, machine panel, console, fire confirmation, job recovery, welcome flow and LaserGRBL migration.
- All **MVP** items of the matrix in `design/research.md` §6.
- i18n: `cargo xtask resx2ftl` converts the LaserGRBL `.resx` translations that still apply.
- Automatic density by input type; light, dark and high-contrast themes from the `fk-` tokens.
- **Automation in the desktop app (§9):** all view actions (mode, panels, selection, viewport, dialogs) in the registry; the UI dispatches only through it; attach-or-launch of the desktop app; semantic view snapshot + window screenshot; "Agent connected" and running-script indicators, action log, cancel and disconnect; `laser` tier with human confirmation on screen; automation settings page (off by default); macro editor with `api.d.ts` autocomplete; LaserGRBL custom buttons migrated to scripts.

**Acceptance:** canvas performance check on Linux (20,000 paths + 8000×6000 image, ≥ 50 fps pan with mouse; results in `doc/benchmarks.md`); the 8 flows of `design/ux.md` pass as UI end-to-end tests against the emulator in CI on Linux, and the same flows can be driven end to end by a test MCP client through `run_script`; the no-UI-only-actions lint passes; front-end unit tests (component logic) with coverage reported; user-facing docs drafted for `website`.

### M5 — v1 and distribution (8–12 weeks)
- **v1** items of the matrix: node editing, booleans, offset, array, trace, material library UI, material test, `$$` editor, Smoothie/Marlin in the UI, Telnet/WebSocket + discovery, notifications.
- Packaging:
  - Linux: `.deb`, AppImage, Flatpak;
  - Windows: signed MSI;
  - macOS: signed and notarized DMG;
  - no auto-update (§14): new versions reach users through the installers and package managers; listed in `doc/roadmap.md`.
- CI matrix on real runners (Linux, Windows, macOS): unit + integration tests on all three; UI E2E on Linux and Windows.

**Acceptance:** installers built by the release workflow; smoke test of each installer in CI; `CHANGELOG.md`; user docs published on `website`.

### M6 — Tablet and remote control (later)
`freekerf-remote` (LAN WebSocket, heartbeat for continuous jog, STOP < 50 ms) and the tablet client (Tauri mobile or web served by the core). Android with direct USB host; iPad only as a remote client.

**Acceptance:** latency tests (jog step < 100 ms, STOP < 50 ms) in an integration test over loopback and on a real LAN (manual, documented); the heartbeat stops continuous jog when the client disappears (integration test).

## 11. Safety

- No automated test opens real serial devices (`/dev/ttyUSB*`, `/dev/ttyACM*`, `/dev/ttyS*`, `COMx`). Use PTYs, the emulator or `conformance`'s fake Grbl. Real-machine tests are manual and documented in the PR.
- STOP (soft reset + laser off) bypasses every queue and works in every state; covered by unit and integration tests.
- No command that turns the laser on is sent without an explicit user action. Continuous jog by touch requires a heartbeat (dead-man switch).
- Materials flagged `block` by `library`'s hazard list cannot be used in a job; `warn` shows an alert.
- AI agents (MCP) are off by default, limited by permission tiers, and can never fire the laser without a human confirming on screen (§9.5).

## 12. How to start

1. Read `conformance/README.md`, `SCOPE.md` and `FINDINGS.md`; `design/README.md`, `ux.md` and `tech-notes.md`; and `library/AGENTS.md` as the reference for documentation and testing style.
2. Do M0 and open a PR with the skeleton.
3. M1 in small PRs, one crate at a time: `gcode` → `analyze --json` + golden → `protocol` → `transport`/`machine` → `host --stdio` + `RustHost` → variants, emulator, settings. Each PR brings its tests and its feature page.

## 13. Where each item lives

Updated by every PR that delivers a milestone item.

| Item | Implementation | Tests | Documentation |
| --- | --- | --- | --- |
| M0: workspace, toolchain, lints | `Cargo.toml`, `rust-toolchain.toml`, `rustfmt.toml`, `clippy.toml` | CI `lint`, `msrv` | `AGENTS.md` |
| M0: layer rules | `tools/xtask/src/layers.rs` | `layers.rs` unit tests (a broken rule fails), CI `lint` | `AGENTS.md`, `doc/architecture.md` |
| M0: CI | `.github/workflows/ci.yml` | — | `AGENTS.md` |
| M0: test kit | `crates/testing/freekerf-testkit` (`SimClock`, `Bench`) | used by the protocol tests | `AGENTS.md` |
| M0: docs and ADRs | `AGENTS.md`, `CLAUDE.md`, `doc/` | `cargo xtask check-docs` | `doc/adr/0001`-`0005` |
| M1: G-code parser, state, analysis | `crates/core/freekerf-gcode` | `tests/golden.rs`, `tests/properties.rs`, unit tests | `doc/features/gcode-analysis.md` |
| M1: protocol host (streaming, responses, status, `$$`/`$I`, overrides, jog, jobs, resume, custom code, life counters) | `crates/core/freekerf-protocol` | `tests/protocol.rs` (conformance scenarios), `tests/host_behaviour.rs`, `tests/buffer_properties.rs`, unit tests | `doc/features/{connection,streaming,job-lifecycle,custom-code,jog,overrides,machine-settings,laser-life}.md` |
| M1: firmware variants | `crates/core/freekerf-protocol/src/firmware.rs` | `host_behaviour.rs` (banners, Smoothie, Marlin, Vigo) | `doc/features/firmware-variants.md` |
| M1: emulator | `crates/core/freekerf-emulator`, `freekerf-transport` (in process, PTY) | unit tests, transport tests | `doc/features/emulator.md` |
| M1: transports | `crates/native/freekerf-transport` | `tests/serial_pty.rs`, unit tests | `doc/features/connection.md` |
| M1: machine driver | `crates/runtime/freekerf-machine` | `tests/driver.rs` | `doc/architecture.md`, ADR 0001 |
| M1: settings | `crates/config/freekerf-settings`, `doc/schema/settings.schema.json` | unit tests, `tests/it/settings_files.rs` | `doc/features/settings.md`, ADR 0005 |
| M1: action registry and service | `crates/core/freekerf-session`, `crates/runtime/freekerf-service` | unit tests, `freekerf-service/tests/*` | `doc/features/actions.md` (generated), ADR 0004 |
| M1: CLI (`analyze --json`, `send`, `console`, `emulate --pty`, `host --stdio`) | `apps/cli` | `apps/cli/tests/cli.rs` | `doc/features/{cli,host-protocol}.md` |
| M1: conformance `RustHost`, Rust analyzer, `rust_divergence`, `make test-rust` | `freekerf/conformance` (`lasergrbl_harness/host.py`, `golden.py`, `Makefile`; freekerf/conformance#2) | `make test-rust`: 32 passed, 3 runs in a row; CI job `conformance` | `doc/features/host-protocol.md`, conformance `README.md` |
| M1: large files | `freekerf-gcode` (`rayon`, feature `parallel`) | `benches/analysis.rs` | `doc/benchmarks.md` |
| M1: divergences | — | conformance (no marker needed so far) | `doc/divergences.md` |
| M3: UI toolkit decision (Tauri 2) | — | — | `doc/research/ui-toolkit.md`, `doc/adr/0006-ui-toolkit.md` |

## 14. Decisions already taken

- **Settings format: TOML** (readable in diffs, consistent with `library`). The schema is generated from the Rust model and checked in CI; the web UI reads and writes settings only through actions, never the file.
- **`conformance` is public**, so CI in this repository checks it out without a token.
- **No auto-update for now.** Releases ship as installers and through package managers (M5); the app does not check for or download updates. Revisit later through `doc/roadmap.md`.
- **UI toolkit: Tauri 2** (ADR 0006). Desktop first; gesture problems are deferred, not blocking; tablets at M6.
- **No MCP prompts for now.** The MCP server exposes the tools of §9.3 and the state resources; MCP *prompts* (e.g. "prepare a cut job from this SVG") and example scripts as resources are not needed now. Listed in `doc/roadmap.md` to revisit later.

## 15. Open questions

- MCP: should `allow_laser` exist at all, or should agents be limited to `machine` forever? (current plan: exists, off by default, and always needs a human confirmation on screen)
