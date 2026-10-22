# FreeKerf: monorepo plan

FreeKerf is free software (**GPL-3.0-or-later**) for designing jobs and driving laser machines (Grbl, grblHAL, Smoothie, Marlin…). It is the successor of LaserGRBL (© 2016 Diego Settimi), rewritten in Rust, cross-platform (Linux, Windows, macOS; tablets later), and designed for mouse, keyboard **and** touch.

> **Status: M0, M1 and M3 done; M2 and M4a in progress.** This document defines **what to build, in which order, and under which rules** (architecture, tests, documentation). The maintenance guide is [`AGENTS.md`](AGENTS.md) and this file tracks progress in §13 *Where each item lives*.

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
| **UI end to end** | `ui/web/tests/`, `apps/desktop/tests/` | the 8 flows of `design/ux.md` against the emulator (Produce flows from M4a, all from M4b) | WebdriverIO + `tauri-driver` for the real app (Playwright cannot drive WebKitWebDriver); Playwright for `ui/web` in a browser against the fake service |
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
| Single instance + local endpoint | `freekerf-ipc` (native) | One running instance per user. A Unix domain socket (`$XDG_RUNTIME_DIR/freekerf/ipc.sock`, mode `0600`) or a Windows named pipe restricted to the current user; a per-run token in a user-only file. Used by `freekerf mcp`, by "open file in the running app", and by tests. |
| MCP server | `freekerf-mcp` (runtime) | Exposes the fixed tool set of §9.3 and app state as MCP **resources** (`freekerf://state/view`, `…/document`, `…/machine`, `…/console`, `…/screenshot`, `…/api.d.ts`), with change notifications through resource subscriptions. No logic of its own. |
| Entry point | `apps/cli` (`freekerf mcp`) | stdio MCP server that an agent configures as a command. **Attach or launch**, see §9.4. |
| Screen capture | `apps/desktop` (+ `freekerf-platform`) | A screenshot of the app window as an MCP image resource. The **semantic snapshot** (`freekerf://state/view`) is the primary way to "see the screen"; the screenshot is a complement. |

`freekerf mcp` and the `host --stdio` test protocol are all thin adapters over `freekerf-service`; none adds behaviour.

### 9.2 Script engine

- **Language: JavaScript, embedded.** QuickJS through `rquickjs` (MIT) is the default choice; Boa (pure Rust, MIT/Unlicense) is the alternative. Decide in an ADR in M2 with a small benchmark (startup time, a 10,000-object batch edit, memory). CPython is **not** embedded: it cannot be sandboxed, and shipping it per platform (including tablets) is heavy.
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

Each action declares one tier. **Tiers are checked on every `dispatch`, including each call made inside a script**, so a script cannot do more than its caller is allowed to. Settings choose the baseline (highest tier allowed without asking) per caller kind (agent via MCP, user macro); above it, the human can grant more (see *Permission grants* below):

| Tier | Examples | Default for agents when MCP is enabled |
| --- | --- | --- |
| `read` | read state, document, console, screenshot | allowed |
| `ui` | select, open/close panels, change mode, zoom/pan, open dialogs | allowed |
| `edit` | change the document, import files, change layer settings, save | allowed (`document.save` only to a new `.fkproj` file; never overwrites) |
| `machine` | connect, home, jog, unlock, `$$` changes, overrides | **off** (`automation.mcp.allow_machine`) |
| `laser` | frame with laser on, test fire, run/resume a job | **never** (no setting or grant) |

- **STOP / feed hold are always allowed** (they only make things safer), and STOP cancels running scripts first.
- Agents never run `laser` actions (decided 2026-10-10). A `laser` action from a script never skips the fire confirmation of the UI: the job goes to *awaiting confirmation* and a **human confirms on the machine's screen**. In headless mode with no human present, `laser` actions are refused. Dropping the human confirmation is out of scope.
- The app shows a persistent **"Agent connected"** indicator with the client name, a one-click disconnect, a **running-script indicator with a cancel button**, and an **action log** (who did what, when, from which script) in the console.
- Materials flagged `block` by `library` stay blocked for scripts and agents too.

