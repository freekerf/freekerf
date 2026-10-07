//! Settings of FreeKerf, stored as TOML (`settings.toml`).
//!
//! The file has a `version`; older versions are migrated when loaded and newer
//! ones are refused (written by a newer FreeKerf). The JSON schema of the file is
//! generated from this model ([`json_schema`]) and checked in CI. Reading and writing
//! the file goes through [`SettingsStore`], so this crate does no I/O.

use std::collections::BTreeMap;

use freekerf_protocol::host::{DEFAULT_FOOTER, DEFAULT_HEADER, DEFAULT_PASSES};
use freekerf_protocol::{AutoCooling, FirmwareKind, HostOptions, StreamingMode};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Current version of the settings file.
pub const VERSION: u32 = 1;

/// Errors.
#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    /// Not valid TOML or not matching the model.
    #[error("invalid settings: {0}")]
    Invalid(String),
    /// Written by a newer FreeKerf.
    #[error("settings version {found} is newer than this FreeKerf supports ({supported})")]
    TooNew {
        /// Version in the file.
        found: u32,
        /// Newest supported.
        supported: u32,
    },
    /// Unknown key.
    #[error("unknown setting `{0}`")]
    UnknownKey(String),
    /// A key only the user may change in the app (automation, permissions).
    #[error("`{0}` can only be changed by the user")]
    UserOnly(String),
    /// Storage failure.
    #[error("storage: {0}")]
    Storage(String),
}

/// Where settings are kept (a file in the app; memory in tests).
pub trait SettingsStore: Send + Sync {
    /// The stored text, `None` when nothing was saved yet.
    ///
    /// # Errors
    /// On storage failures.
    fn load(&self) -> Result<Option<String>, SettingsError>;

    /// Replaces the stored text.
    ///
    /// # Errors
    /// On storage failures.
    fn save(&self, text: &str) -> Result<(), SettingsError>;
}

/// In-memory store (tests, `--no-settings`).
#[derive(Debug, Default)]
pub struct MemoryStore(std::sync::Mutex<Option<String>>);

impl SettingsStore for MemoryStore {
    fn load(&self) -> Result<Option<String>, SettingsError> {
        Ok(self
            .0
            .lock()
            .map_err(|e| SettingsError::Storage(e.to_string()))?
            .clone())
    }

    fn save(&self, text: &str) -> Result<(), SettingsError> {
        *self.0.lock().map_err(|e| SettingsError::Storage(e.to_string()))? = Some(text.to_owned());
        Ok(())
    }
}

/// Firmware family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Firmware {
    /// Grbl, grblHAL and compatible.
    Grbl,
    /// Smoothieware.
    Smoothie,
    /// Marlin.
    Marlin,
    /// VigoWork.
    VigoWork,
}

impl From<Firmware> for FirmwareKind {
    fn from(f: Firmware) -> Self {
        match f {
            Firmware::Grbl => Self::Grbl,
            Firmware::Smoothie => Self::Smoothie,
            Firmware::Marlin => Self::Marlin,
            Firmware::VigoWork => Self::VigoWork,
        }
    }
}

/// Streaming mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Streaming {
    /// Fill the RX buffer (character counting).
    Buffered,
    /// One line at a time.
    Synchronous,
    /// One line at a time, failed job lines retried 3 times.
    RepeatOnError,
}

impl From<Streaming> for StreamingMode {
    fn from(s: Streaming) -> Self {
        match s {
            Streaming::Buffered => Self::Buffered,
            Streaming::Synchronous => Self::Synchronous,
            Streaming::RepeatOnError => Self::RepeatOnError,
        }
    }
}

/// How to reach the machine and how to talk to it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct MachineSettings {
    /// Firmware family.
    pub firmware: Firmware,
    /// Last serial port used.
    pub port: Option<String>,
    /// Baud rate.
    pub baud: u32,
    /// Streaming mode.
    pub streaming_mode: Streaming,
    /// Soft reset when connecting.
    pub reset_on_connect: bool,
    /// Ask `$I` when connecting.
    pub query_machine_info: bool,
    /// Status query period, ms.
    pub status_query_ms: u64,
    /// Hardware PWM: `S0` does not burn.
    pub support_pwm: bool,
    /// Continuous jog (jog while pressed).
    pub continuous_jog: bool,
    /// Automatic cooling cycles.
    pub auto_cooling: AutoCoolingSettings,
}

impl Default for MachineSettings {
    fn default() -> Self {
        Self {
            firmware: Firmware::Grbl,
            port: None,
            baud: 115_200,
            streaming_mode: Streaming::Buffered,
            reset_on_connect: true,
            query_machine_info: true,
            status_query_ms: 500,
            support_pwm: true,
            continuous_jog: false,
            auto_cooling: AutoCoolingSettings::default(),
        }
    }
}

