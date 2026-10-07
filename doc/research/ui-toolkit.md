# UI toolkit research (M3)

> **Status:** desk research, received 2026-10-07 from a web-research agent with no access to this repository. It replaces the code spike originally planned for M3. Its recommendation (Flutter) was **not adopted**: the product decision is Tauri 2, desktop first, with gesture problems deferred ([ADR 0006](../adr/0006-ui-toolkit.md)). The report is kept as background for the risks it lists.
>
> **Read §0 first.** The original report came without a source list, and spot checks found factual errors in some of its supporting evidence. The report body (§1–§10) is kept as received, reformatted to Markdown; the corrections in §0 take precedence over it.

## 0. Verification notes (added on import)

Five claims that weigh on the decision were checked on 2026-10-07:

| Claim in the report | Check | Verdict |
| --- | --- | --- |
| Flutter renders with Impeller on Linux desktop | Impeller became the **default** renderer on macOS, Windows and Linux only in Flutter 3.47 (August 2026); a Vulkan backend for Linux is still in progress ([What's new in Flutter 3.47](https://flutter.dev/blog/whats-new-in-flutter-3-47), [flutter/flutter#162684](https://github.com/flutter/flutter/pull/162684)) | **True but very recent.** The report presents Impeller on Linux as mature; it is two months old. Benchmarks quoted for Impeller come from mobile, not from Linux desktop. |
| Flutter maps Linux touch (GTK/libinput) into its gesture arena | The GTK embedder sends GTK touch events to the framework as touch pointers; pinch works on Linux touchscreens in community apps; trackpad pinch had a first-gesture bug fixed recently ([flutter/flutter#193659](https://github.com/flutter/flutter/pull/193659), [snonux/comicredr#5](https://github.com/snonux/comicredr/pull/5)). Wayland multi-touch problems are reported for Sony's embedded embedder, not the desktop one ([sony/flutter-embedded-linux#127](https://github.com/sony/flutter-embedded-linux/issues/127)) | **Plausible, not proven** for our case. Experiment C is still needed. |
| Tauri IPC inflates `Vec<u8>` to JSON number arrays; a custom URI protocol is required | Tauri v2 returns raw bytes as an `ArrayBuffer` through `tauri::ipc::Response`, and channels carry raw payloads (`InvokeResponseBody::Raw`); requests can be sent as `application/octet-stream` ([Calling Rust from the frontend](https://v2.tauri.app/develop/calling-rust/), [core API](https://v2.tauri.app/reference/javascript/api/namespacecore/)) | **Wrong as stated.** JSON inflation only happens with default serialization. The IPC gap between Tauri and flutter_rust_bridge is smaller than the report says. Experiment B must test Tauri's raw `Response`/channels, not only `invoke` with JSON. |
| RustDesk migrated **from Tauri** to Flutter because Tauri IPC could not stream 60 Hz frames | RustDesk migrated **from Sciter** (not open source) to Flutter; Tauri vs. Flutter was only discussed ([discussion #533](https://github.com/rustdesk/rustdesk/discussions/533), [migration post](https://dev.to/rustdesk/rustdesk-is-migrated-from-sciter-to-flutter-279m)). The team notes Flutter code is more verbose than web UI code ([Flutter desktop practice](https://dev.to/rustdesk/flutter-desktop-development-practice-4c7a)). RustDesk also has an open report of the Flutter main window painting black on Linux ([rustdesk#16350](https://github.com/rustdesk/rustdesk/issues/16350)) | **Wrong.** The IPC justification is not supported. RustDesk is still a relevant Flutter + Rust reference, including its Linux rendering issues. |
| xTool Creative Space chose Electron to bypass WebKitGTK on Linux | XCS is Electron-based but **does not support Linux** at all ([xTool support](https://support.xtool.com/article/1917), [AUR package notes](https://aur.archlinux.org/packages/xtool-creative-space)) | **Wrong.** Not evidence about Linux rendering. |

Consequences for the decision:
- The strongest arguments left for Flutter are **one self-contained renderer on every Linux distribution** (independent of the system WebKitGTK version), **gestures handled in-app**, and **Android with USB host**. They are plausible but not measured for our workload.
- The case against Tauri rests on **WebKitGTK canvas/WebGL stability on Linux** (NVIDIA, Wayland) and touch through WebKit. That is a known risk in `design/tech-notes.md`, but this report gives **no measured numbers** for it.
- The "20,000 elements at 28 FPS / 0.48 W" Impeller figure and the "450 ms → 11 ms" clipping figure have no source and are not Linux desktop numbers: do not quote them as evidence.
- ADR 0006 chose Tauri 2 without running experiments A–C; canvas performance on Linux is checked in M4 instead.

---

## 1. Executive summary (as received)

The application must render 20,000 paths and an 8000×6000 raster at ≥ 50 fps, track the laser head at 60 Hz, support multi-touch and screen readers, and run on Linux (X11 and Wayland), Windows, macOS and Android tablets, with a Rust core.

The report recommends a pivot from the preliminary choice (Tauri 2):

- **Primary: Flutter + flutter_rust_bridge v2** — Impeller rendering, zero-copy FFI, GTK/libinput multi-touch on Linux, mature Android USB host serial.
- **Backup: Electron** if keeping the HTML/CSS/JS prototype is mandatory, or **Tauri 2** with raw IPC if installer size and idle memory take precedence over Linux WebKitGTK canvas stability.

| Dimension | Flutter + flutter_rust_bridge v2 | Tauri 2 (system WebView) | Electron |
| --- | --- | --- | --- |
| Canvas rendering | High (Impeller over Vulkan/Metal/GL) | Conditional (high on Windows/macOS; unstable on Linux WebKitGTK) | High (same Chromium WebGL2/WebGPU everywhere) |
| Rust IPC, 5 MB batch | Zero-copy via Dart FFI (< 5 ms) | Raw payloads needed (< 20 ms) — see §0 | SharedMemory / Node addon (< 10 ms) |
| Linux Wayland multi-touch | GTK/libinput into the gesture arena | WebKitGTK touch events (issues on NVIDIA/Wayland) | Chromium touch/pointer events |
| Idle memory | ~90–120 MB | ~50–150 MB | ~300–500 MB |
| HTML/CSS prototype reuse | Low (rewrite as Dart widgets) | Full | Full |

Three reasons given for the pivot: WebKitGTK instability on canvas workloads on Linux; IPC serialization overhead (corrected in §0); multi-touch and screen-reader parity on Linux Wayland.

## 2. Evidence matrix (as received)

| Criterion | Tauri 2 | Flutter + frb v2 | KMP / Compose Desktop | Pure Rust (Slint / iced / egui) | Electron |
| --- | --- | --- | --- | --- | --- |
| 1. 20k paths + 8k image | Weak: WebGL/WebGPU stalls under WebKitGTK on Linux | Strong: Impeller handles 20k elements efficiently | Moderate: Skia via Skiko, JVM GC pauses | Moderate: possible, but no CAD widget ecosystem | Strong: optimized Chromium WebGL2/WebGPU |
| 2. Touch & gestures (Linux/Wayland) | Weak: gestures dropped on WebKitGTK/Wayland; NVIDIA issues | Strong: GTK/libinput gestures into the gesture arena | Weak: desktop Wayland gestures immature | Weak: rudimentary outside mobile | Strong: reliable Chromium touch/pointer events |
| 3. Rust IPC (60 Hz, 5 MB) | Moderate (see §0: raw payloads exist) | Strong: zero-copy arrays over C FFI | Moderate: UniFFI/JNI arrays on the JVM heap | Strong: in-process calls | Moderate: native addons or IPC streams |
| 4. Android USB serial host | Weak: custom plugin over JNI | Strong: usb-serial-for-android through a Kotlin plugin | Strong: direct usb-serial-for-android | Inexistent | N/A (no Android) |
| 5. LAN remote client (iPad) | Strong: same web front end served over HTTP/WebSocket | Moderate: Flutter Web, larger bundle | Weak: heavy Kotlin/Wasm assets | Weak: wasm exists, different layout | Strong: web front end served to the browser |
| 6. Screen readers | Strong: DOM/ARIA parallel tree | Strong: Semantics → NSAccessibility, UIA/NVDA, Orca | Moderate: AccessKit integration in progress | Strong (iced, egui via AccessKit) | Strong: DOM to OS accessibility |
| 7. Installer & memory | Strong: ~10–15 MB, ~50–150 MB RAM | Strong: ~15–25 MB, ~90–120 MB RAM | Weak: ~50–80 MB (jlink JRE), ~150–300 MB RAM | Strong: ~5–12 MB, ~30–60 MB RAM | Weak: ~80–120 MB, ~300–500 MB RAM |
| 8. HTML prototype reuse | Strong: full, tokens as CSS custom properties | Weak: rebuild as Dart widgets | Weak: rebuild as composables | Inexistent: rewrite in a DSL or Rust | Strong: full |

## 3. Candidates (as received, condensed)

### 3.1 Tauri 2
- **Strengths:** runs the HTML/CSS/JS prototype as is (tokens and Pointer Events logic); 10–15 MB installers and 50–150 MB idle memory on Windows/macOS; the same front end can be served to a remote iPad or Android browser.
- **Risks:** on Linux, WebKitGTK reports blank WebViews on Wayland + NVIDIA, WebGL context loss and slow canvas texture uploads; the report claims frame rates below 30 fps for our canvas size (no source given). IPC: see §0. Touch on Wayland: secondary pointers reportedly dropped during simultaneous pan + pinch, breaking "cancel the drag on the second finger".

### 3.2 Flutter + flutter_rust_bridge v2
- **Strengths:** AOT-compiled Dart; Impeller pre-compiles shaders (no shader jank); flutter_rust_bridge v2 passes typed data over Dart FFI without serialization; the Linux GTK embedder delivers touch to the gesture arena; Android USB serial through usb-serial-for-android.
- **Risks:** the HTML/CSS prototype and CSS tokens must be rebuilt as Dart widgets and theme data; a remote iPad client through Flutter Web has a large wasm download.

### 3.3 Kotlin Multiplatform + Compose Multiplatform
- **Strengths:** native Jetpack Compose on Android with direct USB serial access and good touch; clean separation of shared logic and platform UI.
- **Risks:** desktop needs a bundled JRE (50–80 MB installers, 150–300 MB idle memory, 1.5–3 s cold start); Wayland touch reportedly mapped to single-pointer mouse events (no pinch, no multi-finger cancel); fractional scaling blur on Wayland; 60 Hz streams over UniFFI/JNI allocate on the JVM heap and can cause GC pauses.

### 3.4 Pure Rust (Slint, egui, iced, Dioxus)
- **Slint:** small and fast, GPLv3 option, but its DSL means a full rewrite and the 20k-path canvas must be custom.
- **egui:** immediate mode re-lays out every frame (power use on static scenes); incomplete accessibility for canvas content.
- **iced:** powers COSMIC; AccessKit integration; pinch and multi-finger cancel need low-level `iced_winit` work.
- **Dioxus:** desktop uses the system WebView (same WebKitGTK issues); Dioxus Native (Blitz) is experimental.

### 3.5 Electron (reference)
- **Strengths:** a pinned Chromium gives the same WebGL2/WebGPU and touch behaviour on every Linux distribution, X11, Wayland and driver; the prototype runs unchanged.
- **Risks:** 300–500 MB idle memory, 80–120 MB installers, no Android.

## 4. Linux deep dive (as received)

| Dimension | WebKitGTK (Tauri 2) | Flutter Linux embedder | Compose Desktop (Skiko/JVM) |
| --- | --- | --- | --- |
| Graphics pipeline | GTK4/GDK compositing over ANGLE/Skia | Own EGL/Vulkan surface from the GTK host; Impeller (default since 3.47, see §0) | Skiko/Skia over OpenGL or software AWT |
| Wayland drivers | WebGL context loss and black windows reported on NVIDIA | Renders to its own surface, bypassing GTK widget composition | Fractional DPI scaling blur reported |
| Multi-touch | libinput → compositor → GTK → WebKit UI process → web process; dropped pointers reported | GTK touch events → touch pointers → gesture arena | Touch mapped to single-point mouse events; no pinch |
| Distro consistency | Depends on the system WebKitGTK version | Self-contained engine | Self-contained Skia, depends on the JRE/AWT |

## 5. Real-world applications (as received, with §0 corrections)

- **LightBurn (Qt/C++):** paints with OpenGL directly; handles 100,000+ paths smoothly. Lesson in the report: direct GPU rendering without a WebView is the most stable for real-time laser software.
- **xTool Creative Space (Electron + C++ core):** Electron-based. *Correction (§0): XCS does not support Linux, so it is not evidence about WebKitGTK.*
- **Graphite (Rust + wasm + web, WebGPU via wgpu):** reports delays from browser WebGPU gaps, notably on Linux and Safari/WebKit, with software fallbacks for SVG rendering.
- **RustDesk (Flutter + Rust core via flutter_rust_bridge):** *Correction (§0): migrated from Sciter, not Tauri; the IPC rationale is not supported. Still the closest Flutter + Rust reference, including an open Linux rendering issue.*
- **Rayforge (GTK4/Python laser CAM):** GTK render-node invalidation overhead when moving the laser head in real time; lesson: never draw a 60 Hz overlay through a widget tree over heavy geometry.

## 6. Kotlin Multiplatform assessment (as received)

| Dimension | Finding | Impact |
| --- | --- | --- |
| Desktop packaging | jpackage/JRE bundling, 50–80 MB installers | Against the small-installer preference |
| Memory and cold start | 150–300 MB idle, 1.5–3 s start | Heavier than Rust/Flutter |
| Linux Wayland touch | Multi-touch mapped to single mouse clicks | Fails multi-finger gestures and drag cancel |
| UniFFI/JNI bridge | 60 Hz streams cause JVM GC pauses | Frame drops in head simulation |
| Android tablets | Native Compose, direct USB serial | Fully meets the tablet requirement |

Conclusion of the report: strong for shared logic on mobile, ill-suited as the desktop UI of a heavy vector CAD/CAM tool on Linux.

## 7. Top risks of the recommendation and mitigations (as received)

| Risk | Severity | Mitigation |
| --- | --- | --- |
| Losing the HTML/CSS prototype | Medium | Generate Dart `ThemeData` from the same JSON design tokens (e.g. Style Dictionary), so colours, density and accessibility themes stay in sync with `design/tokens.json`. |
| Canvas repaint jank with 20k paths | High | Two layers: the static scene (paths + image) rendered to a cached GPU picture (`Picture.toImage()`), panned/zoomed as a transformed texture and re-rasterized at rest; the 60 Hz laser head on its own `RepaintBoundary`. |
| Flutter Web size for the remote iPad | Medium | Do not use Flutter Web: the Rust core serves a lightweight HTML5/WebGL page over HTTP/WebSocket for the remote client. |

## 8. Remaining validation (not required after ADR 0006; kept for reference)

| | Experiment A — canvas stress | Experiment B — IPC latency | Experiment C — Wayland multi-touch |
| --- | --- | --- | --- |
| Goal | Sustained fps, freeze time and RSS while panning/zooming 20,000 SVG paths over an 8000×6000 RGBA image | Transport latency and UI-thread blocking for 60 Hz head positions plus 5 MB geometry batches from Rust | Pinch accuracy, two-finger pan, and immediate drag cancel when a second finger lands |
| Targets | Flutter (Impeller), Tauri 2 (WebKitGTK), Electron | flutter_rust_bridge v2; Tauri 2 **raw `ipc::Response` and raw channels** (added on import, §0); Tauri 2 custom URI protocol; Tauri 2 JSON `invoke` as baseline | Flutter GTK embedder, WebKitGTK (Tauri 2), Chromium (Electron) |
| Setup | Ubuntu 24.04 LTS on Wayland, mid-range laptop with Intel Iris Xe **and** an NVIDIA GPU (e.g. RTX 3050 mobile); add X11 runs | `Instant` in Rust to the UI clock, 10,000 iterations | GNOME Wayland touchscreen, kernel ≥ 6.8; injected multi-touch via libevdev (`ABS_MT_*`) |
| Measure | MangoHud frame pacing, renderer timelines | Per-message latency, UI-thread stall | Event traces in the app |
| Pass | ≥ 50 fps over 30 s of continuous pan; ≤ 100 ms worst frame during fast 10× zoom; ≤ 1.5 GB RSS | ≤ 5 ms UI-thread stall per 5 MB batch; < 16.6 ms end-to-end | 100 % of single-finger drags cancel on the second finger; no lost gesture-end events |
| Effort | ~16 h | ~12 h | ~12 h |

## 9. Open product decisions (as received)

1. **Prototype reuse vs. canvas performance.** Electron keeps all HTML/CSS/JS and tokens but costs 300–500 MB idle memory; Flutter needs the UI rebuilt in Dart but promises better canvas performance, zero-copy IPC, in-app gestures and ~90–120 MB.
2. **System WebView on Linux.** Tauri keeps a ~15 MB installer but depends on the distribution's WebKitGTK; distribution updates can bring rendering bugs the app cannot fix.
3. **Separate remote iPad client.** Serving a small HTML5 canvas page from the Rust core keeps Flutter Web out of the remote path, at the cost of a second, smaller front end.

## 10. Recommendation (as received)

Adopt **Flutter + flutter_rust_bridge v2** for desktop and Android; use **usb-serial-for-android** through a Kotlin plugin for tablet USB serial; serve the **remote iPad client** as a lightweight HTML5/WebGL page from the Rust core; fall back to **Electron** only if full reuse of the HTML/CSS prototype is mandatory and 300–500 MB idle memory is acceptable.

## Sources

The original report had no source list. Sources used for the verification in §0 (accessed 2026-10-07):

- [What's new in Flutter 3.47](https://flutter.dev/blog/whats-new-in-flutter-3-47); [Flutter 3.47: Impeller for desktop (AlternativeTo)](https://alternativeto.net/news/2026/8/flutter-3-47-introduces-standalone-ui-packages-and-impeller-for-desktop/); [flutter/flutter#162684](https://github.com/flutter/flutter/pull/162684)
- [flutter/flutter#193659](https://github.com/flutter/flutter/pull/193659); [snonux/comicredr#5](https://github.com/snonux/comicredr/pull/5); [sony/flutter-embedded-linux#127](https://github.com/sony/flutter-embedded-linux/issues/127)
- [Tauri v2: Calling Rust from the frontend](https://v2.tauri.app/develop/calling-rust/); [Tauri v2 core API](https://v2.tauri.app/reference/javascript/api/namespacecore/); [tauri-apps/tauri#7706](https://github.com/tauri-apps/tauri/issues/7706)
- [RustDesk discussion #533: Tauri or Flutter](https://github.com/rustdesk/rustdesk/discussions/533); [RustDesk migrated from Sciter to Flutter](https://dev.to/rustdesk/rustdesk-is-migrated-from-sciter-to-flutter-279m); [Flutter desktop development practice](https://dev.to/rustdesk/flutter-desktop-development-practice-4c7a); [rustdesk#16350](https://github.com/rustdesk/rustdesk/issues/16350)
- [xTool Creative Space compatibility](https://support.xtool.com/article/1917); [AUR: xtool-creative-space](https://aur.archlinux.org/packages/xtool-creative-space)
