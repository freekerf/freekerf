# 0006. UI toolkit: Tauri 2

- Status: **accepted** (product decision, 2026-10-07)
- Date: 2026-10-07
- Confirms the preliminary recommendation of `design/tech-notes.md`; the research in [`doc/research/ui-toolkit.md`](../research/ui-toolkit.md) proposed Flutter + flutter_rust_bridge, and that proposal is **not** adopted.

## Context

The UI is a thin client of `freekerf-service` (PLAN §2, §9): app state lives in Rust and the UI renders it and dispatches actions. M3 was changed from a code spike to desk research. The research recommended Flutter, but verification on import found errors in part of its evidence (report §0): Tauri v2 has raw-byte IPC, and two of the "real-world" cases did not hold up. What remained against Tauri — WebKitGTK canvas/WebGL stability and touch on Linux Wayland — is a known risk without measured numbers.

## Decision

- **Tauri 2** for the desktop app (`apps/desktop`, `src-tauri` = crate `freekerf-desktop`), with the web front end in `ui/web` grown from the HTML/CSS prototype of `design` and the `fk-` tokens as CSS custom properties.
- **The first goal is a desktop app** (Linux first, then Windows and macOS). Tablets (Android USB host, iPad remote client) stay at M6 and will be revisited then.
- **Gestures are best effort.** If multi-touch problems show up during implementation (for example WebKitGTK on Wayland), they are recorded in `doc/roadmap.md` and handled later; they do not block milestones. Mouse, keyboard and single-pointer touch must work.
- **IPC:** bulk data (geometry, images, previews) uses Tauri's raw payloads (`tauri::ipc::Response`, raw channels), never JSON number arrays. Events and small commands use JSON.
- The experiments of the research report (§8) are **not required** to proceed. Canvas performance on Linux is watched during M4 instead (see Consequences).

## Alternatives considered

- **Flutter + flutter_rust_bridge:** self-contained renderer and in-app gestures, but a full UI rewrite in Dart, no reuse of the prototype, and its Linux renderer (Impeller) became the default only in Flutter 3.47.
- **Electron:** same Chromium everywhere and full prototype reuse; 300–500 MB idle memory, no Android.
- **Kotlin Multiplatform / Compose:** bundled JRE, Wayland touch limits and JNI/GC costs on desktop.
- **Slint / iced / egui / Dioxus:** full rewrite and a custom CAD canvas with little ecosystem help.

## Consequences

- The prototype in `design/prototypes/app` is the starting point of `ui/web`.
- **Canvas risk on Linux (WebKitGTK):** M4 includes a canvas performance check (20,000 paths + 8000×6000 image, ≥ 50 fps pan) on Linux. If it fails, first options are rendering inside the WebView with WebGL2/WebGPU or a wasm renderer; changing the shell (Electron or a Chromium-based WebView) is a later product decision recorded in a new ADR.
- **Gesture problems** found on Linux go to `doc/roadmap.md` with the platform and a reproduction, not into milestone acceptance.
- UI end-to-end tests use Playwright with Tauri WebDriver (PLAN §6), on Linux in CI.
- Window screenshots for MCP come from the Tauri window.