**Permission grants.** The tiers above are the baseline from settings. When a caller (a user macro, an agent through MCP) needs a tier above its baseline, the call pauses and the app asks the human, naming the caller, the tier and the action. The human chooses the **scope** of the grant:

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
- UI end to end (from M4): a test agent runs a script that opens a panel, selects an object and changes a layer, and Playwright checks the UI reflects it; and the opposite (a UI click shows up in `get_state`).
- **No UI-only actions:** a lint test fails if the web UI mutates app state outside `dispatch`, and the generated `doc/features/actions.md` must match the registry.
- Docs: `doc/features/mcp.md` (setup for common agents, tools, tiers, safety, attach/launch), `doc/features/scripting.md` (engine, sandbox, limits, transactions, macros, examples), `doc/features/actions.md` (generated catalogue), and ADRs for the action registry, the script engine and the MCP transport/security model.

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
- **Automation, headless (§9):** `freekerf-session` (document actions), `freekerf-script` (engine ADR, sandbox, limits, transactions, dry run, `api.d.ts`), `freekerf run`, `freekerf-ipc`, `freekerf-mcp` and `freekerf mcp` with attach-or-launch of a **headless** instance; tiers `read`, `edit`, `machine`; `laser` refused headless; disabled by default. (A Python client was planned; dropped in M2, see §15.)

**Acceptance:** `conformance` *golden (importers)* green with `LASERGRBL_HOST=rust`; `platform_dependent` (GDI+) cases compared with a documented tolerance. Script engine and MCP integration tests of §9.6 green for the headless scope; `doc/features/scripting.md`, `doc/features/mcp.md`, the generated `doc/features/actions.md` and `api.d.ts`. Fuzz targets for SVG/DXF/G-code running in a scheduled CI job. Integration test import → plan → stream to the emulator. Feature pages for every importer and generator.

### M3 — UI toolkit decision (done)
The code spike was replaced by desk research ([`doc/research/ui-toolkit.md`](doc/research/ui-toolkit.md)). **Decision: Tauri 2** ([ADR 0006](doc/adr/0006-ui-toolkit.md)). The first goal is a **desktop app**; tablets stay at M6. Multi-touch problems found during implementation are recorded in `doc/roadmap.md` and handled later; they do not block milestones. Bulk data crosses the IPC as raw payloads (`tauri::ipc::Response`, raw channels), never JSON arrays.

### M4 — Desktop MVP (10–16 weeks)
M4 is split in two phases so the app shell does not wait for M2. **M4a** builds everything that only needs the M1 service; **M4b** plugs the Design mode into the M2 crates. The Design mode screens are written in M4a against a contract (`doc/features/ui-contract.md`, removed in M4b once the real actions replaced it) and a fake service, so M4b only swaps the implementation.

#### M4a — App shell and Produce mode (does not depend on M2)
- `apps/desktop` (Tauri 2, crate `freekerf-desktop`, depends only on `freekerf-service`) and `ui/web` grown from `design/prototypes/app`; front-end framework, canvas renderer and Rust → TypeScript type generation each decided in an ADR.
- `ui/tokens` generated from `design/tokens.json` (pinned `design` commit); brand fonts and icons with credits; light, dark and high-contrast themes; automatic density by input type.
- View state in Rust (`freekerf-session::view`: mode, panels, viewport, dialogs, toasts, focus) behind `view.*` actions; the UI changes app state **only** through `dispatch(action)` (lint).
- Shell: Design/Produce modes, top bar with machine state and **STOP always visible** (also over dialogs), dockable panels, responsive layouts (wide ≥ 1180 px, medium 820–1179 px, narrow < 820 px machine only), keyboard shortcuts.
- Canvas: renderer, pan/zoom/selection with the mouse, rulers, work-area grid, **G-code preview** from M1 analysis and head simulation over a raw channel at 60 Hz.
- **Produce mode, real:** connect (serial and emulator), machine states, jog (step and continuous), home/unlock, origin, frame, open G-code, run/pause/stop, overrides, progress and remaining time, console, job recovery, laser life counters, basic custom buttons (G-code).
- **Fire confirmation** (checklist + countdown, `design/ux.md`) as the only path to `laser`-tier actions, enforced in Rust.
- Settings screen through actions (the UI never reads the TOML), automation settings page (MCP off by default, read-only in this phase).
- i18n: Fluent in `ui/i18n`, `cargo xtask resx2ftl` converting the LaserGRBL `.resx` translations that still apply, `en` and `pt-BR` complete, long-string check (`de`).
- Design mode screens (layers palette and list, inspector, shape and text tools, move/resize/rotate handles, import dialogs, migration step of the welcome flow) against the fake service in `ui/web/src/dev/`, behind a feature flag that is **off** in distributed builds.

