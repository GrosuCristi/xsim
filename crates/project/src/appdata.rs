//! Per-user application data: settings, recent projects and crash recovery.
//!
//! Location: `$XSIM_DATA_DIR` when set (tests and development runs use this
//! so they never touch the real profile), otherwise
//! `<platform local data dir>/xsim`. Several xsim instances may run at once
//! (Q19); every operation here is safe under concurrent use.
//!
//! Layout:
//! ```text
//! <root>/appdata.lock           serialises access to everything below
//! <root>/settings.json          AppSettings
//! <root>/recent.json            recent project paths, newest first
//! <root>/recovery/<session>/    one directory per running instance
//!     lock                      held (File::lock) while the instance runs
//!     snapshot.json             RecoverySnapshot, rewritten after edits
//! ```
//! A recovery directory whose `lock` is not held belongs to an instance that
//! crashed; its snapshot is offered for recovery at startup (Q41).
//!
//! ## Concurrency
//! Files are only ever replaced atomically. Readers hold a shared lock on
//! `appdata.lock` and writers an exclusive one, so a read never overlaps a
//! replacement (on Windows a replacement fails while a reader has the file
//! open) and a read-modify-write such as [`AppData::push_recent`] cannot lose
//! another instance's update. Creating a recovery session and scanning for
//! orphans are serialised the same way, so a scan never mistakes a session
//! that is still being set up for a crashed one.
//!
//! ## Tolerated data
//! Missing, unreadable or foreign files never cause errors on read: settings
//! fall back to defaults and the recent list to empty. A `recent.json` that
//! is a JSON array of path strings — the prototype's format — is read as is.

use crate::document::Project;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, ErrorKind};
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Environment variable that overrides the data directory.
pub const DATA_DIR_ENV: &str = "XSIM_DATA_DIR";

/// Maximum number of recent projects kept (Q46).
pub const MAX_RECENT: usize = 10;

const ROOT_LOCK: &str = "appdata.lock";
const SETTINGS_FILE: &str = "settings.json";
const RECENT_FILE: &str = "recent.json";
const RECOVERY_DIR: &str = "recovery";
const SESSION_LOCK: &str = "lock";
const SNAPSHOT_FILE: &str = "snapshot.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ThemePreference {
    #[default]
    Light,
    Dark,
}

/// Application-wide preferences (Q25, Q30).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub theme: ThemePreference,
    /// Interface scale factor, `0.75..=2.0`.
    pub ui_scale: f32,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: ThemePreference::Light,
            ui_scale: 1.0,
        }
    }
}

impl AppSettings {
    /// Permitted interface scale factors.
    pub const UI_SCALE_RANGE: RangeInclusive<f32> = 0.75..=2.0;

    /// These settings with `ui_scale` brought into [`Self::UI_SCALE_RANGE`]
    /// (a non-finite scale becomes the default).
    pub fn sanitized(self) -> Self {
        let range = Self::UI_SCALE_RANGE;
        let ui_scale = if self.ui_scale.is_finite() {
            self.ui_scale.clamp(*range.start(), *range.end())
        } else {
            Self::default().ui_scale
        };
        Self { ui_scale, ..self }
    }
}

/// Unsaved work of one instance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecoverySnapshot {
    /// File the project was opened from or last saved to, if any.
    pub source: Option<PathBuf>,
    pub project: Project,
}

/// A snapshot left behind by an instance that is no longer running.
#[derive(Debug, Clone, PartialEq)]
pub struct Orphan {
    /// The recovery directory; pass to [`AppData::discard_orphan`].
    pub dir: PathBuf,
    pub snapshot: RecoverySnapshot,
}

/// Handle to the data directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppData {
    root: PathBuf,
}

