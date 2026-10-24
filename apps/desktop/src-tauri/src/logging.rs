//! The app's log: standard error (when there is a console) and a file per run in
//! the logs directory, so what happened before a crash or an odd machine move can
//! be read afterwards (doc/features/desktop-app.md, Logs).

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

/// The current run's log file.
pub const FILE_NAME: &str = "freekerf.log";
/// Logs of earlier runs kept next to it (`freekerf.1.log` is the latest).
pub const KEPT: usize = 5;
/// Past this size the file is rotated while the app runs.
pub const MAX_BYTES: u64 = 20 * 1024 * 1024;
/// What the file records when `RUST_LOG` is not set: what the person and the
/// machine did (actions that move the machine, machine states, console rows).
pub const FILE_FILTER: &str = "info,freekerf_machine=debug";

/// Starts logging to standard error (`RUST_LOG`, default `warn`) and, when `dir`
/// is known, to [`FILE_NAME`] there (`RUST_LOG`, default [`FILE_FILTER`]); panics
/// are logged before the default hook runs. Returns the log file's path.
pub fn init(dir: Option<&Path>) -> Option<PathBuf> {
    let opened = dir.map(|d| (d, LogFile::open(d)));
    let file = match &opened {
        Some((_, Ok(f))) => Some(f.clone()),
        _ => None,
    };
    let path = file.as_ref().map(LogFile::path);
    let stderr = tracing_subscriber::fmt::layer()
        .with_writer(io::stderr)
        .with_filter(filter("warn"));
    let file_layer = file.map(|f| {
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_thread_names(true)
            .with_writer(f)
            .with_filter(filter(FILE_FILTER))
    });
    let _ = tracing_subscriber::registry()
        .with(stderr)
        .with(file_layer)
        .try_init();
    if let Some((d, Err(e))) = &opened {
        tracing::warn!("no log file in {}: {e}", d.display());
    }
    log_panics();
    path
}

fn filter(default: &str) -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default))
}

/// Logs every panic (message, place, thread, backtrace), then runs the default
/// hook: a release build on Windows has no console to print it on.
fn log_panics() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let backtrace = std::backtrace::Backtrace::force_capture();
        tracing::error!(
            thread = thread.name().unwrap_or("unnamed"),
            "panic: {info}\n{backtrace}"
        );
        default(info);
    }));
}

/// The log file, shared by the threads that write to it.
#[derive(Debug, Clone)]
pub struct LogFile(Arc<Mutex<State>>);

#[derive(Debug)]
struct State {
    dir: PathBuf,
    file: File,
    written: u64,
}

impl LogFile {
    /// Rotates the logs of earlier runs in `dir` and opens a new [`FILE_NAME`].
    ///
    /// # Errors
    /// The directory or the file cannot be created.
    pub fn open(dir: &Path) -> io::Result<Self> {
        fs::create_dir_all(dir)?;
        rotate(dir);
        let file = create(dir)?;
        Ok(Self(Arc::new(Mutex::new(State {
            dir: dir.to_path_buf(),
            file,
            written: 0,
        }))))
    }

    /// Path of the current file.
    #[must_use]
    pub fn path(&self) -> PathBuf {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .dir
            .join(FILE_NAME)
    }
}

fn create(dir: &Path) -> io::Result<File> {
    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(dir.join(FILE_NAME))
}

/// `freekerf.log` → `freekerf.1.log` → … → `freekerf.KEPT.log` (dropped).
fn rotate(dir: &Path) {
    let numbered = |n: usize| dir.join(format!("freekerf.{n}.log"));
    let _ = fs::remove_file(numbered(KEPT));
    for n in (1..KEPT).rev() {
        let _ = fs::rename(numbered(n), numbered(n + 1));
    }
    let _ = fs::rename(dir.join(FILE_NAME), numbered(1));
}

impl Write for LogFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut s = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if s.written >= MAX_BYTES {
            rotate(&s.dir);
            s.file = create(&s.dir)?;
            s.written = 0;
        }
        s.file.write_all(buf)?;
        s.written += buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).file.flush()
    }
}

impl<'a> MakeWriter<'a> for LogFile {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("fk-log-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn keeps_earlier_runs() {
        let d = temp_dir("runs");
        for run in 0..KEPT + 2 {
            let mut f = LogFile::open(&d).unwrap();
            writeln!(f, "run {run}").unwrap();
        }
        let read = |p: PathBuf| fs::read_to_string(p).unwrap();
        assert_eq!(read(d.join(FILE_NAME)), format!("run {}\n", KEPT + 1));
        assert_eq!(read(d.join("freekerf.1.log")), format!("run {KEPT}\n"));
        assert_eq!(read(d.join(format!("freekerf.{KEPT}.log"))), "run 1\n");
        assert!(!d.join(format!("freekerf.{}.log", KEPT + 1)).exists());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn rotates_a_full_file() {
        let d = temp_dir("full");
        let mut f = LogFile::open(&d).unwrap();
        f.0.lock().unwrap().written = MAX_BYTES;
        writeln!(f, "after").unwrap();
        assert_eq!(fs::read_to_string(d.join(FILE_NAME)).unwrap(), "after\n");
        assert!(d.join("freekerf.1.log").exists());
        let _ = fs::remove_dir_all(&d);
    }
}
