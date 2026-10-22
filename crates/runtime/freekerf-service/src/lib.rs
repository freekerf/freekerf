//! The application facade: the machine driver, the settings and the loaded
//! program behind **one action registry** (PLAN §9.1). Every front end (the CLI,
//! `host --stdio`, later the desktop UI, scripts and MCP) calls
//! [`Service::dispatch`] with an action name and JSON parameters; nothing else
//! changes the state.
//!
//! The catalogue of actions is generated into `doc/features/actions.md`.

use std::collections::{BTreeMap, VecDeque};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};
use freekerf_gcode::{AnalysisOptions, Program};
use freekerf_machine::{Machine, MachineError, MachineEvent, SystemClock};
use freekerf_protocol::Clock;
use freekerf_protocol::host::decimal;
use freekerf_protocol::{
    ConsoleRow, Event, FirmwareKind, Host, HostError, HostSnapshot, Issue, JogDirection, ResumeSuggestion,
};
use freekerf_session::actions::SessionHost;
use freekerf_session::fire::{FireGate, FireRequest, FireSummary};
use freekerf_session::session::{Change, Session, SessionEvent};
use freekerf_session::view::ViewCommand;
use freekerf_session::{NoParams, Registry};
use freekerf_settings::{MachineCache, Settings, SettingsStore};
use freekerf_transport::{EmulatorTransport, SerialTransport, Transport, TransportError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod analysis;
pub mod automation;
pub mod autotrace;
pub mod import_case;
pub mod migrate;
pub mod permissions;
pub mod preview;
pub mod remote;
pub mod script;

pub use freekerf_session::{ActionError, ActionSpec, Caller, CallerKind, Failure, FailureCause, Tier};

/// Rows kept in the console history.
pub const CONSOLE_ROWS: usize = 5000;
const OP_TIMEOUT: Duration = Duration::from_secs(30);

/// Events for front ends.
#[derive(Debug, Clone, PartialEq)]
pub enum ServiceEvent {
    /// From the machine driver.
    Machine(MachineEvent),
    /// Settings changed.
    SettingsChanged,
    /// A program was loaded.
    ProgramLoaded,
    /// The session (document, selection, view, history) changed.
    Session(SessionEvent),
    /// Agents, scripts, their actions and permission requests.
    Automation(automation::AutomationEvent),
}

/// A [`ServiceEvent`] as front ends receive it (JSON, tagged by `event`).
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum EventMessage {
    /// The link opened.
    Connected {
        /// Transport description.
        transport: String,
    },
    /// The link closed.
    Disconnected {
        /// Closed by the user (else: lost or failed).
        user: bool,
    },
    /// A protocol event.
    Host {
        /// The event.
        data: Event,
    },
    /// Settings changed.
    SettingsChanged,
    /// A program was loaded.
    ProgramLoaded,
    /// The session changed: read what changed again (`view.get`, `document.get`,
    /// `session.snapshot`).
    Session {
        /// Revision after the change.
        revision: u64,
        /// What changed.
        change: Change,
    },
    /// Agents, scripts, their actions and permission requests.
    Automation {
        /// What happened.
        data: automation::AutomationEvent,
    },
}

impl ServiceEvent {
    /// The event as JSON for front ends (`{"event": "...", ...}`, see [`EventMessage`]).
    #[must_use]
    pub fn to_json(&self) -> Value {
        serde_json::to_value(self.message()).unwrap_or(Value::Null)
    }

    /// The event in its wire shape.
    #[must_use]
    pub fn message(&self) -> EventMessage {
        match self {
            Self::SettingsChanged => EventMessage::SettingsChanged,
            Self::ProgramLoaded => EventMessage::ProgramLoaded,
            Self::Machine(MachineEvent::Connected { transport }) => EventMessage::Connected {
                transport: transport.clone(),
            },
            Self::Machine(MachineEvent::Disconnected { user }) => EventMessage::Disconnected { user: *user },
            Self::Machine(MachineEvent::Host(e)) => EventMessage::Host { data: e.clone() },
            Self::Session(e) => EventMessage::Session {
                revision: e.revision,
                change: e.change,
            },
            Self::Automation(e) => EventMessage::Automation { data: e.clone() },
        }
    }

    /// One line of text for a terminal console, when the event has one.
    #[must_use]
    pub fn console_line(&self) -> Option<String> {
        use freekerf_protocol::CommandStatus;
        match self {
            Self::Machine(MachineEvent::Host(Event::Console(row))) => Some(match row {
                ConsoleRow::Command { text, .. } => format!("> {text}"),
                ConsoleRow::Result {
                    result,
                    status,
                    decoded,
                    ..
                } => match (status, decoded) {
                    (CommandStatus::ResponseGood, _) => return None,
                    (_, Some(d)) => format!("  {result} ({d})"),
                    _ => format!("  {result}"),
                },
                ConsoleRow::Message { text, decoded, .. } => decoded.clone().unwrap_or_else(|| text.clone()),
            }),
            Self::Machine(MachineEvent::Host(Event::Issue(i))) => Some(format!("! issue: {i}")),
            Self::Machine(MachineEvent::Host(Event::StatusChanged { new, .. })) => Some(format!("# {new}")),
            Self::Machine(MachineEvent::Host(Event::JobEnded {
                lines,
                errors,
                time_ms,
            })) => Some(format!(
                "# job ended: {lines} lines, {errors} errors, {:.1} s",
                *time_ms as f64 / 1000.0
            )),
            Self::Machine(MachineEvent::Connected { transport }) => Some(format!("# connected: {transport}")),
            Self::Machine(MachineEvent::Disconnected { .. }) => Some("# disconnected".into()),
            _ => None,
        }
    }
}

/// The emulator served on a pseudo terminal (`freekerf emulate --pty`).
#[cfg(unix)]
pub mod emulator {
    pub use freekerf_transport::pty::PtyEmulator;

    /// Starts the emulator on a PTY; `timed`: motions take their real time.
    ///
    /// # Errors
    /// No PTY available.
    pub fn serve_pty(timed: bool) -> Result<PtyEmulator, String> {
        let cfg = freekerf_emulator::DeviceConfig {
            timed,
            ..freekerf_emulator::DeviceConfig::default()
        };
        freekerf_transport::pty::serve_emulator(cfg).map_err(|e| e.to_string())
    }
}

/// Where the service keeps its files (none: memory only).
#[derive(Default)]
pub struct Stores {
    /// `settings.toml`.
    pub settings: Option<Arc<dyn SettingsStore>>,
    /// Machine cache (last version, `$$`, laser counters).
    pub cache: Option<Arc<dyn SettingsStore>>,
    /// The macro folder (migrated LaserGRBL buttons).
    pub macros: Option<std::path::PathBuf>,
    /// LaserGRBL's data folder, read by the welcome flow's migration.
    pub lasergrbl: Option<std::path::PathBuf>,
    /// Installed fonts for texts (only the built-in fonts when `None`).
    pub fonts: Option<Arc<dyn freekerf_text::FontSource>>,
}

impl std::fmt::Debug for Stores {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stores")
            .field("settings", &self.settings.is_some())
            .field("cache", &self.cache.is_some())
            .field("macros", &self.macros)
            .field("lasergrbl", &self.lasergrbl)
            .field("fonts", &self.fonts.is_some())
            .finish()
    }
}

