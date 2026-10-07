//! The host state machine (port of LaserGRBL's `GrblCore`), sans-IO.
//!
//! The caller owns the transport and the clock. It reports what happened
//! ([`Host::open`], [`Host::receive`], [`Host::close`]), calls [`Host::tick`] when
//! [`Host::next_deadline`] is reached (or at any time), and drains
//! [`Host::poll_output`] (bytes to write, close requests) and [`Host::poll_event`].
//! Every method takes the current monotonic time in milliseconds, so the whole
//! protocol is tested with a simulated clock and no threads.

use std::collections::VecDeque;

use freekerf_gcode::number::decimal_from_f64;
use freekerf_gcode::{Command, Decimal, MachineState, Program};
use serde::{Deserialize, Serialize};

use crate::console::{CommandStatus, ConsoleRow, MessageKind, decode_message, decode_result};
use crate::expr::{Variables, evaluate_brackets};
use crate::firmware::{Firmware, FirmwareKind, LineHandled};
use crate::grbl_settings::GrblSettings;
use crate::jog::{JogDirection, edge_axes, format_coord, format_speed, step_axes};
use crate::life::{LaserLife, Selection};
use crate::projection::{Issue, TimeProjection};
use crate::status::{MachineStatus, Point3, StatusField, parse_report, report_version};
use crate::version::FirmwareVersion;

/// Default custom header (`GCODE_STD_HEADER`).
pub const DEFAULT_HEADER: &str = "G90 (use absolute coordinates)";
/// Default code between passes (`GCODE_STD_PASSES`).
pub const DEFAULT_PASSES: &str = ";(Uncomment if you want to sink Z axis)\n;G91 (use relative coordinates)\n;G0 Z-1 (sinks the Z axis, 1mm)\n;G90 (use absolute coordinates)";
/// Default custom footer (`GCODE_STD_FOOTER`).
pub const DEFAULT_FOOTER: &str = "G0 X0 Y0 Z0 (move back to origin)";

/// RX buffer size assumed until the board tells otherwise.
pub const DEFAULT_BUFFER_SIZE: usize = 127;
const CONNECT_TIMEOUT_MS: u64 = 10_000;
const REFRESH_DELAY_MS: u64 = 500;
const QUERY_TIMEOUT_MS: u64 = 10_000;
const COLLECT_QUIET_MS: u64 = 500;
const COLLECT_MAX_MS: u64 = 5_000;
const STUCK_MS: u64 = 10_000;

/// How lines are streamed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StreamingMode {
    /// Fill the board's RX buffer (character counting).
    Buffered,
    /// One line at a time.
    Synchronous,
    /// One line at a time; a job line answered with an error is sent again (3 times).
    RepeatOnError,
}

impl StreamingMode {
    /// Parses `Buffered`, `Synchronous` or `RepeatOnError`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "Buffered" => Some(Self::Buffered),
            "Synchronous" => Some(Self::Synchronous),
            "RepeatOnError" => Some(Self::RepeatOnError),
            _ => None,
        }
    }
}

/// Automatic cooling: hold the job `off_ms` every `on_ms` of work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoCooling {
    /// Working time between pauses.
    pub on_ms: u64,
    /// Pause length.
    pub off_ms: u64,
}

/// Options that change what goes on the wire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostOptions {
    /// Streaming mode (Smoothie and Marlin always stream synchronously).
    pub streaming_mode: StreamingMode,
    /// Soft reset (ctrl-x) right after opening the port.
    pub reset_on_connect: bool,
    /// Ask `$I` after connecting.
    pub query_machine_info: bool,
    /// Continuous jog (jog until released).
    pub continuous_jog: bool,
    /// Status query period, ms (LaserGRBL "Fast" threading mode: 500).
    pub status_query_ms: u64,
    /// Code sent before a job.
    pub custom_header: String,
    /// Code sent between passes.
    pub custom_passes: String,
    /// Code sent after a job.
    pub custom_footer: String,
    /// "Support Hardware PWM" (S0 does not burn).
    pub support_pwm: bool,
    /// Automatic cooling cycles, off when `None`.
    pub auto_cooling: Option<AutoCooling>,
    /// Jog step of the jog panel, mm (`[jogstep]` in custom code).
    pub jog_step_mm: f64,
    /// Jog speed of the jog panel, mm/min (`[jogspeed]` in custom code).
    pub jog_speed_mm_min: f64,
    /// Version of the last board seen (used until the banner arrives).
    pub last_known_version: Option<FirmwareVersion>,
    /// Settings table of the last board seen.
    pub last_known_settings: Option<GrblSettings>,
}

impl Default for HostOptions {
    fn default() -> Self {
        Self {
            streaming_mode: StreamingMode::Buffered,
            reset_on_connect: true,
            query_machine_info: true,
            continuous_jog: false,
            status_query_ms: 500,
            custom_header: DEFAULT_HEADER.to_owned(),
            custom_passes: DEFAULT_PASSES.to_owned(),
            custom_footer: DEFAULT_FOOTER.to_owned(),
            support_pwm: true,
            auto_cooling: None,
            jog_step_mm: 1.0,
            jog_speed_mm_min: 1000.0,
            last_known_version: None,
            last_known_settings: None,
        }
    }
}

/// Something the caller must do on the transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Output {
    /// Write these bytes.
    Write(Vec<u8>),
    /// Close the connection (connection timeout).
    Close,
}

/// Where a queued command came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Origin {
    /// Typed by the user, a button, a jog, custom code.
    Manual,
    /// A line of the loaded program.
    Job,
    /// A settings/info query or write ([`Host::write_settings`]).
    System(u64),
}

/// The kind of a system operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SystemOpKind {
    /// `$$`.
    ReadSettings,
    /// `$I`.
    ReadInfo,
    /// `$N=V` lines.
    WriteSettings,
    /// Wi-Fi configuration commands.
    WriteWifi,
}

/// What happened, for the front ends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Event {
    /// The machine state changed.
    StatusChanged {
        /// Before.
        old: MachineStatus,
        /// After.
        new: MachineStatus,
    },
    /// A problem during a job (user actions are not reported).
    Issue(Issue),
    /// The firmware version was detected.
    VersionDetected(FirmwareVersion),
    /// Current overrides changed (feed, rapids, power in %).
    OverridesChanged([i32; 3]),
    /// A console row.
    Console(ConsoleRow),
    /// The console was cleared (a job started from the beginning).
    ConsoleCleared,
    /// A job started (`target` commands queued).
    JobStarted {
        /// Commands queued.
        target: usize,
    },
    /// A pass of a multi-pass job ended; `remaining` passes left.
    PassEnded {
        /// Passes left.
        remaining: u32,
    },
    /// The job ended.
    JobEnded {
        /// Commands in the program.
        lines: usize,
        /// Errors reported by the board.
        errors: usize,
        /// Duration of the job.
        time_ms: u64,
    },
    /// A system operation finished.
    SystemOpFinished {
        /// Id returned when it was requested.
        id: u64,
        /// What it was.
        kind: SystemOpKind,
        /// `Ok` or the reason it failed (errors listed one per line).
        result: Result<(), String>,
    },
    /// The settings table changed (read or written).
    SettingsChanged,
    /// Several laser modules could be in use: the user must pick one.
    LaserSelectionNeeded,
    /// The board stopped acknowledging lines and reported an empty buffer: the
    /// host acknowledged `count` lines itself.
    BufferStuckUnlocked {
        /// Lines acknowledged.
        count: usize,
    },
    /// An IP address reported by a Wi-Fi board.
    IpDetected(String),
    /// The loop count changed.
    LoopCountChanged(u32),
}