impl AppData {
    /// `$XSIM_DATA_DIR`, else the platform default.
    pub fn locate() -> Self {
        let root = std::env::var_os(DATA_DIR_ENV)
            .map(PathBuf::from)
            .or_else(|| dirs::data_local_dir().map(|d| d.join("xsim")))
            .unwrap_or_else(|| std::env::temp_dir().join("xsim"));
        Self { root }
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Stored settings, or defaults when missing or unreadable.
    pub fn settings(&self) -> AppSettings {
        self.read(SETTINGS_FILE)
            .and_then(|text| serde_json::from_str::<AppSettings>(&text).ok())
            .map(AppSettings::sanitized)
            .unwrap_or_default()
    }

    pub fn save_settings(&self, settings: &AppSettings) -> io::Result<()> {
        let text = json_text(&settings.clone().sanitized())?;
        let _lock = self.lock_exclusive()?;
        crate::durable::replace_file(&self.root.join(SETTINGS_FILE), text.as_bytes())
    }

    /// Recent projects, newest first, skipping files that no longer exist.
    /// Unreadable or foreign-format data yields an empty list.
    pub fn recent(&self) -> Vec<PathBuf> {
        let stored = self.read(RECENT_FILE).map(|t| parse_recent(&t));
        stored
            .unwrap_or_default()
            .into_iter()
            .filter(|p| p.is_file())
            .take(MAX_RECENT)
            .collect()
    }

    /// Moves `path` to the front of the recent list (deduplicated,
    /// at most [`MAX_RECENT`]).
    ///
    /// The path is stored in absolute form. Entries whose file is missing
    /// are kept (the drive may only be disconnected); [`Self::recent`] hides
    /// them. Corrupt or foreign data is replaced.
    pub fn push_recent(&self, path: &Path) -> io::Result<()> {
        let path = std::path::absolute(path)?;
        if path.to_str().is_none() {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "the project path is not valid Unicode and cannot be remembered",
            ));
        }
        let _lock = self.lock_exclusive()?;
        let file = self.root.join(RECENT_FILE);
        let stored = match fs::read_to_string(&file) {
            Ok(text) => parse_recent(&text),
            Err(_) => Vec::new(),
        };
        let list: Vec<PathBuf> = std::iter::once(path.clone())
            .chain(stored.into_iter().filter(|p| *p != path))
            .take(MAX_RECENT)
            .collect();
        crate::durable::replace_file(&file, json_text(&list)?.as_bytes())
    }

    /// Starts this instance's recovery session (creates and locks its dir).
    pub fn start_recovery(&self) -> io::Result<RecoverySession> {
        let _lock = self.lock_exclusive()?;
        let sessions = self.root.join(RECOVERY_DIR);
        fs::create_dir_all(&sessions)?;
        // `tempdir_in` picks a fresh unique name and removes the directory
        // again if anything below fails.
        let staging = tempfile::Builder::new()
            .prefix("session-")
            .tempdir_in(&sessions)?;
        let lock = File::create_new(staging.path().join(SESSION_LOCK))?;
        lock.lock()?;
        Ok(RecoverySession {
            dir: staging.keep(),
            lock: Some(lock),
        })
    }

    /// Snapshots left by crashed instances, oldest first. Never returns the
    /// sessions of running instances.
    ///
    /// Directories of crashed instances that hold no snapshot (the instance
    /// crashed before its first edit or after saving) are deleted. Snapshots
    /// this version cannot read are left in place.
    pub fn orphans(&self) -> Vec<Orphan> {
        let sessions = self.root.join(RECOVERY_DIR);
        // Checked first so that a scan never creates the data directory.
        if !sessions.is_dir() {
            return Vec::new();
        }
        let Ok(_lock) = self.lock_exclusive() else {
            return Vec::new();
        };
        let Ok(entries) = fs::read_dir(&sessions) else {
            return Vec::new();
        };
        let mut found = Vec::new();
        for entry in entries.flatten() {
            let dir = entry.path();
            match inspect_session(&dir) {
                Session::Running | Session::Unreadable => {}
                Session::Empty => {
                    let _ = fs::remove_dir_all(&dir);
                }
                Session::Orphaned { snapshot, modified } => {
                    found.push((modified, Orphan { dir, snapshot }));
                }
            }
        }
        found.sort_by(|(t1, a), (t2, b)| (t1, &a.dir).cmp(&(t2, &b.dir)));
        found.into_iter().map(|(_, orphan)| orphan).collect()
    }

    /// Deletes an orphaned recovery directory after it was restored or
    /// declined.
    ///
    /// Refuses anything but a session directory of this data directory's
    /// recovery area (one holding a session lock), and sessions whose
    /// instance is running. An orphan that is already gone (another instance
    /// discarded it) is not an error.
    pub fn discard_orphan(&self, orphan: &Orphan) -> io::Result<()> {
        let not_a_session = || {
            io::Error::new(
                ErrorKind::InvalidInput,
                "not a recovery session of this data directory",
            )
        };
        let sessions = self.root.join(RECOVERY_DIR);
        // `file_name` rules out paths such as `recovery/..`, whose parent
        // would also be the recovery area.
        if orphan.dir.file_name().is_none() || orphan.dir.parent() != Some(sessions.as_path()) {
            return Err(not_a_session());
        }
        let _lock = self.lock_exclusive()?;
        if !orphan.dir.exists() {
            return Ok(());
        }
        let lock = match open_session_lock(&orphan.dir) {
            Ok(lock) => lock,
            // `orphans` never returns a directory without a session lock.
            Err(e) if e.kind() == ErrorKind::NotFound => return Err(not_a_session()),
            Err(e) => return Err(e),
        };
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Err(io::Error::other(
                    "this recovery session belongs to a running xsim instance",
                ));
            }
            Err(TryLockError::Error(e)) => return Err(e),
        }
        // Only checked, then closed again: Windows cannot delete a directory
        // holding an open file. No instance can claim the directory meanwhile
        // because sessions are never reused.
        drop(lock);
        match fs::remove_dir_all(&orphan.dir) {
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }

    /// Contents of a data file under a shared lock; `None` when the file
    /// (or the whole data directory) is missing or unreadable.
    fn read(&self, name: &str) -> Option<String> {
        // Where no lock can be taken (a read-only data directory) nobody can
        // be replacing the file either, so reading without one is safe.
        let _lock = self
            .open_root_lock()
            .ok()
            .filter(|lock| lock.lock_shared().is_ok());
        fs::read_to_string(self.root.join(name)).ok()
    }

    /// Exclusive access to the data directory, created if needed. Released
    /// when the returned file is dropped.
    fn lock_exclusive(&self) -> io::Result<File> {
        fs::create_dir_all(&self.root)?;
        let lock = self.open_root_lock()?;
        lock.lock()?;
        Ok(lock)
    }

    fn open_root_lock(&self) -> io::Result<File> {
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.root.join(ROOT_LOCK))
    }
}