impl Stores {
    /// Files in `dir`, or in the per-user directories when `None` (`$FREEKERF_HOME`
    /// overrides them).
    ///
    /// # Errors
    /// No home directory.
    pub fn files(dir: Option<&Path>) -> Result<Self, String> {
        let dirs = match dir {
            Some(d) => freekerf_platform::Dirs::under(d),
            None => freekerf_platform::Dirs::user().map_err(|e| e.to_string())?,
        };
        Ok(Self {
            settings: Some(Arc::new(FileStore(dirs.settings_file()))),
            cache: Some(Arc::new(FileStore(dirs.data_file("machine-cache.json")))),
            macros: Some(dirs.data.join("macros")),
            lasergrbl: std::env::var_os("FREEKERF_LASERGRBL_DIR")
                .map(std::path::PathBuf::from)
                .or_else(migrate::default_lasergrbl_dir),
            fonts: Some(Arc::new(freekerf_platform::SystemFonts::new(Some(
                dirs.fonts_dir(),
            )))),
        })
    }

    /// Path of the settings file these stores use, if any.
    #[must_use]
    pub fn settings_path(dir: Option<&Path>) -> Option<std::path::PathBuf> {
        match dir {
            Some(d) => Some(freekerf_platform::Dirs::under(d).settings_file()),
            None => freekerf_platform::Dirs::user().ok().map(|d| d.settings_file()),
        }
    }
}

/// A file store (settings or cache) in the platform directories.
#[derive(Debug, Clone)]
pub struct FileStore(pub std::path::PathBuf);

impl SettingsStore for FileStore {
    fn load(&self) -> Result<Option<String>, freekerf_settings::SettingsError> {
        freekerf_platform::read_optional(&self.0)
            .map(|o| o.map(|b| String::from_utf8_lossy(&b).into_owned()))
            .map_err(|e| freekerf_settings::SettingsError::Storage(e.to_string()))
    }

    fn save(&self, text: &str) -> Result<(), freekerf_settings::SettingsError> {
        freekerf_platform::write_atomic(&self.0, text.as_bytes())
            .map_err(|e| freekerf_settings::SettingsError::Storage(e.to_string()))
    }
}

/// The loaded program.
#[derive(Debug, Clone)]
struct Loaded {
    name: String,
    program: Program,
    /// Tool path buffer ([`preview`]), built on the first request.
    preview: Option<Arc<Vec<u8>>>,
}

/// State shared by the actions.
pub struct Inner {
    machine: Machine,
    settings: Mutex<Settings>,
    stores: Stores,
    loaded: Mutex<Option<Loaded>>,
    fire: Mutex<FireGate>,
    fire_clock: Mutex<Arc<dyn Clock>>,
    console: Arc<Mutex<VecDeque<ConsoleRow>>>,
    issues: Arc<Mutex<Vec<Issue>>>,
    subscribers: Arc<Mutex<Vec<Sender<ServiceEvent>>>>,
    session: Mutex<Session>,
    /// Cancel flags of the running scripts.
    scripts: Mutex<Vec<Arc<std::sync::atomic::AtomicBool>>>,
    /// Who answers permission requests, and the session grants.
    permissions: permissions::Permissions,
    /// Agents, scripts, the action log, permission requests.
    automation: Mutex<automation::State>,
}

impl SessionHost for Inner {
    fn session(&self) -> &Mutex<Session> {
        &self.session
    }
}

impl std::fmt::Debug for Inner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Inner")
            .field("machine", &self.machine)
            .finish_non_exhaustive()
    }
}

/// The service: state plus the action registry.
#[derive(Clone)]
pub struct Service {
    inner: Arc<Inner>,
    registry: Arc<Registry<Inner>>,
    dry: Arc<Registry<script::DryContext>>,
}

impl std::fmt::Debug for Service {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Service")
            .field("registry", &self.registry)
            .finish_non_exhaustive()
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Error of service setup.
#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    /// Settings could not be loaded.
    #[error(transparent)]
    Settings(#[from] freekerf_settings::SettingsError),
}

impl Service {
    /// A service with settings loaded from `stores` (defaults when absent).
    ///
    /// # Errors
    /// The settings file is invalid or too new.
    pub fn new(stores: Stores) -> Result<Self, ServiceError> {
        let settings = match &stores.settings {
            Some(s) => Settings::load(s.as_ref())?,
            None => Settings::default(),
        };
        let cache = stores
            .cache
            .as_ref()
            .and_then(|c| c.load().ok().flatten())
            .map(|t| MachineCache::from_json(&t))
            .unwrap_or_default();
        let mut options = settings.host_options();
        options.last_known_version = cache.version.clone();
        options.last_known_settings = cache.settings.clone();
        let mut host = Host::for_kind(settings.machine.firmware.into(), options);
        if !cache.laser_life.is_empty() {
            host.set_laser_life(freekerf_protocol::LaserLife::with_counters(cache.laser_life));
        }
        let machine = Machine::start(host, Arc::new(SystemClock::default()));
        let mut session = Session::new();
        if let Some(fonts) = &stores.fonts {
            session.set_fonts(freekerf_text::Fonts::with_source(fonts.clone()));
        }
        let inner = Arc::new(Inner {
            machine,
            settings: Mutex::new(settings),
            stores,
            loaded: Mutex::new(None),
            fire: Mutex::new(FireGate::default()),
            fire_clock: Mutex::new(Arc::new(SystemClock::default())),
            console: Arc::new(Mutex::new(VecDeque::new())),
            issues: Arc::new(Mutex::new(Vec::new())),
            subscribers: Arc::new(Mutex::new(Vec::new())),
            session: Mutex::new(session),
            scripts: Mutex::new(Vec::new()),
            permissions: permissions::Permissions::default(),
            automation: Mutex::new(automation::State::default()),
        });
        start_collector(&inner);
        Ok(Self {
            inner,
            registry: Arc::new(build_registry()),
            dry: Arc::new(script::dry_registry()),
        })
    }

    /// A service in memory (no files).
    ///
    /// # Errors
    /// Never in practice.
    pub fn in_memory() -> Result<Self, ServiceError> {
        Self::new(Stores::default())
    }

    /// Runs an action.
    ///
    /// # Errors
    /// See [`ActionError`].
    pub fn dispatch(&self, caller: &Caller, action: &str, params: Value) -> Result<Value, ActionError> {
        tracing::debug!(caller = %caller.name, action, "dispatch");
        permissions::check_agent_files(caller, action, &params)?;
        permissions::check_agent_laser(caller, action)?;
        let r = self.registry.dispatch(&self.inner, caller, action, params);
        self.inner.publish_session_events();
        self.inner
            .log_action(caller, action, r.as_ref().err().map(ToString::to_string));
        r
    }

    /// The registry (specs, docs).
    #[must_use]
    pub fn registry(&self) -> &Registry<Inner> {
        &self.registry
    }

    /// Every action spec.
    #[must_use]
    pub fn specs(&self) -> Vec<ActionSpec> {
        self.registry.specs().cloned().collect()
    }