/// Why a request was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HostError {
    /// Not connected.
    #[error("not connected")]
    NotConnected,
    /// Not allowed in the current state.
    #[error("not allowed now: {0}")]
    NotAllowed(&'static str),
    /// No program loaded.
    #[error("no program loaded")]
    NoProgram,
    /// The connect-time queries are still running.
    #[error("busy reading the machine configuration")]
    Busy,
    /// The operation is not supported by this firmware or version.
    #[error("not supported: {0}")]
    Unsupported(&'static str),
}

#[derive(Debug, Clone)]
struct Queued {
    cmd: Command,
    offset_ms: i64,
    origin: Origin,
}

#[derive(Debug, Clone)]
struct Pending {
    q: Queued,
    row: u64,
    wire_len: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpPhase {
    Waiting,
    Collecting { ok_at: u64, last_row_at: u64 },
}

#[derive(Debug, Clone)]
struct SystemOp {
    id: u64,
    kind: SystemOpKind,
    commands: Vec<Command>,
    started: Option<u64>,
    phase: OpPhase,
    rows: Vec<String>,
    answered: usize,
    errors: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    Main,
    System,
    Retry,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum JogTarget {
    Abort,
    Direction(JogDirection, f32),
    Position(f32, f32, f32),
}

/// Information to offer a job resume after an interruption.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResumeSuggestion {
    /// Commands executed.
    pub executed: usize,
    /// Commands sent.
    pub sent: usize,
    /// Commands in the job.
    pub target: usize,
    /// What interrupted it.
    pub issue: Issue,
    /// A homing cycle is advisable (position may be lost).
    pub homing: bool,
    /// The work offset was lost and can be restored.
    pub set_wco: bool,
}

/// Mutable state shared with the firmware hooks.
#[derive(Debug)]
pub struct Core {
    opts: HostOptions,
    now: u64,
    link_open: bool,
    status: MachineStatus,
    version: Option<FirmwareVersion>,
    settings: GrblSettings,
    queue: VecDeque<Queued>,
    retry: Option<Queued>,
    pending: VecDeque<Pending>,
    used_buffer: usize,
    auto_buffer: usize,
    fixed_buffer: Option<usize>,
    mpos: Point3,
    wco: Point3,
    grbl_blocks: i32,
    grbl_buffer: i32,
    failed_connections: u32,
    tp: TimeProjection,
    cur_f: f32,
    cur_s: f32,
    ov_cur: [i32; 3],
    ov_target: [i32; 3],
    loop_count: u32,
    hold_by_user: bool,
    hold_by_cooling: bool,
    now_cooling: bool,
    connect_start: u64,
    last_status_at: u64,
    last_activity_at: u64,
    query_due: Option<u64>,
    refresh_at: Option<u64>,
    sys: VecDeque<SystemOp>,
    sys_cmds: VecDeque<Queued>,
    next_op_id: u64,
    program: Program,
    welcome_seen: bool,
    vendor_info_seen: Option<String>,
    vendor_version_seen: Option<String>,
    detected_ip: Option<String>,
    life: LaserLife,
    cj_curr: Option<(u64, JogTarget)>,
    cj_prev: Option<(u64, JogTarget)>,
    cj_seq: u64,
    next_row: u64,
    rx_partial: Vec<u8>,
    out: VecDeque<Output>,
    events: VecDeque<Event>,
}

impl Core {
    fn new(opts: HostOptions) -> Self {
        let version = opts.last_known_version.clone();
        let settings = opts.last_known_settings.clone().unwrap_or_default();
        Self {
            opts,
            now: 0,
            link_open: false,
            status: MachineStatus::Disconnected,
            version,
            settings,
            queue: VecDeque::new(),
            retry: None,
            pending: VecDeque::new(),
            used_buffer: 0,
            auto_buffer: DEFAULT_BUFFER_SIZE,
            fixed_buffer: None,
            mpos: Point3::ZERO,
            wco: Point3::ZERO,
            grbl_blocks: -1,
            grbl_buffer: -1,
            failed_connections: 0,
            tp: TimeProjection::default(),
            cur_f: 0.0,
            cur_s: 0.0,
            ov_cur: [100; 3],
            ov_target: [100; 3],
            loop_count: 1,
            hold_by_user: false,
            hold_by_cooling: false,
            now_cooling: false,
            connect_start: 0,
            last_status_at: 0,
            last_activity_at: 0,
            query_due: None,
            refresh_at: None,
            sys: VecDeque::new(),
            sys_cmds: VecDeque::new(),
            next_op_id: 1,
            program: Program::new(),
            welcome_seen: false,
            vendor_info_seen: None,
            vendor_version_seen: None,
            detected_ip: None,
            life: LaserLife::default(),
            cj_curr: None,
            cj_prev: None,
            cj_seq: 0,
            next_row: 1,
            rx_partial: Vec::new(),
            out: VecDeque::new(),
            events: VecDeque::new(),
        }
    }

    // ------------------------------------------------------------------ helpers for hooks

    /// Current time (ms).
    #[must_use]
    pub const fn now(&self) -> u64 {
        self.now
    }

    /// Current machine state.
    #[must_use]
    pub const fn status(&self) -> MachineStatus {
        self.status
    }

    /// True between job start and end.
    #[must_use]
    pub const fn in_program(&self) -> bool {
        self.tp.in_program()
    }

    /// Last activity (a response or a movement), ms.
    #[must_use]
    pub const fn last_activity_at(&self) -> u64 {
        self.last_activity_at
    }

    /// Last status report, ms.
    #[must_use]
    pub const fn last_status_at(&self) -> u64 {
        self.last_status_at
    }

    /// Status query period, ms.
    #[must_use]
    pub const fn query_period(&self) -> u64 {
        self.opts.status_query_ms
    }

    /// Writes a real-time byte when the port is open (`SendImmediate`).
    pub fn send_immediate(&mut self, b: u8) {
        if self.link_open {
            self.out.push_back(Output::Write(vec![b]));
        }
    }

    /// Writes raw text when the port is open.
    pub fn write_raw(&mut self, text: &str) {
        if self.link_open {
            self.out.push_back(Output::Write(text.as_bytes().to_vec()));
        }
    }

    /// Queues a command (`EnqueueCommand`).
    pub fn enqueue(&mut self, cmd: Command) {
        self.queue.push_back(Queued {
            cmd,
            offset_ms: 0,
            origin: Origin::Manual,
        });
    }

    /// Runs custom code: immediate characters are sent at once, lines are queued,
    /// `[expr]` are evaluated (`ExecuteCustomCode`).
    pub fn execute_custom_code(&mut self, code: &str) {
        let vars = self.variables();
        for line in code.trim().split('\n') {
            if line.trim().is_empty() {
                continue;
            }
            let tosend = evaluate_brackets(line, &vars);
            if let Some(b) = immediate_byte(&tosend) {
                if b == b'!' {
                    self.hold_by_user = true;
                }
                self.send_immediate(b);
            } else {
                self.enqueue(Command::preserving_case(&tosend));
            }
        }
    }

    /// The header custom code.
    #[must_use]
    pub fn header(&self) -> String {
        self.opts.custom_header.clone()
    }

    /// The footer custom code.
    #[must_use]
    pub fn footer(&self) -> String {
        self.opts.custom_footer.clone()
    }

    /// Writes a command and counts its bytes in the RX buffer (`SendToSerial`).
    pub fn write_counted(&mut self, cmd: &Command) {
        self.used_buffer += cmd.wire_len();
        self.out.push_back(Output::Write(cmd.wire().into_bytes()));
    }

    /// Writes a command without counting it (Vigo outside a job).
    pub fn write_uncounted(&mut self, cmd: &Command) {
        self.out.push_back(Output::Write(cmd.wire().into_bytes()));
    }

    /// Sets the position from a firmware-specific report.
    pub fn set_machine_position(&mut self, p: Point3) {
        if p != self.mpos {
            self.mpos = p;
            self.last_activity_at = self.now;
        }
    }

    /// Sets feed and power.
    pub fn set_feed_speed(&mut self, f: f32, s: f32) {
        self.life.on_power(self.cur_s, self.settings.max_pwm(), self.now);
        self.cur_f = f;
        self.cur_s = s;
    }

    /// Marks the last status report time.
    pub fn status_seen(&mut self) {
        self.last_status_at = self.now;
    }

    // ------------------------------------------------------------------ state

    fn variables(&self) -> Variables {
        let d = self.program.range().drawing;
        let (left, right, top, bottom) = match d.as_box() {
            Some([x0, y0, x1, y1]) => (x0, x1, y1, y0),
            None => (Decimal::ZERO, Decimal::ZERO, Decimal::ZERO, Decimal::ZERO),
        };
        let f = freekerf_gcode::number::f64_from_decimal;
        let wpos = self.mpos - self.wco;
        let mut v: Variables = [
            ("left", f(left)),
            ("right", f(right)),
            ("top", f(top)),
            ("bottom", f(bottom)),
            ("width", f(right - left)),
            ("height", f(top - bottom)),
            ("jogstep", self.opts.jog_step_mm),
            ("jogspeed", self.opts.jog_speed_mm_min),
            ("WCO.X", f64::from(self.wco.x)),
            ("WCO.Y", f64::from(self.wco.y)),
            ("WCO.Z", f64::from(self.wco.z)),
            ("MPos.X", f64::from(self.mpos.x)),
            ("MPos.Y", f64::from(self.mpos.y)),
            ("MPos.Z", f64::from(self.mpos.z)),
            // LaserGRBL gives the machine position here; FreeKerf the work position (DIV-101)
            ("WPos.X", f64::from(wpos.x)),
            ("WPos.Y", f64::from(wpos.y)),
            ("WPos.Z", f64::from(wpos.z)),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect();
        for (n, value) in self.settings.iter() {
            if let Ok(x) = value.trim().parse::<f64>() {
                v.insert(format!("${n}"), x);
            }
        }
        v.entry("$30".to_owned()).or_insert(1000.0);
        v
    }

    fn log(&mut self, kind: MessageKind, text: &str, decoded: Option<String>) {
        self.collect_row(text);
        self.events.push_back(Event::Console(ConsoleRow::Message {
            kind,
            text: text.to_owned(),
            decoded,
        }));
    }

    fn collect_row(&mut self, text: &str) {
        let now = self.now;
        if let Some(op) = self.sys.front_mut()
            && op.started.is_some()
            && matches!(op.kind, SystemOpKind::ReadSettings | SystemOpKind::ReadInfo)
        {
            op.rows.push(text.to_owned());
            if let OpPhase::Collecting { last_row_at, .. } = &mut op.phase {
                *last_row_at = now;
            }
        }
    }

    fn supports_csv(&self) -> bool {
        self.version.as_ref().is_some_and(FirmwareVersion::is_v11)
    }

    fn generic_message(&mut self, line: &str) {
        let decoded = if self.supports_csv() {
            decode_message(line)
        } else {
            None
        };
        self.log(MessageKind::classify(line), line, decoded);
    }

    fn set_issue(&mut self, issue: Issue) {
        self.tp.job_issue(issue);
        if !issue.is_user_action() {
            self.events.push_back(Event::Issue(issue));
        }
    }

    fn buffer_size(&self) -> usize {
        self.fixed_buffer.unwrap_or(self.auto_buffer)
    }

    fn is_connected(&self) -> bool {
        !matches!(
            self.status,
            MachineStatus::Disconnected | MachineStatus::Connecting
        )
    }

    fn can_read_write_config(&self) -> bool {
        self.is_connected()
            && !self.in_program()
            && matches!(self.status, MachineStatus::Idle | MachineStatus::Alarm)
    }

    fn clear_queue(&mut self, sent: bool) {
        self.queue.clear();
        self.pending.clear();
        self.retry = None;
        if sent {
            self.events.push_back(Event::ConsoleCleared);
        }
    }

    fn reset_overrides(&mut self) {
        self.ov_cur = [100; 3];
        self.ov_target = [100; 3];
        self.events.push_back(Event::OverridesChanged(self.ov_cur));
    }

    fn enlarge_buffer(&mut self, value: i32, force: bool) {
        let Ok(value) = usize::try_from(value) else { return };
        if force {
            self.auto_buffer = value;
        } else if self.auto_buffer == DEFAULT_BUFFER_SIZE && matches!(value, 128 | 254 | 255 | 256 | 10240) {
            // only known sizes, and only the first time (noise on the line)
            self.auto_buffer = value;
        }
    }

    fn set_wco(&mut self, wco: Point3) {
        self.wco = wco;
        self.tp.set_last_known_wco(wco);
    }

    fn manage_overrides(&mut self) {
        let [lin, rap, pow] = self.ov_cur;
        let [tlin, trap, tpow] = self.ov_target;
        if tlin == 100 && lin != 100 {
            self.send_immediate(0x90);
        } else if tlin - lin >= 10 {
            self.send_immediate(0x91);
        } else if lin - tlin >= 10 {
            self.send_immediate(0x92);
        } else if tlin - lin >= 1 {
            self.send_immediate(0x93);
        } else if lin - tlin >= 1 {
            self.send_immediate(0x94);
        }
        if tpow == 100 && pow != 100 {
            self.send_immediate(0x99);
        } else if tpow - pow >= 10 {
            self.send_immediate(0x9A);
        } else if pow - tpow >= 10 {
            self.send_immediate(0x9B);
        } else if tpow - pow >= 1 {
            self.send_immediate(0x9C);
        } else if pow - tpow >= 1 {
            self.send_immediate(0x9D);
        }
        if trap == 100 && rap != 100 {
            self.send_immediate(0x95);
        } else if trap == 50 && rap != 50 {
            self.send_immediate(0x96);
        } else if trap == 25 && rap != 25 {
            self.send_immediate(0x97);
        }
    }
}

/// `ctrl-x`, `?`, `!`, `~` or a `0xNN` byte in custom code.
fn immediate_byte(code: &str) -> Option<u8> {
    match code {
        "?" => Some(b'?'),
        "!" => Some(b'!'),
        "~" => Some(b'~'),
        _ if code.eq_ignore_ascii_case("ctrl-x") => Some(0x18),
        _ => {
            let hex = code.strip_prefix("0x").or_else(|| code.strip_prefix("0X"))?;
            u8::from_str_radix(hex, 16).ok()
        }
    }
}

/// The host: protocol state machine plus the firmware variant.
pub struct Host {
    core: Core,
    fw: Box<dyn Firmware>,
}

impl std::fmt::Debug for Host {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Host")
            .field("firmware", &self.fw.kind())
            .field("core", &self.core)
            .finish()
    }
}

impl Host {
    /// A host for `firmware`.
    #[must_use]
    pub fn new(firmware: Box<dyn Firmware>, options: HostOptions) -> Self {
        let mut core = Core::new(options);
        core.fixed_buffer = firmware.fixed_buffer_size();
        Self { core, fw: firmware }
    }

    /// A host for a known firmware kind.
    #[must_use]
    pub fn for_kind(kind: FirmwareKind, options: HostOptions) -> Self {
        Self::new(kind.create(), options)
    }

    /// Firmware kind.
    #[must_use]
    pub fn firmware(&self) -> FirmwareKind {
        self.fw.kind()
    }

    /// Changes the options (takes effect for the next lines sent).
    pub fn set_options(&mut self, options: HostOptions) {
        self.core.opts = options;
    }

    /// Current options.
    #[must_use]
    pub const fn options(&self) -> &HostOptions {
        &self.core.opts
    }

    // ================================================================== link

    /// The transport was opened (`OpenCom` + start of the TX loop).
    pub fn open(&mut self, now: u64) {
        let c = &mut self.core;
        c.now = now;
        c.auto_buffer = DEFAULT_BUFFER_SIZE;
        c.rx_partial.clear();
        self.set_status(MachineStatus::Connecting);
        let c = &mut self.core;
        c.connect_start = now;
        c.last_status_at = now;
        c.last_activity_at = now;
        c.link_open = true;
        if self.fw.internal_reset_on_start() {
            let reset = self.core.opts.reset_on_connect;
            self.internal_reset(reset);
        }
        self.fw.initialize_board(&mut self.core);
        self.core.query_due = Some(now + self.core.opts.status_query_ms);
    }

    /// Opening the transport failed.
    pub fn open_failed(&mut self, now: u64) {
        self.core.now = now;
        self.set_status(MachineStatus::Connecting);
        self.set_status(MachineStatus::Disconnected);
    }

    /// The transport was closed, by the user (`user`) or because it failed.
    pub fn close(&mut self, user: bool, now: u64) {
        self.core.now = now;
        if self.core.tp.last_issue() == Issue::Unknown
            && self.core.status == MachineStatus::Run
            && self.core.in_program()
        {
            self.core.set_issue(if user {
                Issue::ManualDisconnect
            } else {
                Issue::UnexpectedDisconnect
            });
        }
        let c = &mut self.core;
        c.link_open = false;
        c.used_buffer = 0;
        c.tp.job_end(now);
        c.query_due = None;
        c.refresh_at = None;
        c.clear_queue(false);
        c.fail_system_ops("disconnected");
        c.life.on_disconnect();
        self.set_status(MachineStatus::Disconnected);
    }

    /// Bytes received from the board.
    pub fn receive(&mut self, data: &[u8], now: u64) {
        self.core.now = now;
        for &b in data {
            if b == b'\n' {
                let raw = std::mem::take(&mut self.core.rx_partial);
                let text = String::from_utf8_lossy(&raw);
                let line = text.trim_end_matches(['\r', '\n', ' ']).trim();
                if !line.is_empty() {
                    let line = line.to_owned();
                    self.manage_received_line(&line);
                    self.handle_missing_ok();
                }
            } else {
                self.core.rx_partial.push(b);
            }
        }
        self.pump(now);
    }

    /// Next output for the transport.
    pub fn poll_output(&mut self) -> Option<Output> {
        self.core.out.pop_front()
    }

    /// Next event.
    pub fn poll_event(&mut self) -> Option<Event> {
        self.core.events.pop_front()
    }

    /// When [`Host::tick`] must run next at the latest.
    #[must_use]
    pub fn next_deadline(&self) -> Option<u64> {
        let c = &self.core;
        let mut d: Vec<u64> = Vec::new();
        d.extend(c.query_due);
        d.extend(c.refresh_at);
        if c.status == MachineStatus::Connecting && c.link_open {
            d.push(c.connect_start + CONNECT_TIMEOUT_MS + 1);
        }
        if let Some(op) = c.sys.front() {
            match (op.started, op.phase) {
                (Some(start), OpPhase::Waiting) => d.push(start + QUERY_TIMEOUT_MS + 1),
                (Some(_), OpPhase::Collecting { ok_at, last_row_at }) => {
                    d.push((last_row_at + COLLECT_QUIET_MS).min(ok_at + COLLECT_MAX_MS));
                }
                (None, _) => d.push(c.now),
            }
        }
        if c.in_program() {
            // hang detection, buffer stuck, cooling
            d.push(c.now + 100);
        }
        d.into_iter().min()
    }

    /// Periodic work: sends lines that fit, status queries, timeouts (the TX loop).
    pub fn tick(&mut self, now: u64) {
        self.core.now = now;
        if self.core.status == MachineStatus::Connecting
            && self.core.link_open
            && now.saturating_sub(self.core.connect_start) > CONNECT_TIMEOUT_MS
        {
            self.core.link_open = false;
            self.core.out.push_back(Output::Close);
        }
        if self.core.link_open {
            self.run_housekeeping();
            self.progress_system_ops();
            self.handle_continuous_jog();
            self.send_lines();
            self.manage_cooling();
        }
        if let Some(due) = self.core.query_due
            && now >= due
        {
            self.fw.query_position(&mut self.core);
            self.core.query_due = Some(now + self.core.opts.status_query_ms);
        }
        self.detect_hang();
    }

    fn pump(&mut self, now: u64) {
        if self.core.link_open {
            self.core.now = now;
            self.progress_system_ops();
            self.handle_continuous_jog();
            self.send_lines();
        }
    }

    // ================================================================== status

    fn set_status(&mut self, new: MachineStatus) {
        let c = &mut self.core;
        let old = c.status;
        c.life.on_status(old, c.now);
        if old == new {
            return;
        }
        c.status = new;
        if old == MachineStatus::Connecting && new == MachineStatus::Disconnected {
            c.failed_connections += 1;
        }
        if old == MachineStatus::Connecting && new != MachineStatus::Disconnected {
            c.failed_connections = 0;
            if self.fw.supports_config_queries() {
                c.refresh_at = Some(c.now + REFRESH_DELAY_MS);
            }
            if c.life.on_connect() == Selection::AskUser {
                c.events.push_back(Event::LaserSelectionNeeded);
            }
        }
        if old.is_any_hold() && !new.is_any_hold() {
            c.hold_by_user = false;
            c.hold_by_cooling = false;
        }
        c.events.push_back(Event::StatusChanged { old, new });
        if c.tp.in_program() {
            if new != MachineStatus::Run && new != MachineStatus::Idle {
                c.tp.job_pause(c.now);
            } else {
                c.tp.job_resume(c.now);
            }
        }
        if new == MachineStatus::Disconnected {
            c.set_feed_speed(0.0, 0.0);
        }
    }

    /// Applies a parsed state with LaserGRBL's adjustments (Idle during a job is Run,
    /// a hold the host did not request is AutoHold).
    fn apply_parsed_status(&mut self, mut st: MachineStatus) {
        let c = &self.core;
        if c.in_program() && st == MachineStatus::Idle {
            st = MachineStatus::Run; // Grbl reports Idle during G4
        }
        if st == MachineStatus::Hold && c.hold_by_cooling {
            st = MachineStatus::Cooling;
        } else if st == MachineStatus::Hold && !c.hold_by_user {
            st = MachineStatus::AutoHold;
        }
        self.set_status(st);
    }

    // ================================================================== RX

    fn manage_received_line(&mut self, line: &str) {
        match self.fw.handle_line(&mut self.core, line) {
            LineHandled::Yes => return,
            LineHandled::Status(st) => {
                self.apply_parsed_status(st);
                return;
            }
            LineHandled::Responses(rs) => {
                for r in rs {
                    self.command_response(&r);
                }
                return;
            }
            LineHandled::No => {}
        }
        let lower = line.to_lowercase();
        if lower.starts_with("ok") || lower.starts_with("error") {
            self.command_response(line);
        } else if line.starts_with('<') && line.ends_with('>') {
            self.realtime_status(line);
        } else if line.starts_with("GrblHAL ") {
            self.welcome(line, 8, None, true);
        } else if line.starts_with("Grbl-Vigo:") {
            self.core.vendor_info_seen = Some("Grbl-Vigo".into());
            self.core.vendor_version_seen = line.split(':').nth(2).map(str::to_owned);
            self.welcome(line, 10, None, false);
        } else if line.starts_with("Ortur ") || line.starts_with("Aufero ") {
            let v = line.replace("Ready", "").replace('!', "");
            self.core.vendor_info_seen = Some(v.trim().to_owned());
            self.core.log(MessageKind::classify(line), line, None);
        } else if line.starts_with("OLF") {
            let v = line.replace("OLF", "");
            self.core.vendor_version_seen = Some(v.trim_matches(['.', ' ', ':']).to_owned());
            self.core.log(MessageKind::classify(line), line, None);
        } else if line.starts_with("[Machine:") && line.ends_with(']') {
            let v = line.replace("[Machine:", "").replace(']', "");
            self.core.vendor_info_seen = Some(v.trim().to_owned());
            self.set_version(self.longer_version());
            self.core.log(MessageKind::classify(line), line, None);
        } else if line.starts_with("[Software:") && line.ends_with(']') {
            let v = line.replace("[Software:", "").replace(']', "");
            self.core.vendor_version_seen = Some(v);
            self.set_version(self.longer_version());
            self.core.log(MessageKind::classify(line), line, None);
        } else if line.starts_with("SimpleLaser ") {
            self.core.vendor_info_seen = Some("SimpleLaser".into());
            self.welcome(line, 12, None, false);
        } else if line.starts_with("Grbl ") {
            self.welcome(line, 5, None, false);
        } else if let Some((fw, maj, min, build)) = self.unknown_welcome(line) {
            self.core.vendor_info_seen = Some(fw);
            self.welcome(line, 0, Some((maj, min, build)), false);
        } else if lower.contains("ok") {
            self.core.log(MessageKind::Warning, "Handle broken ok!", None);
            self.command_response("ok");
        } else if lower.starts_with("alarm:") {
            if self.core.tp.last_issue() == Issue::Unknown && self.core.in_program() {
                self.core.set_issue(Issue::MachineAlarm);
            }
            self.core.generic_message(line);
        } else if let Some(ip) = ip_message(line) {
            self.core.detected_ip = Some(ip.clone());
            self.core.events.push_back(Event::IpDetected(ip));
            self.core.generic_message(line);
        } else {
            self.core.generic_message(line);
        }
    }

    fn longer_version(&self) -> FirmwareVersion {
        FirmwareVersion::with_vendor(
            1,
            1,
            'f',
            self.core.vendor_info_seen.clone(),
            self.core.vendor_version_seen.clone(),
            false,
        )
    }

    fn unknown_welcome(&self, line: &str) -> Option<(String, u32, u32, char)> {
        if self.core.welcome_seen {
            return None;
        }
        static RE: std::sync::OnceLock<Option<regex::Regex>> = std::sync::OnceLock::new();
        let re = RE
            .get_or_init(|| {
                regex::Regex::new(r"(?<fw>[a-zA-Z]+)\s+(?<maj>\d+)\.(?<min>\d+)(?<build>\D)(?:$|\s)").ok()
            })
            .as_ref()?;
        let c = re.captures(line)?;
        Some((
            c["fw"].to_owned(),
            c["maj"].parse().ok()?,
            c["min"].parse().ok()?,
            c["build"].chars().next()?,
        ))
    }

    /// A welcome banner: version at fixed offsets (`Grbl 1.1f`), or parsed already.
    fn welcome(&mut self, line: &str, offset: usize, parsed: Option<(u32, u32, char)>, hal: bool) {
        self.core.welcome_seen = true;
        let parse = || -> Option<(u32, u32, char)> {
            let b = line.as_bytes();
            let maj = char::from(*b.get(offset)?).to_digit(10)?;
            let min = char::from(*b.get(offset + 2)?).to_digit(10)?;
            let build = line.get(offset + 3..)?.chars().next()?;
            Some((maj, min, build))
        };
        if let Some((maj, min, build)) = parsed.or_else(parse) {
            let v = FirmwareVersion::with_vendor(
                maj,
                min,
                build,
                self.core.vendor_info_seen.clone(),
                self.core.vendor_version_seen.clone(),
                hal,
            );
            self.set_version(v);
            self.detect_unexpected_reset();
            self.on_startup_message();
        }
        self.core.log(MessageKind::classify(line), line, None);
    }

    fn set_version(&mut self, v: FirmwareVersion) {
        self.core.version = Some(v.clone());
        self.core.events.push_back(Event::VersionDetected(v));
    }

    fn detect_unexpected_reset(&mut self) {
        if self.core.tp.last_issue() == Issue::Unknown
            && self.core.status == MachineStatus::Run
            && self.core.in_program()
        {
            self.core.set_issue(Issue::UnexpectedReset);
        }
    }

    /// Everything is reset, so a hard (unexpected) reset is handled too.
    fn on_startup_message(&mut self) {
        let c = &mut self.core;
        c.fail_system_ops("the board restarted");
        c.clear_queue(false);
        c.used_buffer = 0;
        c.tp.job_end(c.now);
        c.reset_overrides();
    }

    fn realtime_status(&mut self, line: &str) {
        self.core.status_seen();
        let body = &line[1..line.len() - 1];
        let mut version = report_version(self.core.version.as_ref(), body);
        if body.contains('|') && !version.is_v11() {
            // a `|` report is always the 1.1 format (FINDINGS F-15 fixed: DIV-015)
            version = FirmwareVersion::new(1, 1);
        }
        let report = parse_report(body, &version);
        let Some(st) = self.fw.parse_status_name(&report.state) else {
            return;
        };
        self.apply_parsed_status(st);
        let legacy = !version.is_v11();
        for field in report.fields {
            let c = &mut self.core;
            match field {
                StatusField::Overrides(f, r, s) => {
                    let new = [f, r, s];
                    if new != c.ov_cur {
                        c.ov_cur = new;
                        c.events.push_back(Event::OverridesChanged(new));
                    }
                    c.manage_overrides();
                }
                StatusField::Buffer { blocks, bytes } => {
                    c.grbl_blocks = blocks;
                    c.grbl_buffer = bytes;
                    c.enlarge_buffer(bytes, false);
                }
                StatusField::WorkPosition(p) if legacy => {
                    let wco = c.mpos - p;
                    c.set_wco(wco);
                }
                StatusField::WorkPosition(p) => {
                    let m = c.wco + p;
                    c.set_machine_position(m);
                }
                StatusField::MachinePosition(p) => c.set_machine_position(p),
                StatusField::WorkOffset(p) => c.set_wco(p),
                StatusField::FeedSpeed(f, s) => c.set_feed_speed(f, s),
                StatusField::Feed(text) => self.fw.parse_feed_field(&mut self.core, &text),
            }
        }
    }

    fn command_response(&mut self, line: &str) {
        let c = &mut self.core;
        c.last_activity_at = c.now;
        if let Some(p) = c.pending.pop_front() {
            let result = line.to_uppercase().trim().to_owned();
            let status = CommandStatus::from_result(&result);
            c.used_buffer = c.used_buffer.saturating_sub(p.wire_len);
            let decoded = if status == CommandStatus::ResponseBad {
                decode_result(&result)
            } else {
                None
            };
            c.events.push_back(Event::Console(ConsoleRow::Result {
                id: p.row,
                result: result.clone(),
                status,
                decoded,
            }));
            if c.tp.in_program() && p.q.cmd.repeat_count() == 0 {
                c.tp.job_executed(p.q.offset_ms);
            }
            if c.tp.in_program() && status == CommandStatus::ResponseBad {
                c.tp.job_error();
            }
            if p.q.cmd.is_settings_write() && status == CommandStatus::ResponseGood {
                // stored even when no table was read yet (FINDINGS F-16 fixed: DIV-016)
                c.settings.add_or_update(p.q.cmd.text());
                c.events.push_back(Event::SettingsChanged);
            }
            if let Origin::System(id) = p.q.origin
                && let Some(op) = c.sys.front_mut().filter(|op| op.id == id)
            {
                op.answered += 1;
                if status != CommandStatus::ResponseGood {
                    op.errors.push(format!("{} {}", p.q.cmd.text(), result));
                }
                if op.answered == 1 && matches!(op.kind, SystemOpKind::ReadSettings | SystemOpKind::ReadInfo)
                {
                    op.phase = if status == CommandStatus::ResponseGood {
                        OpPhase::Collecting {
                            ok_at: c.now,
                            last_row_at: c.now,
                        }
                    } else {
                        OpPhase::Collecting {
                            ok_at: 0,
                            last_row_at: 0,
                        }
                    };
                }
            }
            let mode = self.fw.streaming_mode(self.core.opts.streaming_mode);
            let c = &mut self.core;
            if c.in_program()
                && mode == StreamingMode::RepeatOnError
                && c.pending.is_empty()
                && status == CommandStatus::ResponseBad
                && p.q.cmd.repeat_count() < 3
            {
                c.retry = Some(Queued {
                    cmd: Command::with_options(p.q.cmd.text(), p.q.cmd.repeat_count() + 1, false),
                    offset_ms: 0,
                    origin: Origin::Job,
                });
            }
        }
        let c = &self.core;
        if c.in_program() && c.queue.is_empty() && c.pending.is_empty() {
            self.on_program_end();
        }
        self.progress_system_ops();
    }

    fn handle_missing_ok(&mut self) {
        if self.is_buffer_stuck()
            && self.core.grbl_buffer >= 0
            && self.core.grbl_buffer as usize == self.core.buffer_size()
        {
            self.unlock_buffer_stuck_inner();
        }
    }

    fn is_buffer_stuck(&self) -> bool {
        let c = &self.core;
        c.status == MachineStatus::Run
            && !c.pending.is_empty()
            && c.used_buffer != 0
            && c.now.saturating_sub(c.last_activity_at) > STUCK_MS
    }

    /// Acknowledges every pending line when the buffer looks stuck (manual recovery).
    pub fn unlock_buffer_stuck(&mut self, now: u64) -> bool {
        self.core.now = now;
        if self.is_buffer_stuck() {
            self.unlock_buffer_stuck_inner();
            true
        } else {
            false
        }
    }

    fn unlock_buffer_stuck_inner(&mut self) {
        let count = self.core.pending.len();
        self.core
            .log(MessageKind::Warning, "Unlock from buffer stuck!", None);
        self.core.events.push_back(Event::BufferStuckUnlocked { count });
        for _ in 0..count {
            self.command_response("ok");
        }
    }

    // ================================================================== TX

    /// Where the next line comes from (`PeekNextCommand`); `None` when nothing may
    /// be sent now.
    fn next_source(&self) -> Option<Source> {
        let c = &self.core;
        let mode = self.fw.streaming_mode(c.opts.streaming_mode);
        if c.pending.front().is_some_and(|p| p.q.cmd.is_settings_write()) {
            return None; // EEPROM writes are synchronous
        }
        let sys_active = c.sys.front().is_some_and(|op| op.started.is_some());
        let (queue, len) = if sys_active {
            (Source::System, c.sys_cmds.len())
        } else {
            (Source::Main, c.queue.len())
        };
        if mode == StreamingMode::Buffered {
            (len > 0).then_some(queue)
        } else if c.pending.is_empty() {
            if !sys_active && c.retry.is_some() {
                Some(Source::Retry)
            } else {
                (len > 0).then_some(queue)
            }
        } else {
            None
        }
    }

    fn peek_next(&self) -> Option<&Queued> {
        let c = &self.core;
        match self.next_source()? {
            Source::Main => c.queue.front(),
            Source::System => c.sys_cmds.front(),
            Source::Retry => c.retry.as_ref(),
        }
    }

    fn can_send(&self) -> bool {
        self.peek_next()
            .is_some_and(|q| self.core.used_buffer + q.cmd.wire_len() <= self.core.buffer_size())
    }

    fn send_lines(&mut self) {
        while self.core.link_open && self.can_send() {
            let source = self.next_source();
            let c = &mut self.core;
            let q = match source {
                Some(Source::Main) => c.queue.pop_front(),
                Some(Source::System) => c.sys_cmds.pop_front(),
                Some(Source::Retry) => c.retry.take(),
                None => None,
            };
            let Some(q) = q else { break };
            let row = c.next_row;
            c.next_row += 1;
            let text = if q.cmd.repeat_count() == 0 {
                q.cmd.text().to_owned()
            } else {
                format!("{} (Retry {})", q.cmd.text(), q.cmd.repeat_count())
            };
            c.collect_row(&text);
            c.events
                .push_back(Event::Console(ConsoleRow::Command { id: row, text }));
            let wire_len = q.cmd.wire_len();
            let send = q.cmd.clone();
            c.pending.push_back(Pending { q, row, wire_len });
            self.fw.send_to_serial(&mut self.core, &send);
            if self.core.in_program() {
                self.core.tp.job_sent();
            }
            self.core.last_activity_at = self.core.now;
        }
    }

    // ================================================================== housekeeping and system operations

    fn run_housekeeping(&mut self) {
        if let Some(at) = self.core.refresh_at
            && self.core.now >= at
        {
            self.core.refresh_at = None;
            if self.core.can_read_write_config() {
                self.push_system_op(SystemOpKind::ReadSettings, vec![Command::new("$$")]);
            }
            if self.core.opts.query_machine_info {
                self.push_system_op(SystemOpKind::ReadInfo, vec![Command::new("$I")]);
            }
        }
    }

    fn push_system_op(&mut self, kind: SystemOpKind, commands: Vec<Command>) -> u64 {
        let c = &mut self.core;
        let id = c.next_op_id;
        c.next_op_id += 1;
        c.sys.push_back(SystemOp {
            id,
            kind,
            commands,
            started: None,
            phase: OpPhase::Waiting,
            rows: Vec::new(),
            answered: 0,
            errors: Vec::new(),
        });
        self.progress_system_ops();
        id
    }

    fn progress_system_ops(&mut self) {
        loop {
            let now = self.core.now;
            let expected = self.core.settings_expected_count();
            let c = &mut self.core;
            let Some(op) = c.sys.front_mut() else { return };
            if op.started.is_none() {
                op.started = Some(now);
                let id = op.id;
                let cmds = op.commands.clone();
                c.sys_cmds = cmds
                    .into_iter()
                    .map(|cmd| Queued {
                        cmd,
                        offset_ms: 0,
                        origin: Origin::System(id),
                    })
                    .collect();
            }
            let done = match (op.kind.clone(), op.phase) {
                (SystemOpKind::ReadSettings | SystemOpKind::ReadInfo, OpPhase::Waiting) => {
                    now.saturating_sub(op.started.unwrap_or(now)) > QUERY_TIMEOUT_MS
                }
                (kind, OpPhase::Collecting { ok_at, last_row_at }) => {
                    // rows include the query itself (LaserGRBL counts its console rows)
                    let target = if kind == SystemOpKind::ReadSettings {
                        expected + 1
                    } else {
                        3
                    };
                    ok_at == 0
                        || op.rows.len() >= target
                        || now.saturating_sub(last_row_at) >= COLLECT_QUIET_MS
                        || now.saturating_sub(ok_at) >= COLLECT_MAX_MS
                }
                (SystemOpKind::WriteSettings | SystemOpKind::WriteWifi, OpPhase::Waiting) => {
                    op.answered >= op.commands.len()
                }
            };
            if !done {
                return;
            }
            let Some(op) = c.sys.pop_front() else { return };
            c.sys_cmds.clear();
            self.finish_system_op(op);
        }
    }

    fn finish_system_op(&mut self, op: SystemOp) {
        let c = &mut self.core;
        let result = match (&op.kind, op.phase) {
            (SystemOpKind::ReadSettings | SystemOpKind::ReadInfo, OpPhase::Waiting) => {
                // FINDINGS F-28: the query stays pending, so a late answer acknowledges
                // it instead of shifting later responses (DIV-028)
                Err("No response received from grbl!".to_owned())
            }
            (SystemOpKind::ReadSettings | SystemOpKind::ReadInfo, OpPhase::Collecting { ok_at: 0, .. }) => {
                Err(op.errors.join("\n"))
            }
            (SystemOpKind::ReadSettings, _) => {
                let mut table = GrblSettings::new(c.version.clone());
                for row in &op.rows {
                    table.add_or_update(row);
                }
                if !table.is_empty() {
                    c.settings = table;
                    c.events.push_back(Event::SettingsChanged);
                }
                Ok(())
            }
            (SystemOpKind::ReadInfo, _) => {
                for row in &op.rows {
                    if let Some(opt) = row.strip_prefix("[OPT:").and_then(|r| r.strip_suffix(']')) {
                        let tx = opt.split(',').nth(2).and_then(|v| v.trim().parse::<i32>().ok());
                        if let Some(tx) = tx.filter(|t| *t > 127) {
                            c.enlarge_buffer(tx, true);
                        }
                    }
                }
                Ok(())
            }
            (SystemOpKind::WriteSettings | SystemOpKind::WriteWifi, _) => {
                if op.errors.is_empty() {
                    Ok(())
                } else {
                    Err(op.errors.join("\n"))
                }
            }
        };
        c.events.push_back(Event::SystemOpFinished {
            id: op.id,
            kind: op.kind,
            result,
        });
    }

    // ================================================================== job

    /// Loads a program (refused during a job).
    pub fn set_program(&mut self, program: Program) -> Result<(), HostError> {
        if self.core.in_program() {
            return Err(HostError::NotAllowed("a job is running"));
        }
        self.core.program = program;
        self.core.tp.reset(true);
        Ok(())
    }

    /// The loaded program.
    #[must_use]
    pub const fn program(&self) -> &Program {
        &self.core.program
    }

    /// Number of passes of the next job.
    pub fn set_loop_count(&mut self, count: u32) {
        self.core.loop_count = count.max(1);
        self.core
            .events
            .push_back(Event::LoopCountChanged(self.core.loop_count));
    }

    fn check_can_send_file(&self) -> Result<(), HostError> {
        let c = &self.core;
        if !c.is_connected() {
            return Err(HostError::NotConnected);
        }
        if c.program.is_empty() {
            return Err(HostError::NoProgram);
        }
        if !matches!(c.status, MachineStatus::Idle | MachineStatus::Check) {
            return Err(HostError::NotAllowed("the machine is not idle"));
        }
        if !c.queue.is_empty() {
            return Err(HostError::NotAllowed("commands are still queued"));
        }
        // FINDINGS F-30 fixed (DIV-030): no job while the connect-time queries run
        if c.refresh_at.is_some() || !c.sys.is_empty() {
            return Err(HostError::Busy);
        }
        Ok(())
    }

    /// Runs the loaded program from the start (header, program, footer).
    pub fn run_program(&mut self, homing: bool, now: u64) -> Result<(), HostError> {
        self.core.now = now;
        self.check_can_send_file()?;
        self.run_from_start(homing, true, false);
        self.pump(now);
        Ok(())
    }

    fn run_from_start(&mut self, homing: bool, first: bool, pass: bool) {
        self.core.clear_queue(true);
        self.core.tp.reset(first);
        if homing {
            self.core.enqueue(Command::new("$H"));
        }
        if first {
            self.fw.on_job_begin(&mut self.core);
        }
        if pass {
            let passes = self.core.opts.custom_passes.clone();
            self.core.execute_custom_code(&passes);
        }
        let c = &mut self.core;
        for line in c.program.lines() {
            c.queue.push_back(Queued {
                cmd: line.command.clone(),
                offset_ms: line.offset_ms,
                origin: Origin::Job,
            });
        }
        let target = c.queue.len();
        c.tp.job_start(c.program.estimated_ms(), target, first, c.now);
        c.events.push_back(Event::JobStarted { target });
    }

    /// Resume information after an interrupted job (`None` if there is nothing to resume).
    #[must_use]
    pub fn resume_suggestion(&self) -> Option<ResumeSuggestion> {
        let tp = &self.core.tp;
        if tp.executed() == 0 || tp.executed() == tp.target() {
            return None;
        }
        let set_wco = self.core.wco == Point3::ZERO && tp.last_known_wco() != Point3::ZERO;
        let homing = self.core.mpos == Point3::ZERO
            && tp.last_issue() != Issue::ManualAbort
            && tp.last_issue() != Issue::ManualReset;
        Some(ResumeSuggestion {
            executed: tp.executed(),
            sent: tp.sent(),
            target: tp.target(),
            issue: tp.last_issue(),
            homing,
            set_wco,
        })
    }

    /// Runs the program from command `position` (0-based), rebuilding position, feed,
    /// power and modal state first (`ContinueProgramFromKnown`).
    pub fn resume_program(
        &mut self,
        position: usize,
        homing: bool,
        set_wco: bool,
        now: u64,
    ) -> Result<(), HostError> {
        self.core.now = now;
        self.check_can_send_file()?;
        let c = &mut self.core;
        c.clear_queue(false);
        c.log(
            MessageKind::Other,
            &format!("[resume from #{}]", position + 1),
            None,
        );
        let mut spb = MachineState::new(c.opts.support_pwm);
        if homing {
            c.enqueue(Command::new("$H"));
        }
        if set_wco {
            let pos = if homing { Point3::ZERO } else { c.mpos };
            let cur = pos - c.tp.last_known_wco();
            c.enqueue(Command::new(&format!("G92 X{} Y{} Z{}", cur.x, cur.y, cur.z)));
        }
        let lines = c.program.lines().to_vec();
        for l in lines.iter().take(position) {
            spb.analyze(&l.command, None);
        }
        c.enqueue(Command::new("G90"));
        c.enqueue(Command::new(&format!(
            "M5 G0 {} {} {} {} {}",
            spb.x().word(),
            spb.y().word(),
            spb.z().word(),
            spb.f().word(),
            spb.s().word()
        )));
        let modals = spb.settled_modals();
        if !modals.is_empty() {
            // LaserGRBL also queues the empty command (FINDINGS F-19 fixed: DIV-019)
            c.enqueue(Command::from_words(&modals));
        }
        let added = c.queue.len();
        c.tp.job_continue(c.program.estimated_ms(), lines.len(), position, added, c.now);
        for (i, l) in lines.iter().enumerate().skip(position) {
            let mut cmd = l.command.clone();
            if i == position && cmd.is_movement() && cmd.get('G').is_none() {
                cmd = Command::prefixed(spb.motion_mode.current(), &cmd);
            }
            c.queue.push_back(Queued {
                cmd,
                offset_ms: l.offset_ms,
                origin: Origin::Job,
            });
        }
        self.pump(now);
        Ok(())
    }

    fn on_program_end(&mut self) {
        let now = self.core.now;
        let ended = self.core.tp.job_end(now);
        let lines = self.core.program.len();
        let errors = self.core.tp.errors();
        let time_ms = self.core.tp.total_job_ms(now);
        let summary = format!("[{lines} lines, {errors} errors, {}]", format_duration(time_ms));
        if ended && self.core.loop_count > 1 && self.core.status != MachineStatus::Check {
            self.core.log(MessageKind::Diagnostic, &summary, None);
            self.core.loop_count -= 1;
            let remaining = self.core.loop_count;
            self.core.events.push_back(Event::PassEnded { remaining });
            self.core.events.push_back(Event::LoopCountChanged(remaining));
            self.run_from_start(false, false, true);
        } else {
            self.core.log(MessageKind::Diagnostic, &summary, None);
            self.core.events.push_back(Event::JobEnded {
                lines,
                errors,
                time_ms,
            });
            self.fw.on_job_end(&mut self.core);
            if self.fw.force_idle_at_end() {
                self.set_status(MachineStatus::Idle);
            }
        }
    }

    /// Stops streaming the job and turns the laser off (`AbortProgram`). Lines already
    /// in the board's buffer still run; use [`Host::soft_reset`] to stop at once.
    pub fn abort_program(&mut self, now: u64) -> Result<(), HostError> {
        self.core.now = now;
        let c = &self.core;
        let allowed = (c.is_connected()
            && !c.program.is_empty()
            && (c.status == MachineStatus::Run || c.status.is_any_hold()))
            || !c.queue.is_empty();
        if !allowed {
            return Err(HostError::NotAllowed("nothing to abort"));
        }
        self.core.set_issue(Issue::ManualAbort);
        let c = &mut self.core;
        c.tp.job_end(now);
        c.queue.clear();
        c.enqueue(Command::new("M5"));
        self.pump(now);
        Ok(())
    }

    // ================================================================== immediate commands

    /// Soft reset (ctrl-x): flushes every queue and resets the board. Always available
    /// while connected; this is the STOP of the user interface.
    pub fn soft_reset(&mut self, now: u64) -> Result<(), HostError> {
        self.core.now = now;
        // also while connecting: STOP must always work (LaserGRBL needs a status report: DIV-102)
        if !self.core.link_open {
            return Err(HostError::NotConnected);
        }
        if self.core.tp.last_issue() == Issue::Unknown
            && self.core.status == MachineStatus::Run
            && self.core.in_program()
        {
            self.core.set_issue(Issue::ManualReset);
        }
        self.internal_reset(true);
        Ok(())
    }

    fn internal_reset(&mut self, device: bool) {
        let c = &mut self.core;
        c.fail_system_ops("reset");
        c.clear_queue(true);
        c.used_buffer = 0;
        c.tp.job_end(c.now);
        c.reset_overrides();
        if device {
            self.fw.send_board_reset(&mut self.core);
        }
    }

    /// Feed hold (`!`), only while running.
    pub fn feed_hold(&mut self, now: u64) -> Result<(), HostError> {
        self.core.now = now;
        self.feed_hold_inner(false)
    }

    fn feed_hold_inner(&mut self, cooling: bool) -> Result<(), HostError> {
        let c = &mut self.core;
        if !(c.is_connected() && c.status == MachineStatus::Run) {
            return Err(HostError::NotAllowed("the machine is not running"));
        }
        c.hold_by_user = !cooling;
        c.hold_by_cooling = cooling;
        c.send_immediate(b'!');
        Ok(())
    }

    /// Cycle start / resume (`~`) from a hold or a door.
    pub fn cycle_start(&mut self, now: u64) -> Result<(), HostError> {
        self.core.now = now;
        let c = &mut self.core;
        if !(c.is_connected() && (c.status == MachineStatus::Door || c.status.is_any_hold())) {
            return Err(HostError::NotAllowed("the machine is not holding"));
        }
        c.hold_by_user = false;
        c.hold_by_cooling = false;
        c.send_immediate(b'~');
        Ok(())
    }

    /// Safety door. LaserGRBL sends `@` (Grbl 0.9); FreeKerf sends 0x84 (FINDINGS F-20
    /// fixed: DIV-020).
    pub fn safety_door(&mut self, now: u64) {
        self.core.now = now;
        self.core.send_immediate(0x84);
    }

    /// Queues `$X` (Idle or Alarm).
    pub fn unlock(&mut self, now: u64) -> Result<(), HostError> {
        self.core.now = now;
        let c = &self.core;
        if !self.fw.shows_unlock() {
            return Err(HostError::Unsupported("this firmware has no unlock"));
        }
        if !(c.is_connected() && matches!(c.status, MachineStatus::Idle | MachineStatus::Alarm)) {
            return Err(HostError::NotAllowed("unlock needs Idle or Alarm"));
        }
        self.core.enqueue(Command::new("$X"));
        self.pump(now);
        Ok(())
    }

    /// Queues `$H` (Idle or Alarm, homing enabled with `$22`).
    pub fn home(&mut self, now: u64) -> Result<(), HostError> {
        self.core.now = now;
        let c = &self.core;
        if !(c.is_connected() && matches!(c.status, MachineStatus::Idle | MachineStatus::Alarm)) {
            return Err(HostError::NotAllowed("homing needs Idle or Alarm"));
        }
        if !c.settings.homing_enabled() {
            return Err(HostError::NotAllowed("homing is disabled ($22=0)"));
        }
        self.core.enqueue(Command::new("$H"));
        self.pump(now);
        Ok(())
    }

    /// Sets the current position as work zero (`G92 X0 Y0 Z0`).
    pub fn set_new_zero(&mut self, now: u64) -> Result<(), HostError> {
        self.core.now = now;
        let c = &self.core;
        if !(c.is_connected() && c.status == MachineStatus::Idle && c.mpos - c.wco != Point3::ZERO) {
            return Err(HostError::NotAllowed(
                "zeroing needs Idle away from the work zero",
            ));
        }
        self.core.enqueue(Command::new("G92 X0 Y0 Z0"));
        self.pump(now);
        Ok(())
    }

    /// Queues a command typed by the user (not during a job).
    pub fn send_command(&mut self, line: &str, now: u64) -> Result<(), HostError> {
        self.core.now = now;
        let c = &self.core;
        if !c.is_connected() {
            return Err(HostError::NotConnected);
        }
        if c.in_program() {
            return Err(HostError::NotAllowed("a job is running"));
        }
        self.core.enqueue(Command::new(line));
        self.pump(now);
        Ok(())
    }

    /// Runs custom code (buttons, macros).
    pub fn execute_custom_code(&mut self, code: &str, now: u64) -> Result<(), HostError> {
        self.core.now = now;
        if !self.core.is_connected() {
            return Err(HostError::NotConnected);
        }
        self.core.execute_custom_code(code);
        self.pump(now);
        Ok(())
    }

    /// Evaluates `[expr]` in `text` with the current variables.
    #[must_use]
    pub fn evaluate_expression(&self, text: &str) -> String {
        evaluate_brackets(text, &self.core.variables())
    }

    // ================================================================== overrides

    /// Target overrides in % (feed 10-200, rapids 25/50/100, power 10-200); the host
    /// steps towards them at each status report.
    pub fn set_override_targets(&mut self, feed: i32, rapid: i32, power: i32) {
        self.core.ov_target = [feed, rapid, power];
    }

    /// Target overrides.
    #[must_use]
    pub const fn override_targets(&self) -> [i32; 3] {
        self.core.ov_target
    }

    /// Current overrides as reported by the board.
    #[must_use]
    pub const fn overrides(&self) -> [i32; 3] {
        self.core.ov_cur
    }

    // ================================================================== jog

    fn supports_true_jogging(&self) -> bool {
        self.fw.supports_true_jogging(self.core.version.as_ref())
    }

    /// True when a jog can be sent now.
    #[must_use]
    pub fn jog_enabled(&self) -> bool {
        let c = &self.core;
        if self.supports_true_jogging() {
            c.is_connected() && matches!(c.status, MachineStatus::Idle | MachineStatus::Jog)
        } else {
            c.is_connected()
                && matches!(c.status, MachineStatus::Idle | MachineStatus::Run)
                && !c.in_program()
        }
    }

    /// Jogs one step (or towards the table edge with continuous jog).
    pub fn jog(&mut self, dir: JogDirection, speed: f32, step: Decimal, now: u64) -> Result<(), HostError> {
        self.core.now = now;
        if !self.jog_enabled() {
            return Err(HostError::NotAllowed("jog is not possible now"));
        }
        let s = format_speed(speed);
        if !self.supports_true_jogging() {
            if dir == JogDirection::Home {
                self.core.enqueue(Command::new("G90"));
                self.core.enqueue(Command::new(&format!("G1X0Y0F{s}")));
            } else {
                self.core.enqueue(Command::new("G91"));
                self.core
                    .enqueue(Command::new(&format!("G1{}F{s}", step_axes(dir, step))));
                self.core.enqueue(Command::new("G90"));
            }
        } else if !self.core.opts.continuous_jog || dir.is_z() {
            if dir == JogDirection::Home {
                self.core.enqueue(Command::new(&format!("$J=G90X0Y0F{s}")));
            } else {
                self.core
                    .enqueue(Command::new(&format!("$J=G91{}F{s}", step_axes(dir, step))));
            }
        } else {
            self.set_jog_target(JogTarget::Direction(dir, speed));
        }
        self.pump(now);
        Ok(())
    }

    /// Jogs to an absolute work position (clamped to the table with soft limits).
    pub fn jog_to(&mut self, x: f32, y: f32, speed: f32, now: u64) -> Result<(), HostError> {
        self.core.now = now;
        let (x, y) = self.limit_to_bound(x, y);
        if !self.jog_enabled() {
            return Err(HostError::NotAllowed("jog is not possible now"));
        }
        let s = format_speed(speed);
        if !self.supports_true_jogging() {
            self.core.enqueue(Command::new("G90"));
            self.core.enqueue(Command::new(&format!(
                "G1X{}Y{}F{s}",
                format_coord(x),
                format_coord(y)
            )));
        } else if !self.core.opts.continuous_jog {
            self.core.enqueue(Command::new(&format!(
                "$J=G90X{}Y{}F{s}",
                format_coord(x),
                format_coord(y)
            )));
        } else {
            self.set_jog_target(JogTarget::Position(x, y, speed));
        }
        self.pump(now);
        Ok(())
    }

    /// Releases a continuous jog (button up): cancels the running jog (0x85).
    pub fn jog_abort(&mut self, now: u64) {
        self.core.now = now;
        if self.supports_true_jogging() && self.core.opts.continuous_jog {
            self.set_jog_target(JogTarget::Abort);
            self.pump(now);
        }
    }

    fn limit_to_bound(&self, x: f32, y: f32) -> (f32, f32) {
        let s = &self.core.settings;
        if !s.soft_limit() {
            return (x, y);
        }
        let w = self.core.wco;
        let tw = freekerf_gcode::number::f64_from_decimal(s.table_width()) as f32;
        let th = freekerf_gcode::number::f64_from_decimal(s.table_height()) as f32;
        (x.max(-w.x).min(tw - w.x), y.max(-w.y).min(th - w.y))
    }

    fn set_jog_target(&mut self, target: JogTarget) {
        self.core.cj_seq += 1;
        self.core.cj_curr = Some((self.core.cj_seq, target));
    }

    fn handle_continuous_jog(&mut self) {
        if !self.supports_true_jogging() || !self.core.pending.is_empty() {
            return;
        }
        let c = &mut self.core;
        let Some(curr) = c.cj_curr.filter(|cur| c.cj_prev.map(|p| p.0) != Some(cur.0)) else {
            return;
        };
        let abort_previous = c.cj_prev.is_some_and(|p| p.1 != JogTarget::Abort);
        c.cj_prev = Some(curr);
        if abort_previous {
            c.send_immediate(0x85);
        }
        match curr.1 {
            JogTarget::Abort => {}
            JogTarget::Position(x, y, speed) => {
                let cmd = format!(
                    "$J=G90X{}Y{}F{}",
                    format_coord(x),
                    format_coord(y),
                    format_speed(speed)
                );
                c.enqueue(Command::new(&cmd));
            }
            JogTarget::Direction(JogDirection::Home, speed) => {
                c.enqueue(Command::new(&format!("$J=G90X0Y0F{}", format_speed(speed))));
            }
            JogTarget::Direction(dir, speed) => {
                let axes = edge_axes(dir, c.settings.table_width(), c.settings.table_height());
                c.enqueue(Command::new(&format!("$J=G53{axes}F{}", format_speed(speed))));
            }
        }
    }

    // ================================================================== settings

    /// Writes settings (`$N=V` each), one at a time. Refused unless the machine is
    /// Idle or Alarm and no job runs (LaserGRBL silently skips: DIV-024). Completion
    /// is reported with [`Event::SystemOpFinished`] carrying the returned id.
    pub fn write_settings(&mut self, values: &[(u32, String)], now: u64) -> Result<u64, HostError> {
        self.core.now = now;
        if !self.core.can_read_write_config() {
            return Err(HostError::NotAllowed(
                "settings can be written only in Idle or Alarm",
            ));
        }
        let cmds = values
            .iter()
            .map(|(n, v)| Command::preserving_case(&format!("${n}={v}")))
            .collect();
        let id = self.push_system_op(SystemOpKind::WriteSettings, cmds);
        self.pump(now);
        Ok(id)
    }

    /// Reads the settings again (`$$`).
    pub fn refresh_settings(&mut self, now: u64) -> Result<u64, HostError> {
        self.core.now = now;
        if !self.fw.supports_config_queries() {
            return Err(HostError::Unsupported("this firmware has no $$"));
        }
        if !self.core.can_read_write_config() {
            return Err(HostError::NotAllowed(
                "settings can be read only in Idle or Alarm",
            ));
        }
        let id = self.push_system_op(SystemOpKind::ReadSettings, vec![Command::new("$$")]);
        self.pump(now);
        Ok(id)
    }

    /// Configures the Wi-Fi of Ortur or Longer boards.
    pub fn write_wifi(&mut self, ssid: &str, password: &str, now: u64) -> Result<Option<u64>, HostError> {
        self.core.now = now;
        let Some(v) = self.core.version.clone() else {
            return Err(HostError::Unsupported("unknown board"));
        };
        self.core.detected_ip = None;
        if v.is_ortur() {
            if !self.core.can_read_write_config() {
                return Err(HostError::NotAllowed(
                    "Wi-Fi can be configured only in Idle or Alarm",
                ));
            }
            let cmds = vec![
                Command::preserving_case(&format!("$74={ssid}")),
                Command::preserving_case(&format!("$75={password}")),
                Command::new("$WRS"),
            ];
            let id = self.push_system_op(SystemOpKind::WriteWifi, cmds);
            self.pump(now);
            Ok(Some(id))
        } else if v.is_longer() {
            for line in [
                "$radio/mode=sta".to_owned(),
                format!("$sta/ssid={ssid}"),
                format!("$sta/password={password}"),
                "$wifi/begin".to_owned(),
            ] {
                self.core.enqueue(Command::preserving_case(&line));
            }
            self.pump(now);
            Ok(None)
        } else {
            Err(HostError::Unsupported(
                "Wi-Fi configuration needs an Ortur or Longer board",
            ))
        }
    }

    /// Settings table read from the board (or written by the user).
    #[must_use]
    pub const fn settings(&self) -> &GrblSettings {
        &self.core.settings
    }

    // ================================================================== cooling and hang detection

    fn manage_cooling(&mut self) {
        let Some(ac) = self.core.opts.auto_cooling else {
            return;
        };
        let c = &self.core;
        let supported = c.version.as_ref().is_some_and(FirmwareVersion::is_v11) && c.settings.laser_mode();
        if !(supported
            && c.in_program()
            && c.status != MachineStatus::Hold
            && c.status != MachineStatus::AutoHold)
        {
            return;
        }
        let cycle = ac.on_ms + ac.off_ms;
        if cycle == 0 {
            return;
        }
        let cooling = c.tp.total_global_ms(c.now) % cycle > ac.on_ms;
        if cooling != c.now_cooling {
            if cooling {
                let _ = self.feed_hold_inner(true);
            } else {
                let _ = self.cycle_start(self.core.now);
            }
            self.core.now_cooling = cooling;
        }
    }

    fn detect_hang(&mut self) {
        let c = &self.core;
        if c.tp.last_issue() == Issue::Unknown
            && c.status == MachineStatus::Run
            && c.in_program()
            && self.fw.hang_detected(c)
        {
            self.core.set_issue(Issue::StopResponding);
        }
    }

    /// Selects the laser module in use by id.
    pub fn select_laser(&mut self, id: &str) -> bool {
        self.core.life.select(id)
    }

    /// Laser life counters.
    #[must_use]
    pub const fn laser_life(&self) -> &LaserLife {
        &self.core.life
    }

    /// Replaces the laser life counters (loaded from storage).
    pub fn set_laser_life(&mut self, life: LaserLife) {
        self.core.life = life;
    }

    // ================================================================== observation

    /// Snapshot of the state for front ends.
    #[must_use]
    pub fn snapshot(&self) -> HostSnapshot {
        let c = &self.core;
        HostSnapshot {
            firmware: self.fw.kind(),
            status: c.status,
            connected: c.is_connected(),
            ready: self.ready(),
            version: c.version.clone(),
            in_program: c.in_program(),
            machine_position: c.mpos,
            work_offset: c.wco,
            work_position: c.mpos - c.wco,
            feed: c.cur_f,
            power: c.cur_s,
            overrides: c.ov_cur,
            override_targets: c.ov_target,
            buffer_size: c.buffer_size(),
            used_buffer: c.used_buffer,
            grbl_blocks: c.grbl_blocks,
            grbl_buffer: c.grbl_buffer,
            queued: c.queue.len(),
            pending: c.pending.len(),
            job_target: c.tp.target(),
            job_sent: c.tp.sent(),
            job_executed: c.tp.executed(),
            job_errors: c.tp.errors(),
            job_time_ms: c.tp.total_job_ms(c.now),
            projected_ms: c.tp.projected_ms(c.now),
            last_issue: c.tp.last_issue(),
            loop_count: c.loop_count,
            failed_connections: c.failed_connections,
            detected_ip: c.detected_ip.clone(),
            program_lines: c.program.len(),
            program_estimated_ms: c.program.estimated_ms(),
            jog_enabled: self.jog_enabled(),
        }
    }

    /// Connected and done with the connect-time queries, nothing pending.
    #[must_use]
    pub fn ready(&self) -> bool {
        let c = &self.core;
        c.is_connected() && c.refresh_at.is_none() && c.sys.is_empty() && c.pending.is_empty()
    }

    /// Machine state.
    #[must_use]
    pub const fn status(&self) -> MachineStatus {
        self.core.status
    }

    /// Detected firmware version.
    #[must_use]
    pub const fn version(&self) -> Option<&FirmwareVersion> {
        self.core.version.as_ref()
    }

    /// True during a job.
    #[must_use]
    pub const fn in_program(&self) -> bool {
        self.core.in_program()
    }

    /// Job progress and timers.
    #[must_use]
    pub const fn projection(&self) -> &TimeProjection {
        &self.core.tp
    }

    /// RX buffer size in use.
    #[must_use]
    pub fn buffer_size(&self) -> usize {
        self.core.buffer_size()
    }

    /// Bytes counted in the board's RX buffer.
    #[must_use]
    pub const fn used_buffer(&self) -> usize {
        self.core.used_buffer
    }
}

impl Core {
    fn fail_system_ops(&mut self, reason: &str) {
        self.sys_cmds.clear();
        let ops: Vec<SystemOp> = self.sys.drain(..).collect();
        for op in ops {
            self.events.push_back(Event::SystemOpFinished {
                id: op.id,
                kind: op.kind,
                result: Err(reason.to_owned()),
            });
        }
    }

    fn settings_expected_count(&self) -> usize {
        GrblSettings::new(self.version.clone()).expected_count()
    }
}

/// `[MSG:Get IP 1.2.3.4]` or `[MSG:Connected with 1.2.3.4]`.
fn ip_message(line: &str) -> Option<String> {
    let body = line.strip_suffix(']')?;
    body.strip_prefix("[MSG:Get IP ")
        .or_else(|| body.strip_prefix("[MSG:Connected with "))
        .map(str::to_owned)
}

/// `1h 2m 3s` style duration for the job summary.
fn format_duration(ms: u64) -> String {
    let s = ms / 1000;
    let (h, m, s) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 {
        format!("{h}h {m}m {s}s")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}

/// State of the host for front ends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostSnapshot {
    /// Firmware kind.
    pub firmware: FirmwareKind,
    /// Machine state.
    pub status: MachineStatus,
    /// Connected (status other than Disconnected/Connecting).
    pub connected: bool,
    /// Connected and done with the connect-time queries, nothing pending.
    pub ready: bool,
    /// Firmware version.
    pub version: Option<FirmwareVersion>,
    /// A job is running.
    pub in_program: bool,
    /// Machine position.
    pub machine_position: Point3,
    /// Work coordinate offset.
    pub work_offset: Point3,
    /// Work position.
    pub work_position: Point3,
    /// Current feed.
    pub feed: f32,
    /// Current power.
    pub power: f32,
    /// Current overrides (feed, rapids, power).
    pub overrides: [i32; 3],
    /// Target overrides.
    pub override_targets: [i32; 3],
    /// RX buffer size in use.
    pub buffer_size: usize,
    /// Bytes counted in the RX buffer.
    pub used_buffer: usize,
    /// Free planner blocks reported by the board (-1 unknown).
    pub grbl_blocks: i32,
    /// Free RX bytes reported by the board (-1 unknown).
    pub grbl_buffer: i32,
    /// Commands waiting to be sent.
    pub queued: usize,
    /// Commands sent and not answered.
    pub pending: usize,
    /// Commands in the job.
    pub job_target: usize,
    /// Job commands sent.
    pub job_sent: usize,
    /// Job commands executed.
    pub job_executed: usize,
    /// Job errors.
    pub job_errors: usize,
    /// Time of the job (pass) so far.
    pub job_time_ms: u64,
    /// Projected total time.
    pub projected_ms: i64,
    /// Issue that stopped the last job.
    pub last_issue: Issue,
    /// Passes left.
    pub loop_count: u32,
    /// Consecutive failed connections.
    pub failed_connections: u32,
    /// IP reported by a Wi-Fi board.
    pub detected_ip: Option<String>,
    /// Commands in the loaded program.
    pub program_lines: usize,
    /// Estimated time of the program.
    pub program_estimated_ms: i64,
    /// A jog can be sent now.
    pub jog_enabled: bool,
}

/// Decimal from a float value given by a front end (e.g. a jog step).
#[must_use]
pub fn decimal(v: f64) -> Decimal {
    decimal_from_f64(v).unwrap_or(Decimal::ZERO)
}