/// Automatic cooling: hold the job `off_s` every `on_s` seconds of work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct AutoCoolingSettings {
    /// Enabled.
    pub enabled: bool,
    /// Work time, seconds.
    pub on_s: u64,
    /// Pause, seconds.
    pub off_s: u64,
}

impl Default for AutoCoolingSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            on_s: 600,
            off_s: 60,
        }
    }
}

/// Code around jobs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct JobSettings {
    /// Before the program (`[expr]` allowed; `!`, `~`, `?`, `ctrl-x`, `0xNN` are sent at once).
    pub header: String,
    /// Between passes.
    pub passes: String,
    /// After the program.
    pub footer: String,
}

impl Default for JobSettings {
    fn default() -> Self {
        Self {
            header: DEFAULT_HEADER.into(),
            passes: DEFAULT_PASSES.into(),
            footer: DEFAULT_FOOTER.into(),
        }
    }
}

/// Jog defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct JogSettings {
    /// Step, mm.
    pub step_mm: f64,
    /// Feed, mm/min.
    pub speed_mm_min: f64,
}

impl Default for JogSettings {
    fn default() -> Self {
        Self {
            step_mm: 1.0,
            speed_mm_min: 1000.0,
        }
    }
}

/// MCP server settings (§9 of the plan). Changed only by the user, never by
/// actions, scripts or agents.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct McpSettings {
    /// MCP server enabled (off by default).
    pub enabled: bool,
    /// Agents may use `machine` actions.
    pub allow_machine: bool,
    /// Agents may request `laser` actions (always confirmed by a human on screen).
    pub allow_laser: bool,
}

/// Automation (scripts and agents).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct AutomationSettings {
    /// MCP.
    pub mcp: McpSettings,
}

/// The whole settings file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    /// Format version.
    pub version: u32,
    /// Machine connection.
    pub machine: MachineSettings,
    /// Job custom code.
    pub job: JobSettings,
    /// Jog.
    pub jog: JogSettings,
    /// Automation (user-only).
    pub automation: AutomationSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: VERSION,
            machine: MachineSettings::default(),
            job: JobSettings::default(),
            jog: JogSettings::default(),
            automation: AutomationSettings::default(),
        }
    }
}

impl Settings {
    /// Parses a settings file, migrating older versions.
    ///
    /// # Errors
    /// Invalid TOML, unknown keys, or a version newer than [`VERSION`].
    pub fn from_toml(text: &str) -> Result<Self, SettingsError> {
        let mut value: toml::Table = text
            .parse()
            .map_err(|e: toml::de::Error| SettingsError::Invalid(e.to_string()))?;
        let found = match value.get("version") {
            None => 0,
            Some(v) => v
                .as_integer()
                .and_then(|v| u32::try_from(v).ok())
                .ok_or_else(|| SettingsError::Invalid("`version` must be a positive integer".into()))?,
        };
        if found > VERSION {
            return Err(SettingsError::TooNew {
                found,
                supported: VERSION,
            });
        }
        migrate(&mut value, found);
        let s: Self = toml::Value::Table(value)
            .try_into()
            .map_err(|e: toml::de::Error| SettingsError::Invalid(e.to_string()))?;
        Ok(s)
    }

    /// The file contents.
    ///
    /// # Errors
    /// Never in practice (the model always serializes).
    pub fn to_toml(&self) -> Result<String, SettingsError> {
        toml::to_string_pretty(self).map_err(|e| SettingsError::Invalid(e.to_string()))
    }

    /// Loads from a store (defaults when nothing is stored).
    ///
    /// # Errors
    /// Invalid or too new file, storage errors.
    pub fn load(store: &dyn SettingsStore) -> Result<Self, SettingsError> {
        store
            .load()?
            .map_or_else(|| Ok(Self::default()), |t| Self::from_toml(&t))
    }

    /// Saves to a store.
    ///
    /// # Errors
    /// Storage errors.
    pub fn save(&self, store: &dyn SettingsStore) -> Result<(), SettingsError> {
        store.save(&self.to_toml()?)
    }

    /// The protocol options these settings give.
    #[must_use]
    pub fn host_options(&self) -> HostOptions {
        let m = &self.machine;
        HostOptions {
            streaming_mode: m.streaming_mode.into(),
            reset_on_connect: m.reset_on_connect,
            query_machine_info: m.query_machine_info,
            continuous_jog: m.continuous_jog,
            status_query_ms: m.status_query_ms.max(50),
            custom_header: self.job.header.clone(),
            custom_passes: self.job.passes.clone(),
            custom_footer: self.job.footer.clone(),
            support_pwm: m.support_pwm,
            auto_cooling: m.auto_cooling.enabled.then(|| AutoCooling {
                on_ms: m.auto_cooling.on_s * 1000,
                off_ms: m.auto_cooling.off_s * 1000,
            }),
            jog_step_mm: self.jog.step_mm,
            jog_speed_mm_min: self.jog.speed_mm_min,
            last_known_version: None,
            last_known_settings: None,
        }
    }