    /// Events from now on.
    #[must_use]
    pub fn subscribe(&self) -> Receiver<ServiceEvent> {
        let (tx, rx) = unbounded();
        lock(&self.inner.subscribers).push(tx);
        rx
    }

    /// Machine state.
    #[must_use]
    pub fn snapshot(&self) -> HostSnapshot {
        self.inner.machine.snapshot()
    }

    /// The tool path of the loaded program as the binary buffer of [`preview`]
    /// (built once per program), or `None` when no program is loaded. Bulk data for
    /// the canvas: front ends read it directly, not through an action.
    #[must_use]
    pub fn preview(&self) -> Option<Arc<Vec<u8>>> {
        let options = self.inner.analysis_options();
        let mut loaded = lock(&self.inner.loaded);
        let l = loaded.as_mut()?;
        Some(Arc::clone(l.preview.get_or_insert_with(|| {
            Arc::new(preview::encode(&l.program, &options))
        })))
    }

    /// The document's drawable geometry for the Design mode canvas (raw, the
    /// [`preview`] format; see [`preview::encode_document`]).
    #[must_use]
    pub fn document_geometry(&self) -> Vec<u8> {
        let doc = lock(&self.inner.session).document().clone();
        preview::encode_document(&doc)
    }

    /// Marks this app as having a person in front of it: permission requests of
    /// scripts and agents wait for their answer ([`Service::answer_permission`],
    /// [`automation::AutomationEvent::PermissionRequested`]) up to `timeout`, then
    /// count as denied.
    pub fn install_window_prompter(&self, timeout: Duration) {
        self.set_prompter(Arc::new(automation::WindowPrompter::new(
            Arc::downgrade(&self.inner),
            timeout,
        )));
    }

    /// The person's answer to a permission request (`None`: denied). Not an action:
    /// only a front end with a person calls it. Returns `false` when the request is
    /// no longer waiting.
    pub fn answer_permission(&self, id: u64, scope: Option<permissions::GrantScope>) -> bool {
        self.inner.answer_permission(id, scope)
    }

    /// Changes a user-only setting (`automation.*`, `safety.*`) or any other, as the
    /// person: not an action, so scripts and agents cannot reach it.
    ///
    /// # Errors
    /// Unknown key, invalid value, or the file cannot be saved.
    pub fn set_user_setting(&self, key: &str, value: Value) -> Result<(), ActionError> {
        lock(&self.inner.settings)
            .set(key, value, true)
            .map_err(|e| ActionError::InvalidParams(e.to_string()))?;
        self.inner.save_settings()?;
        self.inner.publish(&ServiceEvent::SettingsChanged);
        Ok(())
    }

    /// Runs the pending laser request `id` after the person confirmed it on screen
    /// (PLAN §9.5). Not an action of the registry on purpose: scripts and agents can
    /// ask for a laser action (`fire.request`) but never confirm one. Only a front
    /// end with a person in front of it calls this (the desktop window's
    /// "Fire laser" button).
    ///
    /// # Errors
    /// No such request, countdown not over, or the action's own error.
    pub fn confirm_fire(&self, id: u64) -> Result<Value, ActionError> {
        // the request must still be on screen: closing the dialog drops it
        let shown = lock(&self.inner.session)
            .view()
            .fire_request
            .as_ref()
            .map(|r| r.id);
        if shown != Some(id) {
            lock(&self.inner.fire).cancel(Some(id));
            return Err(ActionError::Refused(
                "no pending laser request with this id".into(),
            ));
        }
        let now = lock(&self.inner.fire_clock).now_ms();
        let request = lock(&self.inner.fire)
            .confirm(id, now)
            .map_err(|e| ActionError::Refused(e.to_string()))?;
        // the dialog closes whatever happens next
        self.inner.view(ViewCommand::SetFireRequest(None))?;
        tracing::info!(action = %request.action, by = %request.requested_by, "laser action confirmed");
        self.registry
            .dispatch(&self.inner, &Caller::user(), &request.action, request.params)
    }

    /// Replaces the clock of the fire confirmation countdown (tests).
    pub fn set_fire_clock(&self, clock: Arc<dyn Clock>) {
        *lock(&self.inner.fire_clock) = clock;
    }

    /// STOP without going through the registry (also bound to Ctrl-C by the CLI):
    /// cancels the running scripts, then stops the machine.
    pub fn stop(&self) {
        self.inner.cancel_scripts();
        self.inner.machine.stop();
    }
}

/// Collects console rows and issues and forwards machine events.
fn start_collector(inner: &Arc<Inner>) {
    let events = inner.machine.subscribe();
    let console = Arc::clone(&inner.console);
    let issues = Arc::clone(&inner.issues);
    let subscribers = Arc::clone(&inner.subscribers);
    let weak = Arc::downgrade(inner);
    let spawned = thread::Builder::new()
        .name("freekerf-service-events".into())
        .spawn(move || {
            for e in events {
                match &e {
                    MachineEvent::Host(Event::Console(row)) => {
                        let mut c = lock(&console);
                        c.push_back(row.clone());
                        while c.len() > CONSOLE_ROWS {
                            c.pop_front();
                        }
                    }
                    MachineEvent::Host(Event::ConsoleCleared) => lock(&console).clear(),
                    MachineEvent::Host(Event::Issue(i)) => lock(&issues).push(*i),
                    MachineEvent::Host(
                        Event::VersionDetected(_) | Event::SettingsChanged | Event::JobEnded { .. },
                    ) => {
                        if let Some(inner) = weak.upgrade() {
                            inner.save_cache();
                        }
                    }
                    _ => {}
                }
                lock(&subscribers).retain(|s| s.send(ServiceEvent::Machine(e.clone())).is_ok());
            }
        });
    if let Err(e) = spawned {
        tracing::error!("cannot start the event collector: {e}");
    }
}

impl Inner {
    fn publish(&self, e: &ServiceEvent) {
        lock(&self.subscribers).retain(|s| s.send(e.clone()).is_ok());
    }

    /// Sends the session's changes to the subscribers.
    fn publish_session_events(&self) {
        let events = lock(&self.session).drain_events();
        for e in events {
            self.publish(&ServiceEvent::Session(e));
        }
    }

    /// The planner's machine options from the settings and the board's `$30`.
    fn planner_options(&self) -> freekerf_planner::PlanOptions {
        let st = lock(&self.settings);
        let max_pwm = self
            .machine
            .call(|h, _| h.settings().max_pwm())
            .ok()
            .and_then(|d| d.to_string().parse::<f64>().ok())
            .filter(|v| *v > 0.0 && *v < f64::from(u32::MAX));
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let machine = freekerf_planner::MachineOptions {
            firmware: match st.machine.firmware {
                freekerf_settings::Firmware::Grbl => freekerf_planner::Firmware::Grbl,
                freekerf_settings::Firmware::Smoothie => freekerf_planner::Firmware::Smoothie,
                freekerf_settings::Firmware::Marlin => freekerf_planner::Firmware::Marlin,
                freekerf_settings::Firmware::VigoWork => freekerf_planner::Firmware::VigoWork,
            },
            support_pwm: st.machine.support_pwm,
            max_pwm: max_pwm.map_or(1000, |v| v as u32),
            ..freekerf_planner::MachineOptions::default()
        };
        freekerf_planner::PlanOptions {
            machine,
            ..freekerf_planner::PlanOptions::default()
        }
    }

