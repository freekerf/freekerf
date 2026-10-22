//! Operating system services: where FreeKerf keeps its files, writing them
//! safely (a crash never leaves a half-written settings file), and the installed
//! fonts ([`SystemFonts`]).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

mod fonts;

pub use fonts::SystemFonts;

/// Errors of the platform layer.
#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    /// No home/config directory could be determined.
    #[error("cannot determine the configuration directory")]
    NoConfigDir,
    /// An external program failed to start or exited with an error.
    #[error("{program}: {message}")]
    Program {
        /// Program run.
        program: String,
        /// What went wrong (exit status and standard error, or the start error).
        message: String,
    },
    /// I/O error on a path.
    #[error("{path}: {source}")]
    Io {
        /// Path involved.
        path: PathBuf,
        /// Cause.
        source: std::io::Error,
    },
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> PlatformError + '_ {
    move |source| PlatformError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Directories of the application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dirs {
    /// Settings (`settings.toml`).
    pub config: PathBuf,
    /// Data kept between runs (laser life counters, machine cache).
    pub data: PathBuf,
    /// Files of the running instance (the local endpoint and its token):
    /// `$XDG_RUNTIME_DIR/freekerf` on Linux (`$XDG_RUNTIME_DIR/app/$FLATPAK_ID` in
    /// a Flatpak), the data directory elsewhere.
    pub runtime: PathBuf,
}

impl Dirs {
    /// The per-user directories (`$FREEKERF_HOME` overrides both, for tests and
    /// portable installs).
    ///
    /// # Errors
    /// When no home directory is known.
    pub fn user() -> Result<Self, PlatformError> {
        if let Some(home) = std::env::var_os("FREEKERF_HOME") {
            let home = PathBuf::from(home);
            return Ok(Self::under(&home));
        }
        let p = directories::ProjectDirs::from("org", "FreeKerf", "freekerf")
            .ok_or(PlatformError::NoConfigDir)?;
        Ok(Self {
            config: p.config_dir().to_path_buf(),
            data: p.data_dir().to_path_buf(),
            runtime: flatpak_runtime_dir(
                std::env::var_os("FLATPAK_ID"),
                std::env::var_os("XDG_RUNTIME_DIR"),
            )
            .unwrap_or_else(|| p.runtime_dir().unwrap_or_else(|| p.data_dir()).to_path_buf()),
        })
    }

    /// Both directories under `root` (tests, portable installs).
    #[must_use]
    pub fn under(root: &Path) -> Self {
        Self {
            config: root.to_path_buf(),
            data: root.to_path_buf(),
            runtime: root.to_path_buf(),
        }
    }

    /// `settings.toml`.
    #[must_use]
    pub fn settings_file(&self) -> PathBuf {
        self.config.join("settings.toml")
    }

    /// File in the data directory.
    #[must_use]
    pub fn data_file(&self, name: &str) -> PathBuf {
        self.data.join(name)
    }

    /// The user's own fonts (TrueType, OpenType, single-line SVG fonts).
    #[must_use]
    pub fn fonts_dir(&self) -> PathBuf {
        self.data.join("fonts")
    }
}

/// Writes `contents` to `path` atomically: a temporary file in the same directory
/// is written, flushed and renamed over the target.
///
/// # Errors
/// On any I/O error (the target is then unchanged).
pub fn write_atomic(path: &Path, contents: &[u8]) -> Result<(), PlatformError> {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir).map_err(io(dir))?;
    let name = path
        .file_name()
        .map_or_else(|| "file".into(), |n| n.to_string_lossy().into_owned());
    let tmp = dir.join(format!(".{name}.tmp{}", std::process::id()));
    let result = (|| {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(contents)?;
        f.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result.map_err(io(path))
}

/// Reads a file, `None` when it does not exist.
///
/// # Errors
/// On I/O errors other than "not found".
pub fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, PlatformError> {
    match fs::read(path) {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io(path)(e)),
    }
}

/// Reads a whole file.
///
/// # Errors
/// On I/O errors.
pub fn read(path: &Path) -> Result<Vec<u8>, PlatformError> {
    fs::read(path).map_err(io(path))
}