**Acceptance (M4a):**
- the Tauri app opens on Linux (X11 and Wayland) and builds (`cargo check`) for Windows and macOS in CI;
- Produce mode works end to end against the emulator, with UI end-to-end tests in CI on Linux;
- canvas performance check (20,000 paths + 8000×6000 image, ≥ 50 fps pan with mouse) recorded in `doc/benchmarks.md`, measured on real Linux hardware (CI only gives a software-rendering floor);
- STOP always reachable; `laser` actions only after the human confirmation (tested in Rust and end to end);
- the dispatch-only lint passes; generated types checked in CI (committed == generated); the fake service is absent from the production bundle (tested);
- front-end license check (npm allow-list compatible with GPL-3.0) in CI;
- front-end unit and component tests with coverage reported;
- docs: `doc/features/{shell,produce-mode,canvas,theming,i18n,fire-confirmation,ui-contract}.md`, ADRs for the framework, the canvas and type generation, front-end commands in `AGENTS.md`.

#### M4b — Design mode on top of M2 (after M2 is merged)
- Replace the fake service by the real `document.*` actions of the contract; turn the Design mode flag on.
- Import flows (SVG, DXF, image with dithering), colour layers with cut settings, shapes, text and transforms on the real document; preview and time estimate from the planner.
- Welcome flow with LaserGRBL migration (`freekerf-migrate`); material library import (`freekerf-materials`); LaserGRBL custom buttons migrated to scripts.
- All remaining **MVP** items of the matrix in `design/research.md` §6.
- **Automation in the desktop app (§9):** selection and document actions in the registry; attach-or-launch of the desktop app; semantic view snapshot + window screenshot; "Agent connected" and running-script indicators, action log, cancel and disconnect; automation settings page writable; macro editor with `api.d.ts` autocomplete.