    /// What the fire confirmation shows for a request.
    fn fire_summary(&self, action: &str, params: &Value) -> FireSummary {
        let code = params
            .get("line")
            .or_else(|| params.get("code"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        let program = match action {
            "job.run" | "job.resume" => lock(&self.loaded).as_ref().map(program_info),
            _ => None,
        };
        FireSummary {
            program: program.as_ref().map(|p| p.name.clone()),
            lines: program.as_ref().map(|p| p.lines),
            estimated_ms: program.as_ref().map(|p| p.estimated_ms),
            bounds: program.and_then(|p| p.drawing_range),
            code,
        }
    }

    /// Applies a view command to the session's view and publishes the change.
    fn view(&self, cmd: ViewCommand) -> Result<Option<u64>, ActionError> {
        let r = freekerf_session::actions::view_cmd(self, cmd);
        self.publish_session_events();
        r
    }

    fn save_cache(&self) {
        let Some(store) = &self.stores.cache else { return };
        let Ok((version, table, life)) = self.machine.call(|h, _| {
            (
                h.version().cloned(),
                h.settings().clone(),
                h.laser_life().counters.clone(),
            )
        }) else {
            return;
        };
        let cache = MachineCache {
            version,
            settings: (!table.is_empty()).then_some(table),
            laser_life: life,
            extra: BTreeMap::new(),
        };
        if let Err(e) = store.save(&cache.to_json()) {
            tracing::warn!("cannot save the machine cache: {e}");
        }
    }

    fn save_settings(&self) -> Result<(), ActionError> {
        if let Some(store) = &self.stores.settings {
            lock(&self.settings)
                .save(store.as_ref())
                .map_err(|e| ActionError::failed(e.to_string()))?;
        }
        Ok(())
    }

    fn host<R: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Host, u64) -> R + Send + 'static,
    ) -> Result<R, ActionError> {
        self.machine
            .call(f)
            .map_err(|e| ActionError::failed(e.to_string()))
    }

    fn host_result(
        &self,
        f: impl FnOnce(&mut Host, u64) -> Result<(), HostError> + Send + 'static,
    ) -> Result<(), ActionError> {
        self.host(f)?.map_err(host_err)
    }

    /// Waits for the end of a system operation started by `f`.
    fn system_op(
        &self,
        f: impl FnOnce(&mut Host, u64) -> Result<u64, HostError> + Send + 'static,
    ) -> Result<(), ActionError> {
        let events = self.machine.subscribe();
        let id = self.host(f)?.map_err(host_err)?;
        let end = Instant::now() + OP_TIMEOUT;
        while let Some(left) = end.checked_duration_since(Instant::now()) {
            match events.recv_timeout(left) {
                Ok(MachineEvent::Host(Event::SystemOpFinished { id: i, result, .. })) if i == id => {
                    return result.map_err(ActionError::failed);
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        Err(ActionError::failed("no answer from the machine"))
    }

    fn analysis_options(&self) -> AnalysisOptions {
        let support_pwm = lock(&self.settings).machine.support_pwm;
        let timing = self
            .machine
            .call(|h, _| h.settings().timing())
            .unwrap_or_default();
        AnalysisOptions { timing, support_pwm }
    }
}

#[allow(clippy::needless_pass_by_value)] // used as `map_err(connect_err)`
fn connect_err(e: MachineError) -> ActionError {
    match &e {
        MachineError::Transport(TransportError::PermissionDenied { target, group }) => {
            ActionError::Failed(Failure::with_cause(
                e.to_string(),
                FailureCause::PortPermissionDenied {
                    port: target.clone(),
                    group: group.clone(),
                },
            ))
        }
        _ => ActionError::failed(e.to_string()),
    }
}

#[allow(clippy::needless_pass_by_value)] // used as `map_err(host_err)`
fn host_err(e: HostError) -> ActionError {
    match e {
        HostError::NotConnected | HostError::NotAllowed(_) | HostError::NoProgram | HostError::Busy => {
            ActionError::Refused(e.to_string())
        }
        HostError::Unsupported(_) => ActionError::failed(e.to_string()),
    }
}

// ====================================================================== parameters

/// `machine.connect`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConnectParams {
    /// Serial port (`/dev/ttyUSB0`, `COM3`); default: the last one used.
    pub port: Option<String>,
    /// Baud rate; default: from the settings (115200).
    pub baud: Option<u32>,
    /// Connect to the built-in emulator instead of a serial port.
    #[serde(default)]
    pub emulator: bool,
}

/// `machine.jog`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JogParams {
    /// `N`, `S`, `E`, `W`, `NE`, `NW`, `SE`, `SW`, `Zup`, `Zdown` or `Home`.
    pub direction: String,
    /// Step in mm (default: the jog setting).
    pub step_mm: Option<f64>,
    /// Feed in mm/min (default: the jog setting).
    pub feed_mm_min: Option<f64>,
}

/// `machine.jog_to`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JogToParams {
    /// Target X (work coordinates, mm).
    pub x: f64,
    /// Target Y (work coordinates, mm).
    pub y: f64,
    /// Feed in mm/min (default: the jog setting).
    pub feed_mm_min: Option<f64>,
}

/// `machine.overrides`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OverrideParams {
    /// Feed override target, % (10-200).
    pub feed: i32,
    /// Rapid override target, % (25, 50 or 100).
    pub rapid: i32,
    /// Power override target, % (10-200).
    pub power: i32,
}

/// `machine.write_settings`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WriteSettingsParams {
    /// Values by setting number (`{"110": "6000"}`).
    pub settings: BTreeMap<u32, String>,
}

/// `machine.send`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SendParams {
    /// One line of G-code or a `$` command.
    pub line: String,
}

/// `machine.custom_code`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CustomCodeParams {
    /// Lines of custom code (`[expr]`, `!`, `~`, `?`, `ctrl-x`, `0xNN` allowed).
    pub code: String,
}

/// `machine.select_laser`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectLaserParams {
    /// Id of the laser life counter.
    pub id: String,
}

/// `job.load`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LoadParams {
    /// Path of a G-code file.
    pub path: String,
}

/// `job.load_text`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LoadTextParams {
    /// Name shown for the program.
    pub name: String,
    /// The G-code.
    pub gcode: String,
}

/// `job.run`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunParams {
    /// Home (`$H`) before the job.
    #[serde(default)]
    pub homing: bool,
    /// Number of passes (default 1).
    pub passes: Option<u32>,
}

/// `job.resume`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResumeParams {
    /// 0-based index of the first command to run.
    pub position: usize,
    /// Home first.
    #[serde(default)]
    pub homing: bool,
    /// Restore the work offset of the interrupted job.
    #[serde(default)]
    pub set_wco: bool,
}

/// `settings.get`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SettingsGetParams {
    /// Dotted key (`machine.baud`); all settings when absent.
    pub key: Option<String>,
}