/// This instance's recovery directory. Dropping it removes the directory
/// (clean exit); a crash leaves it behind for [`AppData::orphans`].
#[derive(Debug)]
pub struct RecoverySession {
    dir: PathBuf,
    lock: Option<std::fs::File>,
}

impl RecoverySession {
    /// Atomically replaces the snapshot.
    ///
    /// The project must pass the same validation as a save, so a snapshot
    /// can always be restored.
    pub fn write(&self, snapshot: &RecoverySnapshot) -> io::Result<()> {
        crate::io::validate(&snapshot.project)
            .map_err(|e| io::Error::new(ErrorKind::InvalidData, e.to_string()))?;
        let text = json_text(snapshot)?;
        crate::durable::replace_file(&self.dir.join(SNAPSHOT_FILE), text.as_bytes())
    }

    /// Removes the snapshot (after a successful save).
    pub fn clear(&self) -> io::Result<()> {
        match fs::remove_file(self.dir.join(SNAPSHOT_FILE)) {
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }

    /// The session's recovery directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

impl Drop for RecoverySession {
    fn drop(&mut self) {
        // The snapshot goes first, while the lock is still held, so no other
        // instance can ever offer the work of a cleanly closed session.
        // Errors are ignored: a leftover directory without a snapshot is
        // cleaned up by the next `orphans` scan.
        let _ = self.clear();
        // The lock file must be closed before the directory can be removed
        // on Windows.
        drop(self.lock.take());
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// State of one recovery directory, inspected under the data-directory lock.
enum Session {
    /// Its instance holds the session lock.
    Running,
    /// Not a readable session (unexpected entry, unreadable or newer
    /// snapshot); left alone.
    Unreadable,
    /// Crashed without a snapshot to recover.
    Empty,
    Orphaned {
        snapshot: RecoverySnapshot,
        modified: SystemTime,
    },
}

fn inspect_session(dir: &Path) -> Session {
    if !dir.is_dir() {
        return Session::Unreadable;
    }
    let snapshot_path = dir.join(SNAPSHOT_FILE);
    // Sessions are created under the data-directory lock this scan holds,
    // so a missing session lock means the creator crashed half-way, before
    // it could write anything. A snapshot without a lock is not ours.
    let lock = match open_session_lock(dir) {
        Ok(lock) => lock,
        Err(e) if e.kind() == ErrorKind::NotFound && !snapshot_path.exists() => {
            return Session::Empty;
        }
        Err(_) => return Session::Unreadable,
    };
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return Session::Running,
        Err(TryLockError::Error(_)) => return Session::Unreadable,
    }
    let text = match fs::read_to_string(&snapshot_path) {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::NotFound => return Session::Empty,
        Err(_) => return Session::Unreadable,
    };
    let Some(snapshot) = parse_snapshot(&text) else {
        return Session::Unreadable;
    };
    let modified = fs::metadata(&snapshot_path)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH);
    Session::Orphaned { snapshot, modified }
}

fn open_session_lock(dir: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(dir.join(SESSION_LOCK))
}

/// A snapshot's embedded project goes through the normal schema detection,
/// so snapshots of older or newer versions are restored like their files.
fn parse_snapshot(text: &str) -> Option<RecoverySnapshot> {
    let value: Value = serde_json::from_str(text).ok()?;
    let source = match value.get("source") {
        None | Some(Value::Null) => None,
        Some(source) => Some(PathBuf::deserialize(source).ok()?),
    };
    let project = crate::io::project_from_value(value.get("project")?)
        .ok()?
        .project;
    Some(RecoverySnapshot { source, project })
}

/// Paths in a recent list. Anything other than an array of strings is
/// foreign and yields nothing; duplicates keep their first (newest) entry.
fn parse_recent(text: &str) -> Vec<PathBuf> {
    let Ok(Value::Array(items)) = serde_json::from_str(text) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = Vec::new();
    for path in items.iter().filter_map(Value::as_str).map(PathBuf::from) {
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths
}

/// Pretty JSON with a trailing newline, like project files.
fn json_text(value: &impl Serialize) -> io::Result<String> {
    let mut text = serde_json::to_string_pretty(value)
        .map_err(|e| io::Error::new(ErrorKind::InvalidData, e))?;
    text.push('\n');
    Ok(text)
}
