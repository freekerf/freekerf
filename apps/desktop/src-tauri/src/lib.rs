//! The FreeKerf desktop app (ADR 0006): a Tauri 2 window over
//! [`freekerf_service::Service`]. The web front end in `ui/web` reaches the app only
//! through the commands of [`commands`]; it never sees the protocol or transports.

pub mod bench;
pub mod bridge;
pub mod logging;

/// Starts the app (blocks until the last window closes).
pub fn run() {
    let home = std::env::var_os("FREEKERF_HOME").map(std::path::PathBuf::from);
    let log = logging::init(freekerf_service::Stores::log_dir(home.as_deref()).as_deref());
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        os = std::env::consts::OS,
        arch = std::env::consts::ARCH,
        pid = std::process::id(),
        log = ?log,
        "FreeKerf starting"
    );
    let bridge = match bridge::Bridge::from_env() {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("cannot start the service: {e}");
            return;
        }
    };
    let bench = bench::Bench::from_args(std::env::args().skip(1));
    let result = tauri::Builder::default()
        .manage(bridge)
        .manage(bench.clone())
        .invoke_handler(tauri::generate_handler![
            commands::dispatch,
            commands::dispatch_background,
            commands::catalog,
            commands::subscribe,
            commands::confirm_fire,
            commands::preview,
            commands::document_geometry,
            commands::answer_permission,
            commands::set_user_setting,
            commands::revoke_grants,
            commands::cancel_scripts,
            commands::subscribe_head,
            commands::bench_report
        ])
        .setup(move |app| {
            if let Some(url) = bench.start_url() {
                use tauri::Manager;
                if let Some(w) = app.get_webview_window("main") {
                    let current = w.url()?;
                    w.navigate(current.join(&url)?)?;
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!());
    match result {
        Ok(()) => tracing::info!("FreeKerf closed"),
        Err(e) => tracing::error!("the app stopped with an error: {e}"),
    }
}

/// The Tauri commands: the whole surface the web front end can call.
pub mod commands {
    use std::time::Duration;

    use freekerf_service::permissions::GrantScope;
    use freekerf_service::{ActionError, ActionSpec, CallerKind};
    use serde_json::Value;
    use tauri::ipc::{Channel, InvokeResponseBody, Response};

    use crate::bench::Bench;
    use crate::bridge::Bridge;

    /// Head frames at most this often (60 Hz).
    const HEAD_PERIOD: Duration = Duration::from_millis(16);

    /// Runs an action of the registry as the desktop user.
    ///
    /// # Errors
    /// The action failed, was refused or needs a higher tier.
    #[tauri::command]
    #[allow(clippy::needless_pass_by_value)] // Tauri passes owned arguments
    pub fn dispatch(
        bridge: tauri::State<'_, Bridge>,
        action: String,
        params: Value,
    ) -> Result<Value, ActionError> {
        bridge.dispatch(&action, params)
    }

    /// Runs a slow action ([`crate::bridge::BACKGROUND_ACTIONS`]) on a worker
    /// thread: a synchronous command runs on the main thread and blocks the window
    /// until it returns.
    ///
    /// # Errors
    /// Not a background action, or the action's error.
    #[tauri::command]
    pub async fn dispatch_background(
        app: tauri::AppHandle,
        action: String,
        params: Value,
    ) -> Result<Value, ActionError> {
        use tauri::Manager;
        tauri::async_runtime::spawn_blocking(move || {
            app.state::<Bridge>().dispatch_background(&action, params)
        })
        .await
        .map_err(|e| ActionError::failed(e.to_string()))?
    }

    /// The action catalogue (names, tiers, parameter schemas).
    #[tauri::command]
    #[allow(clippy::needless_pass_by_value)]
    pub fn catalog(bridge: tauri::State<'_, Bridge>) -> Vec<ActionSpec> {
        bridge.catalog()
    }

    /// Streams service events (JSON) to the front end until the channel closes.
    #[tauri::command]
    #[allow(clippy::needless_pass_by_value)]
    pub fn subscribe(bridge: tauri::State<'_, Bridge>, events: Channel<Value>) {
        bridge.forward_events(move |e| events.send(e).is_ok());
    }

    /// "Fire laser" was pressed: runs the pending laser request `id` if its
    /// countdown is over (PLAN §9.5). Scripts and agents have no equivalent.
    ///
    /// # Errors
    /// No such request, too early, an out-of-date job not accepted
    /// (`accept_stale`), or the action's error.
    #[tauri::command]
    #[allow(clippy::needless_pass_by_value)]
    pub fn confirm_fire(
        bridge: tauri::State<'_, Bridge>,
        id: u64,
        accept_stale: Option<bool>,
    ) -> Result<Value, ActionError> {
        bridge.confirm_fire(id, accept_stale.unwrap_or(false))
    }

    /// The tool path of the loaded program as raw bytes (empty when none is loaded).
    #[tauri::command]
    #[allow(clippy::needless_pass_by_value)]
    pub fn preview(bridge: tauri::State<'_, Bridge>) -> Response {
        Response::new(bridge.preview().map(|p| p.as_ref().clone()).unwrap_or_default())
    }

    /// The person's answer to a permission request of a script or an agent
    /// (`scope`: `run`, `session`, `always`; none: denied). Not an action of the
    /// registry: scripts and agents cannot answer for the person.
    #[tauri::command]
    #[allow(clippy::needless_pass_by_value)]
    pub fn answer_permission(bridge: tauri::State<'_, Bridge>, id: u64, scope: Option<GrantScope>) -> bool {
        bridge.answer_permission(id, scope)
    }

    /// Changes a setting as the person, user-only ones included (Automation, Safety).
    #[tauri::command]
    #[allow(clippy::needless_pass_by_value)]
    pub fn set_user_setting(
        bridge: tauri::State<'_, Bridge>,
        key: String,
        value: Value,
    ) -> Result<(), ActionError> {
        bridge.set_user_setting(&key, value)
    }

    /// Revokes the grants of a macro or an agent.
    #[tauri::command]
    #[allow(clippy::needless_pass_by_value)]
    pub fn revoke_grants(
        bridge: tauri::State<'_, Bridge>,
        kind: CallerKind,
        name: String,
    ) -> Result<usize, ActionError> {
        bridge.revoke(kind, &name)
    }

    /// Stops every running script (the person's Cancel).
    #[tauri::command]
    #[allow(clippy::needless_pass_by_value)]
    pub fn cancel_scripts(bridge: tauri::State<'_, Bridge>) {
        bridge.cancel_scripts();
    }

    /// The document's drawable geometry as raw bytes (the Design mode canvas).
    #[tauri::command]
    #[allow(clippy::needless_pass_by_value)]
    pub fn document_geometry(bridge: tauri::State<'_, Bridge>) -> Response {
        Response::new(bridge.document_geometry())
    }

    /// Streams head frames (raw bytes) at up to 60 Hz while they change.
    #[tauri::command]
    #[allow(clippy::needless_pass_by_value)]
    pub fn subscribe_head(bridge: tauri::State<'_, Bridge>, frames: Channel<InvokeResponseBody>) {
        bridge.forward_head(HEAD_PERIOD, move |f| {
            frames.send(InvokeResponseBody::Raw(f)).is_ok()
        });
    }

    /// Receives the result of the canvas benchmark (`--bench-canvas`) and quits.
    #[tauri::command]
    #[allow(clippy::needless_pass_by_value)]
    pub fn bench_report(app: tauri::AppHandle, bench: tauri::State<'_, Bench>, report: Value) {
        let code = bench.finish(&report);
        // the event loop may end the process with code 0: exit here with the result
        app.cleanup_before_exit();
        std::process::exit(code);
    }
}