/// Where a job made from the document starts (design/ux.md flow 3).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobStart {
    /// The document's own coordinates (default).
    #[default]
    Absolute,
    /// The anchor at the head's current position.
    CurrentPosition,
    /// The anchor at the user origin (work 0,0, `machine.set_zero`).
    UserOrigin,
}

/// `job.from_document`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct FromDocumentParams {
    /// Where the job starts (default: absolute).
    #[serde(default)]
    start: JobStart,
    /// Which point of the job's box goes to the start point (not used when absolute).
    #[serde(default)]
    anchor: freekerf_planner::origin::Anchor,
}

/// The file of `document.open` and `document.save`.
#[derive(Debug, Serialize, JsonSchema)]
struct FilePath {
    /// The file.
    path: String,
}

/// What `document.import` added.
#[derive(Debug, Serialize, JsonSchema)]
struct ImportedFile {
    /// Ids of the new objects.
    objects: Vec<u64>,
}

/// `automation.log`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct LogParams {
    /// Only the entries after this `seq`.
    #[serde(default)]
    after: Option<u64>,
}

/// `migrate.detect`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct MigrateDetectParams {
    /// LaserGRBL's data folder (default: its folder on this computer).
    #[serde(default)]
    path: Option<String>,
}

/// `migrate.run`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct MigrateRunParams {
    /// What to import.
    items: Vec<migrate::MigrationItem>,
    /// LaserGRBL's data folder (default: its folder on this computer).
    #[serde(default)]
    path: Option<String>,
}

/// `settings.set`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SettingsSetParams {
    /// Dotted key (`machine.streaming_mode`). `automation.*` is user-only.
    pub key: String,
    /// New value (JSON).
    pub value: Value,
}

/// `console.get`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConsoleParams {
    /// Only the last `last` rows.
    pub last: Option<usize>,
}

/// `machine.system_command`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SystemCommandParams {
    /// A `$` command (`$$`, `$I`, `$H`, `$X`, `$#`, `$G`, `$N=V`, `$J=...`): never turns
    /// the laser on. Other G-code goes through `machine.send` (tier `laser`).
    pub line: String,
}

/// `job.frame`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameParams {
    /// Feed in mm/min (default: the jog setting).
    pub feed_mm_min: Option<f64>,
}

/// Result of `machine.info`: what the board told about itself.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MachineInfo {
    /// Work area width (`$130`), mm.
    pub table_width_mm: f64,
    /// Work area height (`$131`), mm.
    pub table_height_mm: f64,
    /// Laser mode (`$32`).
    pub laser_mode: bool,
    /// Highest S (`$30`).
    pub max_power: f64,
    /// Homing enabled (`$22`).
    pub homing_enabled: bool,
    /// The `$$` table was read.
    pub settings_known: bool,
    /// Laser module life counters.
    pub laser_life: Vec<freekerf_protocol::LaserLifeCounter>,
    /// Index of the module in use, if known.
    pub laser_in_use: Option<usize>,
}

/// `fire.request`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FireRequestParams {
    /// An action of tier `laser` (`job.run`, `job.resume`, `machine.send`, `machine.custom_code`).
    pub action: String,
    /// Its parameters.
    #[serde(default)]
    pub params: Value,
}

/// `fire.cancel`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FireCancelParams {
    /// The request; the pending one when absent.
    pub id: Option<u64>,
}

// ====================================================================== results

/// A serial port of `machine.list_ports`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct SerialPortInfo {
    /// Port name or path (`/dev/ttyUSB0`, `COM3`).
    pub path: String,
    /// Whether this user may open it (Linux checks without opening; elsewhere
    /// always `ok`, and `machine.connect` says when it is not).
    pub access: PortAccess,
    /// Group that owns the device (Unix), when known: the group to join when
    /// access is denied.
    pub group: Option<String>,
}

/// Access to a serial port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PortAccess {
    /// May be opened.
    Ok,
    /// This user may not read and write it.
    Denied,
}

impl From<freekerf_transport::serial::PortInfo> for SerialPortInfo {
    fn from(p: freekerf_transport::serial::PortInfo) -> Self {
        Self {
            path: p.path,
            access: match p.access {
                freekerf_transport::serial::PortAccess::Ok => PortAccess::Ok,
                freekerf_transport::serial::PortAccess::Denied => PortAccess::Denied,
            },
            group: p.group,
        }
    }
}

/// Summary of the loaded program.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProgramInfo {
    /// Name.
    pub name: String,
    /// Commands.
    pub lines: usize,
    /// Estimated time, ms.
    pub estimated_ms: i64,
    /// `[xmin, ymin, xmax, ymax]` where the laser burns.
    pub drawing_range: Option<[f64; 4]>,
    /// Quadrant of the drawing.
    pub quadrant: String,
}

/// Result of `state.get`.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct State {
    /// Machine.
    pub machine: HostSnapshot,
    /// Firmware version as text (`1.1f`).
    pub firmware_version: Option<String>,
    /// Problems reported during jobs since the app started.
    pub issues: Vec<Issue>,
    /// Loaded program.
    pub program: Option<ProgramInfo>,
    /// Resume offer after an interrupted job.
    pub resume: Option<ResumeSuggestion>,
}

fn program_info(l: &Loaded) -> ProgramInfo {
    let r = freekerf_gcode::report::Report::new(&l.name, &l.program);
    ProgramInfo {
        name: l.name.clone(),
        lines: l.program.len(),
        estimated_ms: l.program.estimated_ms(),
        drawing_range: r.drawing_range,
        quadrant: r.quadrant,
    }
}

// ====================================================================== registry

