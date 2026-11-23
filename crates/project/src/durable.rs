//! Crash-safe file replacement shared by project saves and app data.
//!
//! A file is never rewritten in place: the new contents go to a temporary
//! file in the same directory, are flushed to disk, and the temporary file is
//! then renamed over the target. A crash or a full disk at any point leaves
//! either the complete old file or the complete new one.

use std::io::{self, Write};
use std::path::Path;

/// Atomically replaces (or creates) `path` with `contents`.
///
/// On failure the target is untouched and the temporary file is removed.
pub(crate) fn replace_file(path: &Path, contents: &[u8]) -> io::Result<()> {
    let directory = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    // The temporary file must live in the target directory: a rename is only
    // atomic within one file system.
    let mut prefix = std::ffi::OsString::from(".");
    if let Some(name) = path.file_name() {
        prefix.push(name);
        prefix.push(".");
    }
    let mut builder = tempfile::Builder::new();
    builder.prefix(&prefix).suffix(".tmp");
    #[cfg(unix)]
    builder.permissions(unix::new_file_permissions());
    let mut temporary = builder.tempfile_in(directory)?;
    #[cfg(unix)]
    unix::keep_permissions(path, temporary.as_file())?;
    temporary.write_all(contents)?;
    temporary.as_file().sync_all()?;
    // Dropping the returned `NamedTempFile` on error deletes the temporary.
    temporary.persist(path).map_err(|e| e.error)?;
    sync_directory(directory);
    Ok(())
}

/// tempfile creates files readable by their owner only. A replaced file
/// keeps its permissions instead, and a new one gets those of a plain
/// `File::create` (`0o666` minus the umask).
#[cfg(unix)]
mod unix {
    use std::fs::{self, File, Permissions};
    use std::io;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    pub(super) fn new_file_permissions() -> Permissions {
        Permissions::from_mode(0o666)
    }

    pub(super) fn keep_permissions(target: &Path, file: &File) -> io::Result<()> {
        match fs::metadata(target) {
            Ok(existing) => file.set_permissions(existing.permissions()),
            Err(_) => Ok(()),
        }
    }
}

/// Makes the rename itself durable where the platform requires it.
#[cfg(unix)]
fn sync_directory(directory: &Path) {
    // Best effort: the data is already safe in either the old or the new
    // file, so a failure here cannot lose anything.
    if let Ok(dir) = std::fs::File::open(directory) {
        let _ = dir.sync_all();
    }
}

/// NTFS journals the rename's metadata, and directories cannot be opened as
/// plain files on Windows, so there is nothing further to flush.
#[cfg(not(unix))]
fn sync_directory(_directory: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(dir: &Path) -> Vec<String> {
        let mut names: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn creates_and_replaces_without_leaving_temporaries() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("data.json");
        replace_file(&path, b"first").unwrap();
        replace_file(&path, b"second").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        assert_eq!(entries(temp.path()), ["data.json"]);
    }

    #[test]
    fn failed_rename_keeps_the_target_and_removes_the_temporary() {
        let temp = tempfile::tempdir().unwrap();
        // A directory cannot be replaced by a file on any platform.
        let path = temp.path().join("occupied");
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("inner"), b"keep").unwrap();
        assert!(replace_file(&path, b"new").is_err());
        assert_eq!(entries(temp.path()), ["occupied"]);
        assert_eq!(std::fs::read(path.join("inner")).unwrap(), b"keep");
    }

    #[test]
    fn missing_directory_is_an_error() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("missing").join("data.json");
        assert!(replace_file(&path, b"x").is_err());
        assert!(!temp.path().join("missing").exists());
    }
}