**Acceptance (M4b):** the 8 flows of `design/ux.md` pass as UI end-to-end tests against the emulator in CI on Linux, and the same flows can be driven end to end by a test MCP client through `run_script`; user-facing docs drafted for `website`.

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
| M2: geometry (paths, transforms, Bézier flattening, arc fitting, booleans, offsets, hatch fillings, path ordering) | `crates/core/freekerf-geometry` (Clipper 6.4.2 port in `src/clipper`) | unit tests, `tests/properties.rs`, `tests/clipper_differential.rs` (440 cases vs. LaserGRBL's `Clipper.cs`) | ADR 0007, `doc/architecture.md` |
| M2: document model and `.fkproj` (versioned, generated JSON schema) | `crates/core/freekerf-document`, `doc/schema/fkproj.schema.json` | unit tests, `tests/properties.rs` (round trip) | `doc/features/projects.md` |
| M2: planner (layer order, fillings, LaserGRBL G-code writer), generators, Hershey | `crates/core/freekerf-planner`, `freekerf-gcode/src/dotnet.rs` | unit tests, `freekerf-gcode/tests/dotnet_format.rs` (vs. Mono), conformance `generator`/`hershey` goldens | `doc/features/{planner,test-patterns}.md` |
| M2: conformance importer runner (`freekerf import-case`, `LASERGRBL_HOST=rust`) | `freekerf-service/src/import_case.rs`, `apps/cli`; conformance `importers.py` (`run_rust`), `tests/golden/test_importers.py` | `apps/cli/tests/cli.rs`, conformance `make test-rust` | `doc/features/cli.md`, conformance `README.md` |
| M2: SVG and DXF import | `crates/core/freekerf-import` (`svg/`, `dxf/`) | unit tests, `tests/properties.rs`, `freekerf-service/tests/import_cases.rs`, conformance `svg_*`/`dxf_*`/`svg_text` | `doc/features/{svg-import,dxf-import}.md`, `doc/divergences.md` |
| M2: raster (image operations, dithering, Line2Line); GDI+ tolerance | `crates/core/freekerf-raster`, `doc/adr/0008-gdiplus-tolerance.md` | unit tests, `tests/properties.rs`, `freekerf-service/tests/import_cases.rs`, conformance `l2l_*`/`img_*`  | `doc/features/raster.md`, `doc/divergences.md` (DIV-041, DIV-042, DIV-108) |
| M2: vectorization (Potrace, centerline via autotrace) | `crates/core/freekerf-raster/src/{potrace,vectorize.rs,centerline.rs,render.rs}`, `crates/runtime/freekerf-service/src/autotrace.rs`, `doc/adr/0009-potrace.md`, `doc/adr/0010-autotrace.md` | unit tests, `tests/properties.rs`, `freekerf-service/tests/import_cases.rs`, conformance `potrace_*`/`centerline_*`/`svgtext_*` | `doc/features/vectorize.md`, `doc/divergences.md` (DIV-045 to DIV-049) |
| M2: raster dialog pipeline (`image_processor`), images in `plan` | `crates/core/freekerf-raster/src/processor.rs`, `crates/core/freekerf-planner/src/image.rs` | unit tests, `freekerf-service/tests/import_cases.rs`, conformance `ip_*` (11 exact, 1 within ADR 0008 tolerance, 1 failed generation) | `doc/features/raster.md` (Images in jobs), `doc/features/planner.md`, `doc/divergences.md` (DIV-050) |
| M2: material library | `crates/config/freekerf-materials` (git dependency on `freekerf-library`, `rev` pinned; `deny.toml` allow-git) | `tests/package.rs` (package built locally with the library's packaging function) | `doc/features/materials.md` |
| M2: LaserGRBL migration (Settings.bin, laser counters) | `crates/config/freekerf-migrate`, `freekerf-service/src/migrate.rs`, `freekerf migrate lasergrbl` | `tests/lasergrbl_files.rs` (fixtures written by LaserGRBL's code), `tests/properties.rs`, CLI test | `doc/features/migration.md`, `doc/features/cli.md` |
| M2: import → plan → stream integration tests; fuzzing | `tests/it/import_plan_stream.rs`, `fuzz/` (targets `gcode`, `svg`, `dxf`, `nrbf`), `.github/workflows/fuzz.yml` (weekly) | `cargo test -p freekerf-it` (PTY emulator, 7 jobs), fuzz runs | `doc/fuzzing.md`, `doc/divergences.md` (DIV-109) |
| M2 (B1): session actions, undo/redo, transactions, snapshots, change events | `crates/core/freekerf-session/src/{session,actions}.rs`, `freekerf-service` (`document.open/save/import`, `job.from_document`, `ServiceEvent::Session`) | unit tests, `freekerf-service/tests/service.rs` | `doc/features/session.md`, `doc/features/actions.md` |
| M2 (B2): scripts (QuickJS via rquickjs, sandbox, limits, STOP cancel, one undo step, rollback, dry run, laser refused without confirmation, `api.d.ts`, `freekerf run <script.js>`) | `crates/runtime/freekerf-script`, `crates/runtime/freekerf-service/src/script.rs`, `apps/cli` | `freekerf-script/tests/sandbox.rs`, `freekerf-service/tests/scripts.rs`, `cli.rs::run_a_script`, `examples/script_bench.rs` | `doc/features/scripting.md`, `doc/adr/0011-script-engine.md`, `doc/scripting/api.d.ts` (generated) |
| M2 (B3): local endpoint and MCP (single instance, token, `freekerf serve --headless`, tiers from settings, grants this run/session/always, laser refused headless, `freekerf mcp` attach or launch, tools and resources with notifications) | `crates/native/freekerf-ipc`, `crates/runtime/freekerf-service/src/{remote,permissions}.rs`, `crates/runtime/freekerf-mcp`, `apps/cli` | `freekerf-ipc/tests/endpoint.rs`, `freekerf-service/tests/{remote,permissions}.rs`, `freekerf-mcp/tests/mcp.rs`, `cli.rs::mcp_is_off_by_default_then_attaches_or_launches` | `doc/features/{mcp,ipc}.md`, `doc/adr/0012-mcp-transport-and-security.md` |
| M2 (B4): LaserGRBL buttons → scripts (the Python client was written and then removed: no need) | `crates/config/freekerf-migrate/src/buttons.rs`, `freekerf-service/src/migrate.rs` | `freekerf-migrate/tests/lasergrbl_files.rs`, `cli.rs::migrate_lasergrbl` | `doc/features/migration.md` |
| M3: UI toolkit decision (Tauri 2) | — | — | `doc/research/ui-toolkit.md`, `doc/adr/0006-ui-toolkit.md` |
| M4a: plan split into M4a / M4b | `PLAN.md` §10 | — | `PLAN.md` |
| M4a: desktop skeleton (Tauri app, Svelte front end, generated types, design tokens, CI) | `apps/desktop/src-tauri` (`freekerf-desktop`), `ui/web`, `ui/tokens`, `tools/xtask/src/{ts,design}.rs`, `Registry::describe_result`, `EventMessage` | `bridge.rs` unit tests, `ui/web` Vitest (`src/**/*.test.ts`, `tests/unit/`), xtask `ts`/`design` tests, CI `ui`, `desktop`, `native-check` | `doc/features/desktop-app.md`, ADR 0013 (framework), ADR 0015 (types), `ui/CREDITS.md` |
| M4a: canvas (WebGL2 renderer, scene cache, gestures, rulers, grid, G-code preview, head at 60 Hz, `--bench-canvas`) | `ui/web/src/canvas/`, `freekerf-service/src/preview.rs`, `freekerf-desktop` (`preview`, `subscribe_head`, `bench.rs`) | `ui/web/src/canvas/*.test.ts`, `preview.rs` tests (shared fixture `ui/web/tests/fixtures/toolpath-v1.bin`), bridge tests | `doc/features/canvas.md`, ADR 0014, `doc/benchmarks.md` (software floor; GPU: 60 fps on Wayland and X11, 2026-10-20) |
| M4a: view state and app shell (modes, panels, dialogs, toasts, viewport; layouts, themes, density, keyboard, STOP always visible, dispatch-only lint) | `freekerf-session/src/view.rs`, `view.*` actions in `freekerf-service`, `[ui]` settings, `ui/web/src/{App.svelte,shell/,state/,i18n/}`, `ui/web/eslint-rules/`, `ui/i18n/*.ftl` | `view.rs` tests, `service.rs::view_actions_change_the_view_and_publish_it`, `ui_section_is_optional_and_settable`, `App.test.ts`, `shortcuts.test.ts`, `architecture.test.ts` | `doc/features/{shell,theming}.md`, `settings.md` |
| M4a: Produce mode (connect serial/emulator, open G-code, run/pause/stop, frame, overrides, jog step and dead-man continuous, home/unlock/origin, console, recovery, laser counters, custom buttons) | `ui/web/src/produce/`, `job.frame`, `machine.system_command`, `machine.info` in `freekerf-service`, `[[buttons]]` in `freekerf-settings` | `produce.test.ts`, `job.test.ts`, `console.test.ts`, `service.rs::produce_mode_helpers_frame_info_and_system_commands`, `custom_buttons` | `doc/features/produce-mode.md` |
| M4a: fire confirmation (only path to `laser` actions: request, checklist, countdown checked in Rust, confirm only from the window) | `freekerf-session/src/fire.rs`, `fire.request`/`fire.cancel` and `Service::confirm_fire`, `[safety]` setting, desktop `confirm_fire`, `ui/web/src/produce/FireConfirmation.svelte` | `fire.rs` tests, `service.rs::laser_actions_run_only_after_a_confirmed_countdown`, bridge `every_laser_action_needs_the_confirmation`, `fire.test.ts` | `doc/features/fire-confirmation.md` |
| M4a: settings screen (display, machine, job code, read-only safety and automation) and i18n (Fluent, language setting, `resx2ftl`, en/pt-BR/de complete, long strings) | `ui/web/src/shell/SettingsDialog.svelte`, `ui/web/src/i18n/`, `ui/i18n/`, `tools/xtask/src/resx.rs`, `[ui] language` | `i18n.test.ts`, `tests/web/long-strings.spec.ts` (Playwright), `App.test.ts` (settings), xtask `resx` tests, CI `resx2ftl --check` | `doc/features/i18n.md`, `settings.md` |
| M4a: Design mode on the UI contract, behind a flag (layers, palette, inspector, tools, selection and handles, import, welcome with LaserGRBL migration) | `ui/web/src/design/`, `ui/web/src/contract/document.ts`, `ui/web/src/state/document.svelte.ts`, `ui/web/src/dev/fake-document.ts`, `view.set_tool` (`freekerf-session::view::Tool`) | `design/*.test.ts`, `fake-document.test.ts`, `document.test.ts`, `tests/web/design-canvas.spec.ts` (Playwright), `long-strings.spec.ts`, `view.rs` tests | `doc/features/{design-mode,ui-contract}.md`, roadmap |
| M4a: end-to-end tests of the app (Produce flows and safety on the emulator), frontend coverage reported | `ui/web/tests/e2e/` (WebdriverIO + `tauri-driver`), CI `desktop` (E2E) and `ui` (coverage summary), xvfb-run exit-code fix | `produce.e2e.ts`, `safety.e2e.ts` | `doc/features/desktop-app.md`, `doc/coverage.md`, roadmap |
| M4a: canvas acceptance on real hardware (20,000 paths + 8000×6000 image, ≥ 50 fps pan): 60 fps on an Intel Iris Xe, Wayland and X11; **M4a closed** | `freekerf-desktop --bench-canvas`, `mise.toml` (Rust and Node pinned for local builds) | `--bench-canvas` exit code 0 | `doc/benchmarks.md`, ADR 0014, `AGENTS.md` |
| M4b: document editing actions (shapes, editable Hershey text, transforms with a recorded per-object transform, align, duplicate, colour layers, current layer, layer order, toggle selection); `.fkproj` v2; flaky MCP STOP test fixed (answer kept aside by the harness) | `freekerf-document/src/object.rs` (`transform`, `TextSpec`, `rect_path`, `ellipse_path`), `freekerf-planner/src/hershey.rs` (`text_polylines`), `freekerf-session/src/{actions,session}.rs` | `freekerf-document` unit tests (v1 → v2 migration, transforms), `actions.rs::editing_actions`, `hershey.rs::text_as_polylines`, `freekerf-mcp/tests/mcp.rs` | `doc/features/{projects,session,actions}.md`, roadmap |
| M4b: Design mode on the real document (UI contract and flag removed; `session.snapshot` projection, real action names, base64 import, document geometry command, migration actions, typed results for every action) | `ui/web/src/{state/document.svelte.ts,design/,dev/fake-document.ts}`, `freekerf-service/src/{preview,migrate}.rs`, desktop `document_geometry`, `Registry::register` (result schemas) | `design.test.ts`, `import.test.ts`, `fake-document.test.ts`, `document.test.ts`, `design-canvas.spec.ts`, `tests/e2e/specs/design.e2e.ts`, `preview.rs`/`migrate.rs` tests, `service.rs::results_are_described_for_the_front_ends` | `doc/features/{design-mode,canvas,migration,session}.md` |
| M4b: booleans (weld, subtract, intersect, xor), offset outline and array on the document, in the rail and the inspector | `freekerf-session/src/actions.rs` (`document.boolean/offset/array`, Clipper via `freekerf-geometry::boolean`), `ui/web/src/{design/ObjectPanel.svelte,shell/ToolRail.svelte}` | `actions.rs::boolean_offset_and_array`, `design.test.ts`, `fake-document.test.ts` | `doc/features/{session,design-mode}.md` |
| M4b: job from the design in Produce: job start (absolute / current position / user origin) and the 3×3 anchor | `freekerf-planner/src/origin.rs`, `job.from_document {start, anchor}` in `freekerf-service`, `ui/web/src/produce/MachinePanel.svelte` | `origin.rs` tests, `service.rs::document_session_actions_and_events`, `produce.test.ts` | `doc/features/produce-mode.md` |
| M4b: images in the Design mode: the raster dialog (method, dithering, resolution, adjustments, powers) with a Rust preview; toasts above the palette | `freekerf-planner::image::preview_bitmap`, `document.raster_settings/set_raster/raster_preview` in `freekerf-session`, `ui/web/src/design/RasterDialog.svelte` | `actions.rs::raster_settings_and_preview`, `design.test.ts`, `design.e2e.ts` | `doc/features/{design-mode,session}.md` |
| M4b: material test (flow 6: power × speed grid, a layer per cell, Hershey labels) from the tool rail | `document.material_test` in `freekerf-session`, `ui/web/src/design/MaterialTestDialog.svelte`, `Dialog::MaterialTest` | `actions.rs::material_test_grid`, `design.test.ts` | `doc/features/{design-mode,session}.md`, roadmap (library and "save as material") |
| M4b: the welcome opens by itself on the first run (`[ui] welcomed`) | `freekerf-settings` (`UiSettings::welcomed`), `ui/web/src/App.svelte`, `state/settings.svelte.ts` | `ui_section_is_optional_and_settable`, `design.test.ts` | `doc/features/{settings,design-mode}.md` |
| M4b: automation in the desktop app: window prompter (permission dialog, denied after 2 min), local endpoint served by the app, agents and running-script indicators with Cancel, action log, grants with revoke, automation and safety settings writable by the person | `freekerf-service/src/automation.rs` (`AutomationEvent`, `automation.status/log/grants`, `WindowPrompter`, `Service::set_user_setting`, `answer_permission`), desktop `bridge.rs`/`lib.rs` commands, `ui/web/src/{state/automation.svelte.ts,shell/PermissionDialog.svelte,shell/TopBar.svelte,shell/SettingsDialog.svelte}` | `tests/automation.rs`, `tests/remote.rs`, `shell/automation.test.ts`, `App.test.ts` | `doc/features/{desktop-app,mcp}.md`, roadmap (macro editor) |
| M4b: the Design flows driven by an agent through run_script (flows 3, 5, 6; laser still refused); typed results in `api.d.ts` and `search_api` | `freekerf-mcp/tests/mcp.rs::design_flows_driven_by_an_agent`, `freekerf-script/src/dts.rs` | same, `dts.rs` tests | `doc/features/{mcp,scripting}.md`, `doc/scripting/api.d.ts` |
| M4b: the flows of `design/ux.md` as end-to-end tests of the real app on the emulator (1–7; 8 is not in the MVP); STOP test made robust (the host reads `$$`/`$I` again after a reset) | `ui/web/tests/e2e/specs/{welcome,design-flows,job-flows,design,produce,safety}.e2e.ts`, `wdio.conf.ts` (first-run home, LaserGRBL fixtures) | same, `service.rs::stop_after_a_system_command_clears_the_console_and_resets` | `doc/features/desktop-app.md` |
| M4b: user documentation drafted for `website` (en and pt-BR: getting started, LaserGRBL, designing, images, material test, running a job, safety, automation, troubleshooting) | `freekerf/website`: `src/content/docs/{en,pt-br}/` | — | `src/content/docs/README.md` there |
| M5: Windows installers (MSI and NSIS, not signed yet) built by the release workflow on a `v*` tag, smoke-tested (silent install, the app stays up, silent uninstall) and attached to a draft GitHub release | `.github/workflows/release.yml`, `.github/scripts/smoke-windows-installer.ps1` | the workflow's smoke step | `doc/features/desktop-app.md` (Releases), roadmap |
| M5: Linux packages: `.deb` and AppImage built on Ubuntu 22.04 with the `freekerf` CLI inside, shared desktop entry and AppStream metainfo, app id `io.github.freekerf.FreeKerf` | `release.yml` (`linux`), `apps/desktop/src-tauri/tauri.conf.json` (`bundle.linux`), `packaging/linux/` | `.github/scripts/smoke-linux-installer.sh` (`.deb` in `ubuntu:24.04`, AppImage in `ubuntu:24.04` and `fedora:latest`), `appstreamcli validate` | `doc/features/desktop-app.md` (Releases), ADR 0016, roadmap |
| M5: Flatpak bundle built offline (GNOME runtime, committed cargo and npm sources), the CLI attaches to the window across sandboxes (`$XDG_RUNTIME_DIR/app/$FLATPAK_ID`); Flathub submission left to a maintainer | `release.yml` (`flatpak`, `flatpak-test`), `packaging/flatpak/`, `freekerf-platform` (`Dirs::user`) | `smoke-linux-installer.sh flatpak` (MCP attach), `flatpak_instances_share_the_app_runtime_dir` | `doc/features/{desktop-app,mcp}.md`, roadmap (Flathub) |
| M5: macOS universal disk image (Apple Silicon and Intel, macOS 11+, signed ad hoc, not notarized) with the `freekerf` CLI inside the app as a sidecar | `release.yml` (`macos`), `tauri.conf.json` (`bundle.macOS`), `packaging/macos/tauri.cli.json` | `.github/scripts/smoke-macos-installer.sh` (mount, signature, architectures, the app up, MCP attach) | `doc/features/desktop-app.md` (Releases, Running on macOS), roadmap (Developer ID, notarization) |
| M5: release train: quick Linux checks on pull requests to `main`; native runners and the app end to end every night and on pull requests to `stable`; installers on pull requests to `stable`, and a push to `stable` with a new version tags it and drafts the release | `.github/workflows/{ci,native,release}.yml` (`version`, `publish`) | the workflows' `version` job | `doc/features/desktop-app.md` (Releases), `AGENTS.md`, ADR 0018 |
| M5: serial permission warning: ports listed with their access and group (never opened), `PermissionDenied` on connect as a failure with a cause, the Machine panel says how to join `dialout`/`uucp` | `freekerf-transport` (`serial::{available_ports, port_access}`), `freekerf-session` (`Failure`, `FailureCause`), `freekerf-service` (`machine.list_ports`, `connect_err`), `ui/web/src/produce/PortPermissionWarning.svelte` | `serial::tests`, `service.rs::a_denied_port_fails_with_its_cause`, `produce.test.ts` | `doc/features/connection.md`, ADR 0017 |
| M5: text objects with fonts: built-in single-line fonts (Relief SingleLine, Hershey, EMS) and installed TrueType/OpenType fonts, shaping, style (bold, italic, underline, strikethrough), lines, alignment, spacing, width, effects (weld, outline, stencil bridges, arc), text on a path that follows it, convert to paths, SVG `text`/`textPath` imported as editable text; `.fkproj` v4; offset outlines open strokes | `crates/core/freekerf-text`, `freekerf-document/src/object.rs` (`TextSpec`, `TextPath`), `freekerf-session/src/{actions,session}.rs`, `freekerf-import/src/svg/text.rs`, `freekerf-platform/src/fonts.rs`, `ui/web/src/design/TextPanel.svelte` | `freekerf-text` unit tests (`testfont.rs`), `svg/mod.rs` text tests, `tests/properties.rs::svg_texts_never_panic`, `actions.rs::text_styles_paths_and_fonts`, `design.test.ts`, `text-editor.test.ts` | `doc/features/text.md`, `doc/adr/0019-text-and-fonts.md`, `doc/features/{projects,svg-import,design-mode,session}.md`, roadmap |

## 14. Decisions already taken

- **Settings format: TOML** (readable in diffs, consistent with `library`). The schema is generated from the Rust model and checked in CI; the web UI reads and writes settings only through actions, never the file.
- **`conformance` is public**, so CI in this repository checks it out without a token.
- **No auto-update for now.** Releases ship as installers and through package managers (M5); the app does not check for or download updates. Revisit later through `doc/roadmap.md`.
- **UI toolkit: Tauri 2** (ADR 0006). Desktop first; gesture problems are deferred, not blocking; tablets at M6.
- **No MCP prompts for now.** The MCP server exposes the tools of §9.3 and the state resources; MCP *prompts* (e.g. "prepare a cut job from this SVG") and example scripts as resources are not needed now. Listed in `doc/roadmap.md` to revisit later.

## 15. Open questions

- ~~MCP: should `allow_laser` exist at all?~~ Decided 2026-10-10: no, agents never run `laser` actions.