#[allow(clippy::too_many_lines)] // one registration per action
fn build_registry() -> Registry<Inner> {
    let mut r: Registry<Inner> = Registry::new();
    let add = |result: Result<(), freekerf_session::RegistryError>| {
        if let Err(e) = result {
            tracing::error!("action registration: {e}");
        }
    };

    // ---- state
    add(r.register(
        "state.get",
        "Machine state, loaded program, issues and resume offer.",
        Tier::Read,
        |s: &Inner, _: NoParams| {
            let machine = s.machine.snapshot();
            let resume = s.host(|h, _| h.resume_suggestion())?;
            Ok(State {
                firmware_version: machine.version.as_ref().map(ToString::to_string),
                machine,
                issues: lock(&s.issues).clone(),
                program: lock(&s.loaded).as_ref().map(program_info),
                resume,
            })
        },
    ));
    add(r.register(
        "console.get",
        "Console rows (sent commands, answers, messages).",
        Tier::Read,
        |s: &Inner, p: ConsoleParams| {
            let c = lock(&s.console);
            let skip = p.last.map_or(0, |n| c.len().saturating_sub(n));
            Ok(c.iter().skip(skip).cloned().collect::<Vec<_>>())
        },
    ));

    // ---- settings
    add(r.register(
        "settings.get",
        "Reads the settings (all, or one dotted key).",
        Tier::Read,
        |s: &Inner, p: SettingsGetParams| {
            let st = lock(&s.settings);
            match p.key {
                Some(k) => st.get(&k).map_err(|e| ActionError::InvalidParams(e.to_string())),
                None => serde_json::to_value(&*st).map_err(|e| ActionError::failed(e.to_string())),
            }
        },
    ));
    add(r.register(
        "settings.set",
        "Changes one setting and saves the file. Automation settings are user-only and refused here.",
        Tier::Edit,
        |s: &Inner, p: SettingsSetParams| {
            let options = {
                let mut st = lock(&s.settings);
                st.set(&p.key, p.value, false)
                    .map_err(|e| ActionError::Refused(e.to_string()))?;
                st.host_options()
            };
            s.save_settings()?;
            s.host(move |h, _| {
                let mut o = options;
                o.last_known_version = h.version().cloned();
                h.set_options(o);
            })?;
            s.publish(&ServiceEvent::SettingsChanged);
            Ok(())
        },
    ));

    // ---- automation (what scripts and agents do)
    add(r.register(
        "automation.status",
        "Agents attached to the local endpoint, scripts running, permission requests waiting for the person.",
        Tier::Read,
        |s: &Inner, _: NoParams| Ok(s.automation_status()),
    ));
    add(r.register(
        "automation.grants",
        "The permissions the person gave scripts and agents for this session or always (revoked only by the person, in the app).",
        Tier::Read,
        |s: &Inner, _: NoParams| Ok(s.grant_entries()),
    ));
    add(r.register(
        "automation.log",
        "The actions scripts and agents ran or were refused, oldest first (the last 500; `after` skips the entries up to that number).",
        Tier::Read,
        |s: &Inner, p: LogParams| Ok(s.automation_log(p.after.unwrap_or(0))),
    ));

    // ---- LaserGRBL migration (the welcome flow)
    add(r.register(
        "migrate.detect",
        "What can be imported from LaserGRBL's data folder (`path`, default: LaserGRBL's folder on this computer): connection, preferences, laser counters, custom buttons, materials. Nothing is written.",
        Tier::Read,
        |s: &Inner, p: MigrateDetectParams| {
            let from = p.path.map(std::path::PathBuf::from).or_else(|| s.stores.lasergrbl.clone());
            migrate::scan(from.as_deref(), &s.stores).map_err(ActionError::failed)
        },
    ));
    add(r.register(
        "migrate.run",
        "Imports the chosen items from LaserGRBL's data folder into FreeKerf's settings, machine cache and macro folder. LaserGRBL's files are only read.",
        Tier::Edit,
        |s: &Inner, p: MigrateRunParams| {
            let from = p.path.map(std::path::PathBuf::from).or_else(|| s.stores.lasergrbl.clone());
            let macros = s
                .stores
                .macros
                .clone()
                .ok_or_else(|| ActionError::Refused("this instance has no macro folder".into()))?;
            let r = migrate::run(from.as_deref(), &s.stores, &macros, &p.items).map_err(ActionError::failed)?;
            // the settings file changed under the service: read it again
            if let Some(store) = &s.stores.settings {
                let fresh = Settings::load(store.as_ref()).map_err(|e| ActionError::failed(e.to_string()))?;
                *lock(&s.settings) = fresh;
                s.publish(&ServiceEvent::SettingsChanged);
            }
            Ok(r)
        },
    ));

    // ---- machine: connection
    add(r.register(
        "machine.list_ports",
        "Serial ports found on this computer.",
        Tier::Read,
        |_: &Inner, _: NoParams| {
            Ok(freekerf_transport::serial::available_ports()
                .into_iter()
                .map(SerialPortInfo::from)
                .collect::<Vec<_>>())
        },
    ));
    add(r.register(
        "machine.connect",
        "Connects to the machine on a serial port, or to the built-in emulator.",
        Tier::Machine,
        |s: &Inner, p: ConnectParams| {
            let (kind, options, transport) = {
                let mut st = lock(&s.settings);
                let transport: Box<dyn Transport> = if p.emulator {
                    Box::new(EmulatorTransport::default())
                } else {
                    let port = p.port.or_else(|| st.machine.port.clone()).ok_or_else(|| {
                        ActionError::InvalidParams("no port given and no port in the settings".into())
                    })?;
                    st.machine.port = Some(port.clone());
                    Box::new(SerialTransport::new(port, p.baud.unwrap_or(st.machine.baud)))
                };
                (
                    FirmwareKind::from(st.machine.firmware),
                    st.host_options(),
                    transport,
                )
            };
            let _ = s.save_settings();
            // a different firmware needs a new host (only while disconnected)
            s.host(move |h, _| {
                if h.firmware() != kind && !h.snapshot().connected {
                    let mut o = options;
                    o.last_known_version = h.version().cloned();
                    o.last_known_settings = Some(h.settings().clone());
                    let life = h.laser_life().clone();
                    *h = Host::for_kind(kind, o);
                    h.set_laser_life(life);
                }
            })?;
            s.machine.connect(transport).map_err(connect_err)
        },
    ));
    add(r.register(
        "machine.disconnect",
        "Closes the connection.",
        Tier::Machine,
        |s: &Inner, _: NoParams| {
            s.machine
                .disconnect()
                .map_err(|e| ActionError::failed(e.to_string()))
        },
    ));

    // ---- machine: safety
    add(r.register(
        "machine.stop",
        "STOP: cancels running scripts, then soft resets the board (laser off, queues flushed). Always allowed.",
        Tier::Safety,
        |s: &Inner, _: NoParams| {
            s.cancel_scripts();
            s.machine.stop();
            Ok(())
        },
    ));
    add(r.register(
        "machine.feed_hold",
        "Feed hold (pause the motion). Always allowed.",
        Tier::Safety,
        |s: &Inner, _: NoParams| s.host_result(Host::feed_hold),
    ));
    add(r.register(
        "machine.jog_stop",
        "Stops a continuous jog. Always allowed.",
        Tier::Safety,
        |s: &Inner, _: NoParams| s.host(Host::jog_abort),
    ));
    add(r.register(
        "machine.safety_door",
        "Safety door (real-time 0x84): the board holds as if its door opened; resume with machine.resume. Always allowed.",
        Tier::Safety,
        |s: &Inner, _: NoParams| s.host(Host::safety_door),
    ));

    // ---- machine: motion and settings
    add(r.register(
        "machine.resume",
        "Resumes after a feed hold or a door (cycle start).",
        Tier::Machine,
        |s: &Inner, _: NoParams| s.host_result(Host::cycle_start),
    ));
    add(r.register(
        "machine.unlock",
        "Unlocks the alarm state (`$X`).",
        Tier::Machine,
        |s: &Inner, _: NoParams| s.host_result(Host::unlock),
    ));
    add(r.register(
        "machine.home",
        "Runs the homing cycle (`$H`).",
        Tier::Machine,
        |s: &Inner, _: NoParams| s.host_result(Host::home),
    ));
    add(r.register(
        "machine.set_zero",
        "Sets the current position as work zero (`G92 X0 Y0 Z0`).",
        Tier::Machine,
        |s: &Inner, _: NoParams| s.host_result(Host::set_new_zero),
    ));
    add(r.register(
        "machine.jog",
        "Jogs one step in a direction (laser off).",
        Tier::Machine,
        |s: &Inner, p: JogParams| {
            let dir = JogDirection::parse(&p.direction)
                .ok_or_else(|| ActionError::InvalidParams(format!("unknown direction `{}`", p.direction)))?;
            let (step, feed) = {
                let st = lock(&s.settings);
                (
                    p.step_mm.unwrap_or(st.jog.step_mm),
                    p.feed_mm_min.unwrap_or(st.jog.speed_mm_min),
                )
            };
            #[allow(clippy::cast_possible_truncation)]
            let feed = feed as f32;
            s.host_result(move |h, now| h.jog(dir, feed, decimal(step), now))
        },
    ));
    add(r.register(
        "machine.jog_to",
        "Jogs to a work position (laser off).",
        Tier::Machine,
        |s: &Inner, p: JogToParams| {
            let feed = p
                .feed_mm_min
                .unwrap_or_else(|| lock(&s.settings).jog.speed_mm_min);
            #[allow(clippy::cast_possible_truncation)]
            let (x, y, feed) = (p.x as f32, p.y as f32, feed as f32);
            s.host_result(move |h, now| h.jog_to(x, y, feed, now))
        },
    ));
    add(r.register(
        "machine.overrides",
        "Sets the override targets; the host steps the board towards them.",
        Tier::Machine,
        |s: &Inner, p: OverrideParams| s.host(move |h, _| h.set_override_targets(p.feed, p.rapid, p.power)),
    ));
    add(r.register(
        "machine.read_settings",
        "Reads the board settings again (`$$`).",
        Tier::Machine,
        |s: &Inner, _: NoParams| s.system_op(Host::refresh_settings),
    ));
    add(r.register(
        "machine.write_settings",
        "Writes board settings (`$N=V`), one at a time; returns when the board answered all of them.",
        Tier::Machine,
        |s: &Inner, p: WriteSettingsParams| {
            let values: Vec<(u32, String)> = p.settings.into_iter().collect();
            s.system_op(move |h, now| h.write_settings(&values, now))
        },
    ));
    add(r.register(
        "machine.system_command",
        "Sends a `$` command (`$$`, `$I`, `$H`, `$X`, `$N=V`, `$J=...`; not during a job). It cannot turn the laser on.",
        Tier::Machine,
        |s: &Inner, p: SystemCommandParams| {
            let line = p.line.trim().to_owned();
            if !is_system_command(&line) {
                return Err(ActionError::InvalidParams(
                    "only `$` commands; other G-code needs machine.send".into(),
                ));
            }
            s.host_result(move |h, now| h.send_command(&line, now))
        },
    ));
    add(r.register(
        "machine.info",
        "Work area, laser mode, highest power and laser module counters, from the board settings.",
        Tier::Read,
        |s: &Inner, _: NoParams| {
            s.host(|h, _| {
                let st = h.settings();
                let life = h.laser_life();
                MachineInfo {
                    table_width_mm: decimal_to_f64(st.table_width()),
                    table_height_mm: decimal_to_f64(st.table_height()),
                    laser_mode: st.laser_mode(),
                    max_power: decimal_to_f64(st.max_pwm()),
                    homing_enabled: st.homing_enabled(),
                    settings_known: !st.is_empty(),
                    laser_life: life.counters.clone(),
                    laser_in_use: life.current,
                }
            })
        },
    ));
    add(r.register(
        "machine.send",
        "Sends one line of G-code or a `$` command (not during a job). It can turn the laser on.",
        Tier::Laser,
        |s: &Inner, p: SendParams| s.host_result(move |h, now| h.send_command(&p.line, now)),
    ));
    add(r.register(
        "machine.custom_code",
        "Runs custom code (buttons, macros): `[expr]` evaluated, immediate characters sent at once.",
        Tier::Laser,
        |s: &Inner, p: CustomCodeParams| s.host_result(move |h, now| h.execute_custom_code(&p.code, now)),
    ));
    add(r.register(
        "machine.select_laser",
        "Selects the laser module in use (life counters).",
        Tier::Edit,
        |s: &Inner, p: SelectLaserParams| {
            if s.host(move |h, _| h.select_laser(&p.id))? {
                Ok(())
            } else {
                Err(ActionError::InvalidParams("unknown laser id".into()))
            }
        },
    ));

    // ---- job
    // ---- document files (the session actions are registered below)
    add(r.register(
        "document.open",
        "Opens a `.fkproj` project file (the undo history starts again).",
        Tier::Edit,
        |s: &Inner, p: LoadParams| {
            let bytes = freekerf_platform::read(Path::new(&p.path))
                .map_err(|e| ActionError::failed(e.to_string()))?;
            let doc = freekerf_document::Document::from_fkproj(&bytes)
                .map_err(|e| ActionError::failed(e.to_string()))?;
            lock(&s.session).replace_document(doc);
            Ok(FilePath { path: p.path })
        },
    ));
    add(r.register(
        "document.save",
        "Saves the document as a `.fkproj` project file (written atomically).",
        Tier::Edit,
        |s: &Inner, p: LoadParams| {
            let text = lock(&s.session).document().to_fkproj();
            freekerf_platform::write_atomic(Path::new(&p.path), text.as_bytes())
                .map_err(|e| ActionError::failed(e.to_string()))?;
            lock(&s.session).mark_saved();
            Ok(FilePath { path: p.path })
        },
    ));
    add(r.register(
        "document.import",
        "Imports an SVG, DXF or image file into the document (format from the extension). One undo step.",
        Tier::Edit,
        |s: &Inner, p: LoadParams| {
            let bytes = freekerf_platform::read(Path::new(&p.path))
                .map_err(|e| ActionError::failed(e.to_string()))?;
            let ids = freekerf_session::actions::import_file(&mut lock(&s.session), &p.path, &bytes)?;
            Ok(ImportedFile { objects: ids })
        },
    ));
    add(r.register(
        "job.from_document",
        "Plans the document (vectors, then images; centerline uses the `autotrace` program) and loads it as the job, placed by `start` (absolute coordinates, the head's current position, or the user origin 0,0) and `anchor` (which point of the job's box goes there).",
        Tier::Edit,
        |s: &Inner, p: FromDocumentParams| {
            let mut doc = lock(&s.session).document().clone();
            let at = match p.start {
                JobStart::Absolute => None,
                JobStart::UserOrigin => Some((0.0, 0.0)),
                JobStart::CurrentPosition => {
                    let m = s.machine.snapshot();
                    if !m.connected {
                        return Err(ActionError::Refused(
                            "starting at the current position needs a connected machine".into(),
                        ));
                    }
                    Some((f64::from(m.work_position.x), f64::from(m.work_position.y)))
                }
            };
            if let Some(at) = at {
                freekerf_planner::origin::place(&mut doc, p.anchor, at);
            }
            let tracer = autotrace::ExternalAutotrace::default();
            let job = freekerf_planner::plan_with(&doc, &s.planner_options(), Some(&tracer))
                .map_err(|e| ActionError::failed(e.to_string()))?;
            load_program(s, "document".into(), job.lines.join("\n").as_bytes())
        },
    ));
    add(r.register(
        "job.load",
        "Loads and analyzes a G-code file.",
        Tier::Edit,
        |s: &Inner, p: LoadParams| {
            let bytes = freekerf_platform::read(Path::new(&p.path))
                .map_err(|e| ActionError::failed(e.to_string()))?;
            let name = Path::new(&p.path)
                .file_name()
                .map_or_else(|| p.path.clone(), |n| n.to_string_lossy().into_owned());
            load_program(s, name, &bytes)
        },
    ));
    add(r.register(
        "job.load_text",
        "Loads and analyzes G-code given as text.",
        Tier::Edit,
        |s: &Inner, p: LoadTextParams| load_program(s, p.name, p.gcode.as_bytes()),
    ));
    add(r.register(
        "job.info",
        "The loaded program (name, lines, estimated time, range).",
        Tier::Read,
        |s: &Inner, _: NoParams| Ok(lock(&s.loaded).as_ref().map(program_info)),
    ));
    add(r.register(
        "job.frame",
        "Runs the head around the bounds of the loaded program with the laser off (jog moves).",
        Tier::Machine,
        |s: &Inner, p: FrameParams| {
            let range = lock(&s.loaded)
                .as_ref()
                .and_then(|l| program_info(l).drawing_range)
                .ok_or_else(|| ActionError::Refused("no program with a drawing loaded".into()))?;
            let feed = p
                .feed_mm_min
                .unwrap_or_else(|| lock(&s.settings).jog.speed_mm_min);
            #[allow(clippy::cast_possible_truncation)]
            let (x0, y0, x1, y1, feed) = (
                range[0] as f32,
                range[1] as f32,
                range[2] as f32,
                range[3] as f32,
                feed as f32,
            );
            s.host_result(move |h, now| {
                for (x, y) in [(x0, y0), (x1, y0), (x1, y1), (x0, y1), (x0, y0)] {
                    h.jog_to(x, y, feed, now)?;
                }
                Ok(())
            })
        },
    ));
    add(r.register(
        "job.run",
        "Runs the loaded program from the start (header, program, footer, passes). Fires the laser.",
        Tier::Laser,
        |s: &Inner, p: RunParams| {
            let passes = p.passes.unwrap_or(1);
            s.host_result(move |h, now| {
                h.set_loop_count(passes);
                h.run_program(p.homing, now)
            })
        },
    ));
    add(r.register(
        "job.resume",
        "Runs the loaded program from a position, rebuilding position, power and modes first. Fires the laser.",
        Tier::Laser,
        |s: &Inner, p: ResumeParams| s.host_result(move |h, now| h.resume_program(p.position, p.homing, p.set_wco, now)),
    ));
    add(r.register(
        "job.abort",
        "Stops streaming the job and sends `M5` (lines already in the board still run; use machine.stop to stop at once). Always allowed.",
        Tier::Safety,
        |s: &Inner, _: NoParams| s.host_result(Host::abort_program),
    ));
    add(freekerf_session::actions::register_session_actions(&mut r));
    register_fire(&mut r);
    describe_results(&mut r);
    r
}