    /// Value of a dotted key (`machine.baud`) as JSON.
    ///
    /// # Errors
    /// Unknown key.
    pub fn get(&self, key: &str) -> Result<serde_json::Value, SettingsError> {
        let root = serde_json::to_value(self).map_err(|e| SettingsError::Invalid(e.to_string()))?;
        key.split('.')
            .try_fold(&root, |v, part| v.get(part))
            .cloned()
            .ok_or_else(|| SettingsError::UnknownKey(key.to_owned()))
    }

    /// Sets a dotted key from JSON. `automation.*` is refused when `user` is false:
    /// only the user changes it, in the app or with `freekerf config set`.
    ///
    /// # Errors
    /// Unknown key, user-only key, or a value of the wrong type.
    pub fn set(&mut self, key: &str, value: serde_json::Value, user: bool) -> Result<(), SettingsError> {
        if !user && (key == "automation" || key.starts_with("automation.") || key == "version") {
            return Err(SettingsError::UserOnly(key.to_owned()));
        }
        let mut root = serde_json::to_value(&*self).map_err(|e| SettingsError::Invalid(e.to_string()))?;
        let mut slot = &mut root;
        for part in key.split('.') {
            slot = slot
                .get_mut(part)
                .ok_or_else(|| SettingsError::UnknownKey(key.to_owned()))?;
        }
        *slot = value;
        *self = serde_json::from_value(root).map_err(|e| SettingsError::Invalid(format!("{key}: {e}")))?;
        Ok(())
    }

    /// Dotted names of every leaf setting.
    #[must_use]
    pub fn keys(&self) -> Vec<String> {
        fn walk(prefix: &str, v: &serde_json::Value, out: &mut Vec<String>) {
            match v {
                serde_json::Value::Object(map) => {
                    for (k, v) in map {
                        let p = if prefix.is_empty() {
                            k.clone()
                        } else {
                            format!("{prefix}.{k}")
                        };
                        walk(&p, v, out);
                    }
                }
                _ => out.push(prefix.to_owned()),
            }
        }
        let mut out = Vec::new();
        if let Ok(v) = serde_json::to_value(self) {
            walk("", &v, &mut out);
        }
        out
    }
}

/// Upgrades the raw table from `from` to [`VERSION`].
fn migrate(table: &mut toml::Table, from: u32) {
    if from < 1 {
        // version 0 (unversioned drafts): `[machine] streaming = ...` was renamed
        if let Some(toml::Value::Table(m)) = table.get_mut("machine")
            && let Some(v) = m.remove("streaming")
        {
            m.insert("streaming_mode".into(), v);
        }
    }
    table.insert("version".into(), toml::Value::Integer(i64::from(VERSION)));
}

/// JSON schema of the settings file (committed as `doc/schema/settings.schema.json`).
#[must_use]
pub fn json_schema() -> String {
    let schema = schemars::schema_for!(Settings);
    let mut s = serde_json::to_string_pretty(&schema).unwrap_or_default();
    s.push('\n');
    s
}

/// What the app remembers about the last machine (not user settings).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MachineCache {
    /// Last firmware version seen.
    pub version: Option<freekerf_protocol::FirmwareVersion>,
    /// Last `$$` table read.
    pub settings: Option<freekerf_protocol::GrblSettings>,
    /// Laser life counters.
    pub laser_life: Vec<freekerf_protocol::LaserLifeCounter>,
    /// Free-form values for later use.
    pub extra: BTreeMap<String, String>,
}

impl MachineCache {
    /// Parses the cache file, JSON (an unreadable cache is replaced by an empty one).
    #[must_use]
    pub fn from_json(text: &str) -> Self {
        serde_json::from_str(text).unwrap_or_default()
    }

    /// Serializes the cache as JSON (not meant to be edited by hand).
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip() {
        let s = Settings::default();
        let text = s.to_toml().unwrap();
        assert!(text.contains("version = 1"));
        assert_eq!(Settings::from_toml(&text).unwrap(), s);
        assert_eq!(Settings::from_toml("").unwrap().version, VERSION);
    }

