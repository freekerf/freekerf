# Desktop app

The FreeKerf desktop app is a [Tauri 2](https://v2.tauri.app/) window
(`apps/desktop/src-tauri`, crate `freekerf-desktop`) around the same
`freekerf-service` the CLI uses, with a web front end in `ui/web` (Svelte 5,
[ADR 0013](../adr/0013-ui-framework.md)). Decision: [ADR 0006](../adr/0006-ui-toolkit.md).

## Running it

See `AGENTS.md` ("Desktop app"). In short: `npm ci` in `ui/web`, then
`cargo run -p freekerf-desktop` (development, loads the Vite dev server) or
`npm run build && cargo run -p freekerf-desktop --features custom-protocol`
(the front end embedded, as in a release). `FREEKERF_HOME` points the app at
another settings directory (tests).

`npm run dev` and `http://localhost:5173/dev.html` run the same UI in a browser on
the **fake service** (`ui/web/src/dev/`), without Rust: useful for layout work and
for the Design mode screens until M2 lands.

## Boundary between the window and the app

| Tauri command | Does |
| --- | --- |
| `dispatch(action, params)` | runs an action of the registry ([actions.md](actions.md)) and returns its JSON result or an `ActionError` |
| `catalog()` | the action specs (name, tier, parameter and result schemas) |
| `subscribe(events)` | streams service events (`EventMessage` JSON) on a Tauri channel |
| `confirm_fire(id)` | the Fire laser button: runs the pending laser request after its countdown ([fire-confirmation.md](fire-confirmation.md)) |
| `preview()` | the tool path of the loaded program, raw bytes ([canvas.md](canvas.md)) |
| `subscribe_head(frames)` | head frames, raw 24 bytes, up to 60 Hz ([canvas.md](canvas.md)) |
| `document_geometry()` | the document's drawable geometry, raw bytes ([canvas.md](canvas.md)) |
| `answer_permission(id, scope)` | the person's answer to a permission request of a script or an agent (`run`, `session`, `always`, or none: denied) |
| `set_user_setting(key, value)` | changes a setting as the person, `automation.*` and `safety.*` included |
| `revoke_grants(kind, name)` | revokes a macro's or an agent's session and "always" grants |
| `cancel_scripts()` | stops every running script |
| `bench_report(report)` | only with `--bench-canvas`: prints the canvas benchmark and exits |

- The window calls as `{kind: user, name: "desktop", max_tier: machine}`. Actions
  of tier `laser` (`job.run`, `job.resume`, `machine.send`, `machine.custom_code`)
  are refused; they run only through the fire confirmation (`fire.request`, then
  `confirm_fire` after the countdown).
- STOP (`machine.stop`) and the other `safety` actions are always allowed.
- **Automation.** The app has a person in front of it: it installs the window
  prompter (`Service::install_window_prompter`), so a script or an agent asking
  for more than its tier waits for the person's answer in the permission dialog
  (denied after two minutes); and it serves the local endpoint, so `freekerf mcp`
  attaches to it (agents are refused while MCP is off). The top bar shows the
  agents attached and the running script (with Cancel); Settings → Automation
  turns MCP on and off, lists the grants (revoke) and the recent actions of
  scripts and agents (`automation.status`, `automation.grants`, `automation.log`,
  `automation` events). The person-only commands above are not actions: no script
  or agent can answer for the person or change these settings.
- No Tauri plugin is enabled and the capability file grants nothing: the web view
  has no file system, shell or network access of its own. The content security
  policy only allows the app's own resources and the IPC.
- The app makes no network calls (no telemetry, no update checks).

## Types

The TypeScript types of actions (parameters, results, tiers), events and settings
are generated from the Rust schemas into `ui/web/src/gen/api.ts`
([ADR 0015](../adr/0015-generated-ui-types.md)); `cargo xtask check-docs` fails in CI
when they are stale.

## Design system

Tokens (`ui/tokens/fk-tokens.css`), UI icons, fonts and app icons come from
`freekerf/design` at the commit in `ui/tokens/design.lock`
(`cargo xtask sync-tokens`, checked in CI). Credits: `ui/CREDITS.md`.

## Platforms

Linux (WebKitGTK 4.1) is the first target and runs the CI smoke test (the app
starts on Xvfb and stays up). Windows (WebView2) and macOS (WKWebView) compile in
CI on native runners (`native-check`, which checks the whole workspace). Both jobs
are in `.github/workflows/native.yml`, which runs every night on `main`, on pull
requests to `stable` and by hand, not on pull requests to `main`.

## Releases

`.github/workflows/release.yml` builds the installers with the Tauri CLI
(`cargo tauri build`, version pinned in the workflow) and `flatpak-builder`, and
smoke-tests each one. Nothing is signed with a publisher certificate yet (roadmap);
the macOS app is signed ad hoc.

| Format | Job | Built on | What the smoke test checks |
| --- | --- | --- | --- |
| MSI (WiX, every user, Program Files) and NSIS `-setup.exe` (current user, `%LOCALAPPDATA%`, no administrator rights) | `windows` | `windows-latest` | `.github/scripts/smoke-windows-installer.ps1`: silent install, the app starts on a fresh `FREEKERF_HOME` and stays up 15 s, silent uninstall |
| `.deb` (Debian, Ubuntu; package `free-kerf`, which Tauri derives from the product name; `/usr/bin/freekerf-desktop` and the CLI `/usr/bin/freekerf`) | `linux` | `ubuntu-22.04` (its glibc is the oldest supported) | `.github/scripts/smoke-linux-installer.sh deb` in a clean `ubuntu:24.04` container: `apt install`, `freekerf --version`, the app up 15 s under Xvfb, `apt remove` |
| AppImage (any distribution with glibc ≥ 2.35; the CLI is inside) | `linux` | `ubuntu-22.04` | `smoke-linux-installer.sh appimage` with `--appimage-extract-and-run` in `ubuntu:24.04`, and in `fedora:latest` (reported, not blocking) |
| Disk image `FreeKerf_<version>_universal.dmg` (one app for Apple Silicon and Intel, macOS 11 or newer, signed ad hoc, not notarized; the CLI is `FreeKerf.app/Contents/MacOS/freekerf`) | `macos` | `macos-latest` (Apple Silicon; the Intel slice is cross-compiled) | `.github/scripts/smoke-macos-installer.sh`: mounts the image, copies the app out, `codesign --verify`, both architectures in the app and the CLI, `freekerf --version`, the app up 15 s on a fresh `FREEKERF_HOME` with `freekerf mcp` attached to it |
| Flatpak bundle (`io.github.freekerf.FreeKerf`, GNOME runtime; the CLI is inside) | `flatpak` (build), `flatpak-test` | the `ghcr.io/flathub-infra/flatpak-github-actions:gnome-51` container (the freedesktop SDK image: no package manager, no Xvfb), then `ubuntu-24.04` | `smoke-linux-installer.sh flatpak` on the runner: bundle install, the app up 15 s, `freekerf mcp` in a second sandbox attaches to it, uninstall |

The WebView2 runtime (Windows) is installed by the installers when it is missing
(bootstrapper, needs the internet); WebKitGTK comes from the distribution (`.deb`
dependencies), the AppImage or the GNOME runtime (Flatpak).

Releases come from the `stable` branch, a release train fed from `main`
([ADR 0018](../adr/0018-release-train.md)):

- A pull request to `stable` builds and tests the installers (and runs
  `native.yml`); they are the run's `freekerf-windows`, `freekerf-linux`,
  `freekerf-macos` and `freekerf-flatpak` artifacts. Its first job, `version`,
  checks that `Cargo.toml`, `tauri.conf.json` and the metainfo name the same
  numeric version.
- A push to `stable` (the merge) builds and tests them again. When `v<version>` is
  not tagged yet, `publish` tags the commit that was built and attaches the
  installers and `SHA256SUMS-{windows,linux,macos,flatpak}.txt` to a **draft**
  GitHub release; a maintainer writes the notes and publishes it. A push whose
  version is already tagged only leaves the artifacts.
- A manual run (*Run workflow*) builds and tests them on any branch (artifacts
  only): the way to try a packaging change before it reaches `stable`.

To release:

1. On `main` (or in the release pull request): bump `version` in `Cargo.toml`
   (`[workspace.package]`) and `apps/desktop/src-tauri/tauri.conf.json`, add a
   `<release>` to `packaging/linux/io.github.freekerf.FreeKerf.metainfo.xml` and
   move the `[Unreleased]` notes of `CHANGELOG.md` under the version.
2. Open a pull request from `main` to `stable` and merge it with a **merge commit**
   (never squash: `stable` must keep `main`'s history, or the next train conflicts).
3. Review and publish the draft release.

The MSI needs a numeric version (`1.2.3`, or a numeric pre-release such as
`1.2.3-1`); `version` refuses `1.2.3-beta`. A fix goes to `main` first and reaches
`stable` with the next train; when `main` cannot be released, a pull request to
`stable` carries the fix, and `stable` is then merged back into `main`.

### Linux packaging files

- `packaging/linux/io.github.freekerf.FreeKerf.metainfo.xml`: AppStream metainfo
  (description, screenshots from `packaging/linux/screenshots/` on `main`, OARS,
  releases, links), shared by the three formats; checked with `appstreamcli
  validate` in the `linux` job.
- `packaging/linux/io.github.freekerf.FreeKerf.desktop.in`: the desktop entry, a
  template (`{{exec}}`, `{{icon}}`): Tauri fills it for the `.deb` and the AppImage
  (`bundle.linux.deb.desktopTemplate`, installed as `FreeKerf.desktop`), the Flatpak
  manifest fills it with `sed` (`io.github.freekerf.FreeKerf.desktop`).
- `packaging/flatpak/io.github.freekerf.FreeKerf.yml`: the manifest. It builds
  offline from `cargo-sources.json` and `node-sources.json`, generated from
  `Cargo.lock` and `ui/web/package-lock.json` by `packaging/flatpak/update-sources.sh`
  (flatpak-builder-tools at a pinned commit; Playwright's browsers are left out).
  After changing either lock file:

  ```bash
  mise exec python@3.13 -- packaging/flatpak/update-sources.sh   # then commit the two .json
  ```

  `packaging/flatpak/check-sources.py` compares them with the lock files, offline
  (crates by name, version and checksum, npm packages by URL and sha512). It runs
  in `ci.yml` (`lint`) on every pull request, so a dependency change that forgets
  them fails there, and again in the `flatpak-test` job of `release.yml`.
  Permissions: `--share=ipc --socket=wayland --socket=fallback-x11 --device=all`
  (serial ports and the GPU) `--filesystem=/run/udev:ro` (udev's port database; the port list itself comes
  from `/sys/class/tty`).

### Building them locally

Linux, after `npm ci` in `ui/web` and the WebKitGTK packages of
`.github/actions/tauri-deps`:

```bash
cargo install tauri-cli --version 2.12.3 --locked   # once
cargo build --release --locked -p freekerf-cli      # the .deb and the AppImage ship it
cd apps/desktop && cargo tauri build --bundles deb,appimage
# packages in target/release/bundle/{deb,appimage}/
```

Flatpak (with `flatpak-builder` and the Flathub remote):

```bash
flatpak-builder --user --install-deps-from=flathub --install --force-clean \
  build-dir packaging/flatpak/io.github.freekerf.FreeKerf.yml
flatpak run io.github.freekerf.FreeKerf
```

Windows:

```bash
cargo install tauri-cli --version 2.12.3 --locked   # once
cd apps/desktop && cargo tauri build --bundles msi,nsis
# installers in target/release/bundle/{msi,nsis}/
```

macOS (Xcode command line tools; both Rust targets):

```bash
cargo install tauri-cli --version 2.12.3 --locked   # once
rustup target add aarch64-apple-darwin x86_64-apple-darwin
mkdir -p target/sidecar                             # the CLI, as a sidecar per target
for t in aarch64-apple-darwin x86_64-apple-darwin; do
  cargo build --release --locked -p freekerf-cli --target $t
  cp target/$t/release/freekerf target/sidecar/freekerf-$t
done
lipo -create -output target/sidecar/freekerf-universal-apple-darwin \
  target/{aarch64,x86_64}-apple-darwin/release/freekerf
cd apps/desktop && cargo tauri build --target universal-apple-darwin --bundles app,dmg \
  --config ../../packaging/macos/tauri.cli.json
# disk image in target/universal-apple-darwin/release/bundle/dmg/
```

`packaging/macos/tauri.cli.json` adds the CLI as a Tauri sidecar (`externalBin`),
so it is signed with the app; it is not in `tauri.conf.json` because the sidecar
must exist whenever the desktop crate builds with it. `bundle.macOS` in
`tauri.conf.json` sets macOS 11 as the minimum and the ad hoc signature
(`signingIdentity: "-"`), which Apple Silicon needs to run the app at all.

### Running on macOS

- **First start**: the app is not notarized (no Apple Developer ID yet, see the
  roadmap). Drag `FreeKerf.app` to Applications and open it; macOS refuses it the
  first time (*"FreeKerf" Not Opened*: choose *Done*). Open *System Settings →
  Privacy & Security*, choose *Open Anyway* next to "FreeKerf" was blocked, choose
  *Open Anyway* again and enter an administrator's name and password (macOS 26).
  Later starts open normally, and so does the bundled CLI. Or, in a terminal:
  `xattr -dr com.apple.quarantine /Applications/FreeKerf.app`.
- **Serial ports** are `/dev/cu.*` and need no group membership.
- **MCP agents** run `/Applications/FreeKerf.app/Contents/MacOS/freekerf mcp`
  ([mcp.md](mcp.md#setup)); to have `freekerf` on the `PATH`:
  `sudo ln -s /Applications/FreeKerf.app/Contents/MacOS/freekerf /usr/local/bin/freekerf`.

### Running on Linux

- **Serial ports**: the user must be in the group that owns them, `dialout`
  (Debian, Ubuntu, Fedora) or `uucp` (Arch, openSUSE): `sudo usermod -aG dialout
  $USER`, then log out and back in. The Machine panel says so when a port is denied
  ([connection.md](connection.md#permissions-on-linux)).
- **MCP agents** run the `freekerf` CLI of the same package
  ([mcp.md](mcp.md#setup)):

  | Format | Agent command |
  | --- | --- |
  | `.deb` | `freekerf mcp` |
  | AppImage | `<dir>/squashfs-root/usr/bin/freekerf mcp`, after `FreeKerf_<version>_amd64.AppImage --appimage-extract` in `<dir>` |
  | Flatpak | `flatpak run --command=freekerf io.github.freekerf.FreeKerf mcp` |

- **Blank or flickering window on Nvidia** (WebKitGTK's DMA-BUF renderer with the
  proprietary driver): start it with `WEBKIT_DISABLE_DMABUF_RENDERER=1`, for example
  `WEBKIT_DISABLE_DMABUF_RENDERER=1 freekerf-desktop`, or for the Flatpak
  `flatpak override --user --env=WEBKIT_DISABLE_DMABUF_RENDERER=1 io.github.freekerf.FreeKerf`.

## Tests

- `apps/desktop/src-tauri/src/bridge.rs`: the window's caller (laser refused, STOP
  allowed, events forwarded as JSON).
- `ui/web`: Vitest unit and component tests (`npm test`), the architecture lint
  rules (`tests/unit/architecture.test.ts`) and the bundle check
  (`tests/unit/bundle-check.test.ts`).
- `ui/web/tests/web/` (Playwright, Chromium on `dev.html` and the fake service):
  German long strings in the three layouts, the Design mode gestures on the real
  WebGL2 canvas.
- `ui/web/tests/e2e/` (WebdriverIO + `tauri-driver`, Linux/WebKitWebDriver): the
  real app, the real service and the built-in emulator (never a serial port), a
  fresh `FREEKERF_HOME` per spec with a 1 s fire countdown. Produce: connect, console
  `$$`, jog, load a program, the fire confirmation (disabled during the countdown,
  cancel runs nothing), a job to its end, STOP during a run. Safety: `machine.send`
  called directly from the page is refused (only `fire.request` is allowed),
  Shift+Esc from a text field resets the controller, settings reach
  `settings.toml`. The flows of `design/ux.md`, one spec each:

  | Flow | Spec |
  | --- | --- |
  | 1 first run (the welcome by itself, explore with the simulator) and 2 LaserGRBL migration | `welcome.e2e.ts` (a first-run home; `FREEKERF_LASERGRBL_DIR` = the migration's fixtures, files written by LaserGRBL's code) |
  | 3 import an SVG → tune a layer → job from the design at the head, frame, burn | `job-flows.e2e.ts` |
  | 4 photo → raster settings | `design.e2e.ts` |
  | 5 text and a rectangle → subtract → array → cut layer, and 6 material test | `design-flows.e2e.ts` |
  | 7 pause, resume, STOP, recover the job from the banner | `job-flows.e2e.ts` |
  | 8 tablet controls, desktop prepares | not in the MVP: the remote client is M6 (`PLAN.md`) |

  Run:

  ```bash
  cargo install tauri-driver --locked          # once; Linux also needs webkit2gtk-driver
  cd ui/web && npm run build
  cargo build -p freekerf-desktop --features custom-protocol
  xvfb-run -a npm run test:e2e                 # or without xvfb-run on a desktop
  ```

  WebKitWebDriver cannot type into file inputs: the tests set the program with a
  `DataTransfer` on the panel's own input (the native picker is not driven).
- CI jobs `ui` (with coverage in the job summary), `desktop` (smoke run and the
  end-to-end tests), `native-check` (Windows, macOS); workflow `release` (installers, above).