/// `fire.request` and `fire.cancel` (PLAN §9.5). Confirming is
/// [`Service::confirm_fire`], not an action.
fn register_fire(r: &mut Registry<Inner>) {
    let laser: Vec<String> = r
        .specs()
        .filter(|s| s.tier == Tier::Laser)
        .map(|s| s.name.clone())
        .collect();
    let add = |result: Result<(), freekerf_session::RegistryError>| {
        if let Err(e) = result {
            tracing::error!("action registration: {e}");
        }
    };
    add(r.register(
        "fire.request",
        "Asks to run a laser action: it opens the fire confirmation (checklist and countdown) and runs only when a person confirms on screen. Returns the pending request.",
        Tier::Machine,
        move |s: &Inner, p: FireRequestParams| {
            if !laser.contains(&p.action) {
                return Err(ActionError::InvalidParams(format!(
                    "`{}` is not a laser action ({})",
                    p.action,
                    laser.join(", ")
                )));
            }
            let summary = s.fire_summary(&p.action, &p.params);
            let countdown_ms = lock(&s.settings).safety.fire_countdown_s.saturating_mul(1000);
            let now = lock(&s.fire_clock).now_ms();
            let request = lock(&s.fire).request(&p.action, p.params, "desktop", summary, countdown_ms, now);
            s.view(ViewCommand::SetFireRequest(Some(request.clone())))?;
            Ok(request)
        },
    ));
    add(r.register(
        "fire.cancel",
        "Cancels the pending laser request (the one given, or any). Always allowed.",
        Tier::Safety,
        |s: &Inner, p: FireCancelParams| {
            if lock(&s.fire).cancel(p.id) {
                s.view(ViewCommand::SetFireRequest(None))?;
            }
            Ok(())
        },
    ));
}