    #[test]
    fn partial_files_use_defaults() {
        let s = Settings::from_toml("version = 1\n[machine]\nbaud = 250000\n").unwrap();
        assert_eq!(s.machine.baud, 250_000);
        assert!(s.machine.reset_on_connect);
        assert!(!s.automation.mcp.enabled);
    }

    #[test]
    fn migration_from_version_0() {
        let s = Settings::from_toml("[machine]\nstreaming = \"Synchronous\"\n").unwrap();
        assert_eq!(s.machine.streaming_mode, Streaming::Synchronous);
        assert_eq!(s.version, 1);
    }

    #[test]
    fn errors() {
        assert!(matches!(
            Settings::from_toml("version = 99"),
            Err(SettingsError::TooNew { found: 99, .. })
        ));
        assert!(matches!(
            Settings::from_toml("version = -1"),
            Err(SettingsError::Invalid(_))
        ));
        assert!(matches!(
            Settings::from_toml("nope = 1"),
            Err(SettingsError::Invalid(_))
        ));
        assert!(matches!(
            Settings::from_toml("[machine"),
            Err(SettingsError::Invalid(_))
        ));
        let e = Settings::from_toml("version = 99").unwrap_err();
        assert!(e.to_string().contains("newer"));
    }

    #[test]
    fn get_and_set_dotted_keys() {
        let mut s = Settings::default();
        assert_eq!(s.get("machine.baud").unwrap(), serde_json::json!(115_200));
        s.set(
            "machine.streaming_mode",
            serde_json::json!("RepeatOnError"),
            false,
        )
        .unwrap();
        assert_eq!(s.machine.streaming_mode, Streaming::RepeatOnError);
        assert!(matches!(
            s.set("machine.nope", serde_json::json!(1), false),
            Err(SettingsError::UnknownKey(_))
        ));
        assert!(matches!(
            s.set("machine.baud", serde_json::json!("fast"), false),
            Err(SettingsError::Invalid(_))
        ));
        assert!(matches!(
            s.set("automation.mcp.enabled", serde_json::json!(true), false),
            Err(SettingsError::UserOnly(_))
        ));
        s.set("automation.mcp.enabled", serde_json::json!(true), true)
            .unwrap();
        assert!(s.automation.mcp.enabled);
        assert!(s.get("nope").is_err());
        assert!(s.keys().contains(&"machine.auto_cooling.on_s".to_owned()));
    }

    #[test]
    fn host_options_follow_the_settings() {
        let mut s = Settings::default();
        s.machine.auto_cooling.enabled = true;
        s.machine.status_query_ms = 1;
        s.machine.firmware = Firmware::Marlin;
        s.jog.step_mm = 2.5;
        s.jog.speed_mm_min = 1200.0;
        let o = s.host_options();
        assert_eq!((o.jog_step_mm, o.jog_speed_mm_min), (2.5, 1200.0));
        assert_eq!(
            o.auto_cooling,
            Some(AutoCooling {
                on_ms: 600_000,
                off_ms: 60_000
            })
        );
        assert_eq!(o.status_query_ms, 50);
        assert_eq!(FirmwareKind::from(s.machine.firmware), FirmwareKind::Marlin);
        for (f, k) in [
            (Firmware::Grbl, FirmwareKind::Grbl),
            (Firmware::Smoothie, FirmwareKind::Smoothie),
            (Firmware::VigoWork, FirmwareKind::VigoWork),
        ] {
            assert_eq!(FirmwareKind::from(f), k);
        }
        for (a, b) in [
            (Streaming::Buffered, StreamingMode::Buffered),
            (Streaming::Synchronous, StreamingMode::Synchronous),
            (Streaming::RepeatOnError, StreamingMode::RepeatOnError),
        ] {
            assert_eq!(StreamingMode::from(a), b);
        }
    }

    #[test]
    fn stores_and_cache() {
        let store = MemoryStore::default();
        assert_eq!(Settings::load(&store).unwrap(), Settings::default());
        let mut s = Settings::default();
        s.jog.step_mm = 0.5;
        s.save(&store).unwrap();
        assert_eq!(Settings::load(&store).unwrap().jog.step_mm, 0.5);
        let mut c = MachineCache::default();
        c.extra.insert("a".into(), "b".into());
        c.laser_life
            .push(freekerf_protocol::LaserLifeCounter::new("id", "Default"));
        c.version = Some(freekerf_protocol::FirmwareVersion::with_build(1, 1, 'f'));
        let mut table = freekerf_protocol::GrblSettings::new(c.version.clone());
        table.set(110, "6000");
        c.settings = Some(table);
        assert_eq!(MachineCache::from_json(&c.to_json()), c);
        assert_eq!(MachineCache::from_json("garbage ="), MachineCache::default());
        assert!(json_schema().contains("\"MachineSettings\""));
    }
}