/// Runs `program` with `args` (no shell) and returns its standard output.
///
/// # Errors
/// The program cannot be started or exits with a failure status.
pub fn run_capture(program: &Path, args: &[String]) -> Result<Vec<u8>, PlatformError> {
    let name = program.display().to_string();
    let out = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| PlatformError::Program {
            program: name.clone(),
            message: e.to_string(),
        })?;
    if !out.status.success() {
        return Err(PlatformError::Program {
            program: name,
            message: format!("{}: {}", out.status, String::from_utf8_lossy(&out.stderr).trim()),
        });
    }
    Ok(out.stdout)
}

/// Deletes a file, ignoring a missing one.
///
/// # Errors
/// On I/O errors other than "not found".
pub fn remove_file(path: &Path) -> Result<(), PlatformError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io(path)(e)),
    }
}

/// In a Flatpak, `$XDG_RUNTIME_DIR` is private to each sandbox instance, but
/// `$XDG_RUNTIME_DIR/app/$FLATPAK_ID` is shared by every instance of the app: the
/// window and `flatpak run --command=freekerf <id> mcp` meet there.
fn flatpak_runtime_dir(
    flatpak_id: Option<std::ffi::OsString>,
    xdg_runtime_dir: Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    let id = flatpak_id.filter(|v| !v.is_empty())?;
    let base = xdg_runtime_dir.filter(|v| !v.is_empty())?;
    Some(PathBuf::from(base).join("app").join(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flatpak_instances_share_the_app_runtime_dir() {
        let id = Some("io.github.freekerf.FreeKerf".into());
        let xdg = Some("/run/user/1000".into());
        assert_eq!(
            flatpak_runtime_dir(id.clone(), xdg.clone()),
            Some(PathBuf::from("/run/user/1000/app/io.github.freekerf.FreeKerf"))
        );
        // not in a Flatpak, or no runtime directory: the usual one
        assert_eq!(flatpak_runtime_dir(None, xdg.clone()), None);
        assert_eq!(flatpak_runtime_dir(Some("".into()), xdg), None);
        assert_eq!(flatpak_runtime_dir(id, None), None);
    }

    #[cfg(unix)]
    #[test]
    fn programs_and_removal() {
        let out = run_capture(Path::new("echo"), &["hi".into()]).unwrap();
        assert_eq!(out, b"hi\n");
        assert!(run_capture(Path::new("false"), &[]).is_err());
        let e = run_capture(Path::new("/no/such/program"), &[]).unwrap_err();
        assert!(e.to_string().contains("/no/such/program"));
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x");
        write_atomic(&p, b"1").unwrap();
        remove_file(&p).unwrap();
        remove_file(&p).unwrap();
        assert!(remove_file(dir.path()).is_err());
    }

    #[test]
    fn atomic_write_and_read() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("sub/settings.toml");
        assert_eq!(read_optional(&p).unwrap(), None);
        write_atomic(&p, b"a = 1\n").unwrap();
        write_atomic(&p, b"a = 2\n").unwrap();
        assert_eq!(read(&p).unwrap(), b"a = 2\n");
        assert_eq!(fs::read_dir(p.parent().unwrap()).unwrap().count(), 1);
        assert!(read(&dir.path().join("missing")).is_err());
        // a directory in the way: the write fails and leaves nothing behind
        let blocked = dir.path().join("blocked");
        fs::create_dir_all(blocked.join("x")).unwrap();
        assert!(write_atomic(&blocked, b"x").is_err());
        assert!(read_optional(dir.path()).is_err());
    }

    #[test]
    fn directories() {
        let d = Dirs::under(Path::new("/tmp/fk"));
        assert_eq!(d.settings_file(), PathBuf::from("/tmp/fk/settings.toml"));
        assert_eq!(d.data_file("x"), PathBuf::from("/tmp/fk/x"));
        assert!(Dirs::user().is_ok());
        let e = PlatformError::NoConfigDir;
        assert!(e.to_string().contains("configuration"));
    }
}