/// Result schemas of the actions whose handlers return any JSON, used to generate
/// the front-end types (`cargo xtask gen-ts`); the others are typed from their
/// handler. `settings.get` stays unknown (a setting's value is any JSON).
fn describe_results(r: &mut Registry<Inner>) {
    let add = |result: Result<(), freekerf_session::RegistryError>| {
        if let Err(e) = result {
            tracing::error!("result description: {e}");
        }
    };
    add(r.describe_result::<State>("state.get"));
    add(r.describe_result::<Vec<ConsoleRow>>("console.get"));
    add(r.describe_result::<Vec<SerialPortInfo>>("machine.list_ports"));
    add(r.describe_result::<ProgramInfo>("job.load"));
    add(r.describe_result::<ProgramInfo>("job.load_text"));
    add(r.describe_result::<Option<ProgramInfo>>("job.info"));
    add(r.describe_result::<MachineInfo>("machine.info"));
    add(r.describe_result::<FireRequest>("fire.request"));
    for name in [
        "settings.set",
        "machine.connect",
        "machine.disconnect",
        "machine.stop",
        "machine.feed_hold",
        "machine.jog_stop",
        "machine.safety_door",
        "machine.resume",
        "machine.unlock",
        "machine.home",
        "machine.set_zero",
        "machine.jog",
        "machine.jog_to",
        "machine.overrides",
        "machine.read_settings",
        "machine.write_settings",
        "machine.send",
        "machine.system_command",
        "machine.custom_code",
        "machine.select_laser",
        "job.frame",
        "fire.cancel",
        "job.run",
        "job.resume",
        "job.abort",
    ] {
        add(r.describe_result::<()>(name));
    }
}

/// A `$` command (never turns the laser on).
fn is_system_command(line: &str) -> bool {
    line.starts_with('$') && !line.contains(['\n', '\r'])
}

fn decimal_to_f64(d: freekerf_gcode::Decimal) -> f64 {
    freekerf_gcode::number::f64_from_decimal(d)
}

fn load_program(s: &Inner, name: String, bytes: &[u8]) -> Result<ProgramInfo, ActionError> {
    let program = Program::parse_bytes(bytes, &s.analysis_options());
    let copy = program.clone();
    s.host_result(move |h, _| h.set_program(copy))?;
    let loaded = Loaded {
        name,
        program,
        preview: None,
    };
    let info = program_info(&loaded);
    *lock(&s.loaded) = Some(loaded);
    s.publish(&ServiceEvent::ProgramLoaded);
    Ok(info)
}
