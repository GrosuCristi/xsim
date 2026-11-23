//! Application data: settings, recent projects and crash recovery, including
//! several concurrent instances and real crashed processes.
//!
//! Every test works in its own temporary data directory through
//! [`AppData::at`]; the real profile is never touched. Crashes are simulated
//! by re-running this test binary as a child process that exits without
//! running destructors, exactly like a crashed xsim.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};
use xsim_project::Project;
use xsim_project::appdata::{
    AppData, AppSettings, DATA_DIR_ENV, MAX_RECENT, Orphan, RecoverySnapshot, ThemePreference,
};

/// Environment variables understood by [`subprocess_helper`].
const CHILD_ROLE: &str = "XSIM_TEST_CHILD_ROLE";
const CHILD_ROOT: &str = "XSIM_TEST_CHILD_ROOT";
const CHILD_PROJECT: &str = "XSIM_TEST_CHILD_PROJECT";

fn snapshot(name: &str) -> RecoverySnapshot {
    let mut project = Project::new(name);
    project.circuits[0].name = "Unsaved work".into();
    RecoverySnapshot {
        source: Some(PathBuf::from("C:/projects/amp.xsim_proj")),
        project,
    }
}

/// Runs in a child process started by [`run_child`]; does nothing otherwise.
#[test]
#[ignore = "helper: executed in a subprocess by the recovery and locate tests"]
fn subprocess_helper() {
    let Ok(role) = std::env::var(CHILD_ROLE) else {
        return;
    };
    let root = PathBuf::from(std::env::var_os(CHILD_ROOT).expect("root is set"));
    let data = AppData::at(&root);
    match role.as_str() {
        "crash-with-snapshot" => {
            let session = data.start_recovery().unwrap();
            let name = std::env::var(CHILD_PROJECT).unwrap();
            session.write(&snapshot(&name)).unwrap();
            // Exit without dropping the session, like a crash.
            std::process::exit(0);
        }
        "crash-without-snapshot" => {
            let _session = data.start_recovery().unwrap();
            std::process::exit(0);
        }
        "locate" => {
            assert_eq!(AppData::locate().root(), root.as_path());
        }
        other => panic!("unknown child role {other}"),
    }
}

fn run_child(role: &str, root: &Path, project: &str, data_dir_env: Option<&Path>) {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "subprocess_helper",
            "--exact",
            "--ignored",
            "--test-threads=1",
        ])
        .env(CHILD_ROLE, role)
        .env(CHILD_ROOT, root)
        .env(CHILD_PROJECT, project)
        .env_remove(DATA_DIR_ENV);
    if let Some(dir) = data_dir_env {
        command.env(DATA_DIR_ENV, dir);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "child {role} failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn touch(path: &Path) -> PathBuf {
    fs::write(path, b"{}").unwrap();
    path.to_path_buf()
}

fn session_dirs(root: &Path) -> Vec<PathBuf> {
    match fs::read_dir(root.join("recovery")) {
        Ok(entries) => {
            let mut dirs: Vec<_> = entries.map(|e| e.unwrap().path()).collect();
            dirs.sort();
            dirs
        }
        Err(_) => Vec::new(),
    }
}

#[test]
fn locate_honours_the_data_dir_variable() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("custom-root");
    run_child("locate", &root, "", Some(&root));
    // Locating never creates anything.
    assert!(!root.exists());
}

#[test]
fn settings_default_when_missing_and_reading_creates_nothing() {
    let temp = tempfile::tempdir().unwrap();
    let data = AppData::at(temp.path().join("fresh"));
    assert_eq!(data.settings(), AppSettings::default());
    assert!(data.recent().is_empty());
    assert!(data.orphans().is_empty());
    assert!(!data.root().exists());
}

#[test]
fn settings_round_trip_and_stay_in_range() {
    let temp = tempfile::tempdir().unwrap();
    let data = AppData::at(temp.path());
    let dark = AppSettings {
        theme: ThemePreference::Dark,
        ui_scale: 1.5,
    };
    data.save_settings(&dark).unwrap();
    assert_eq!(data.settings(), dark);
    for (requested, stored) in [
        (5.0, 2.0),
        (0.1, 0.75),
        (f32::NAN, 1.0),
        (f32::INFINITY, 1.0),
    ] {
        data.save_settings(&AppSettings {
            ui_scale: requested,
            ..dark.clone()
        })
        .unwrap();
        assert_eq!(data.settings().ui_scale, stored, "{requested}");
    }
    let text = fs::read_to_string(temp.path().join("settings.json")).unwrap();
    assert!(text.ends_with("}\n"), "{text}");
}

#[test]
fn corrupt_or_foreign_settings_fall_back_to_defaults() {
    let temp = tempfile::tempdir().unwrap();
    let data = AppData::at(temp.path());
    let file = temp.path().join("settings.json");
    for text in [
        "",
        "garbage",
        "[1, 2]",
        "{\"theme\": \"Purple\"}",
        "\u{feff}{",
    ] {
        fs::write(&file, text).unwrap();
        assert_eq!(data.settings(), AppSettings::default(), "{text}");
    }
    // Unknown fields from other versions are ignored; values are clamped.
    fs::write(
        &file,
        r#"{"theme": "Dark", "ui_scale": 9, "accent": "red"}"#,
    )
    .unwrap();
    let expected = AppSettings {
        theme: ThemePreference::Dark,
        ui_scale: 2.0,
    };
    assert_eq!(data.settings(), expected);
}

#[test]
fn recent_list_is_newest_first_deduplicated_and_bounded() {
    let temp = tempfile::tempdir().unwrap();
    let data = AppData::at(temp.path().join("data"));
    let files: Vec<PathBuf> = (0..12)
        .map(|i| touch(&temp.path().join(format!("p{i}.xsim_proj"))))
        .collect();
    for f in &files {
        data.push_recent(f).unwrap();
    }
    let expected: Vec<PathBuf> = files.iter().rev().take(MAX_RECENT).cloned().collect();
    assert_eq!(data.recent(), expected);
    // Re-opening a listed project moves it to the front without duplicating.
    data.push_recent(&files[5]).unwrap();
    let recent = data.recent();
    assert_eq!(recent.len(), MAX_RECENT);
    assert_eq!(recent[0], files[5]);
    assert_eq!(recent.iter().filter(|p| **p == files[5]).count(), 1);
}

#[test]
fn recent_list_hides_missing_files_but_remembers_them() {
    let temp = tempfile::tempdir().unwrap();
    let data = AppData::at(temp.path().join("data"));
    let a = touch(&temp.path().join("a.xsim_proj"));
    let b = touch(&temp.path().join("b.xsim_proj"));
    data.push_recent(&a).unwrap();
    data.push_recent(&b).unwrap();
    fs::remove_file(&a).unwrap();
    assert_eq!(data.recent(), std::slice::from_ref(&b));
    // A file that comes back (a reconnected drive) reappears in place.
    touch(&a);
    assert_eq!(data.recent(), [b, a]);
}

#[test]
fn recent_paths_are_stored_absolute() {
    let temp = tempfile::tempdir().unwrap();
    let data = AppData::at(temp.path());
    let relative = Path::new("relative.xsim_proj");
    data.push_recent(relative).unwrap();
    let text = fs::read_to_string(temp.path().join("recent.json")).unwrap();
    let stored: Vec<PathBuf> = serde_json::from_str(&text).unwrap();
    assert_eq!(stored, [std::path::absolute(relative).unwrap()]);
}

#[test]
fn corrupt_or_foreign_recent_lists_are_tolerated_and_replaced() {
    let temp = tempfile::tempdir().unwrap();
    let data = AppData::at(temp.path().join("data"));
    fs::create_dir_all(data.root()).unwrap();
    let file = data.root().join("recent.json");
    let project = touch(&temp.path().join("p.xsim_proj"));
    for text in [
        "",
        "garbage",
        "{\"projects\": []}",
        "42",
        "[1, {\"path\": 2}]",
    ] {
        fs::write(&file, text).unwrap();
        assert!(data.recent().is_empty(), "{text}");
        data.push_recent(&project).unwrap();
        assert_eq!(data.recent(), std::slice::from_ref(&project), "{text}");
    }
}

#[test]
fn the_prototype_recent_format_is_read_as_is() {
    let temp = tempfile::tempdir().unwrap();
    let data = AppData::at(temp.path().join("data"));
    fs::create_dir_all(data.root()).unwrap();
    // The prototype kept up to 20 absolute paths as a pretty JSON array.
    let mut paths: Vec<PathBuf> = (0..15)
        .map(|i| temp.path().join(format!("old{i}.xsim_proj")))
        .collect();
    for p in paths.iter().skip(1) {
        touch(p);
    }
    paths.insert(3, paths[2].clone());
    fs::write(
        data.root().join("recent.json"),
        serde_json::to_string_pretty(&paths).unwrap(),
    )
    .unwrap();
    // old0 is missing, the duplicate of old2 is dropped, ten remain.
    let expected: Vec<PathBuf> = (1..=10)
        .map(|i| temp.path().join(format!("old{i}.xsim_proj")))
        .collect();
    assert_eq!(data.recent(), expected);
}

#[test]
fn concurrent_instances_never_lose_or_corrupt_shared_data() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("data");
    let settings = |i: usize| AppSettings {
        theme: if i.is_multiple_of(2) {
            ThemePreference::Dark
        } else {
            ThemePreference::Light
        },
        ui_scale: 1.0 + 0.125 * i as f32,
    };
    AppData::at(&root).save_settings(&settings(0)).unwrap();
    let files: Vec<PathBuf> = (0..6)
        .map(|i| touch(&temp.path().join(format!("w{i}.xsim_proj"))))
        .collect();
    std::thread::scope(|scope| {
        for (i, file) in files.iter().enumerate() {
            let root = root.clone();
            scope.spawn(move || {
                // Each thread is a separate instance with its own handle.
                let data = AppData::at(root);
                for _ in 0..10 {
                    data.push_recent(file).unwrap();
                    data.save_settings(&settings(i)).unwrap();
                    let read = data.settings();
                    assert!((0..6).any(|j| read == settings(j)), "{read:?}");
                    assert!(!data.recent().is_empty());
                }
            });
        }
    });
    let data = AppData::at(&root);
    let mut recent = data.recent();
    recent.sort();
    assert_eq!(recent, files);
    let leftovers: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn running_sessions_are_never_offered_and_clean_exits_leave_nothing() {
    let temp = tempfile::tempdir().unwrap();
    let first = AppData::at(temp.path());
    let second = AppData::at(temp.path());
    let a = first.start_recovery().unwrap();
    let b = second.start_recovery().unwrap();
    assert_ne!(a.dir(), b.dir());
    a.write(&snapshot("A")).unwrap();
    b.write(&snapshot("B")).unwrap();
    b.write(&snapshot("B2")).unwrap();
    assert!(first.orphans().is_empty());
    assert!(AppData::at(temp.path()).orphans().is_empty());
    assert_eq!(session_dirs(temp.path()).len(), 2);
    let a_dir = a.dir().to_path_buf();
    drop(a);
    assert!(!a_dir.exists());
    assert!(second.orphans().is_empty());
    drop(b);
    assert!(session_dirs(temp.path()).is_empty());
}

#[test]
fn a_crashed_instance_leaves_an_orphan_until_it_is_discarded() {
    let temp = tempfile::tempdir().unwrap();
    let data = AppData::at(temp.path());
    let running = data.start_recovery().unwrap();
    running.write(&snapshot("still running")).unwrap();
    run_child("crash-with-snapshot", temp.path(), "crashed", None);

    let orphans = data.orphans();
    assert_eq!(orphans.len(), 1);
    let orphan = &orphans[0];
    assert_eq!(orphan.snapshot, snapshot("crashed"));
    assert_ne!(orphan.dir, running.dir());
    // Listing does not consume the orphan.
    assert_eq!(AppData::at(temp.path()).orphans(), orphans);

    data.discard_orphan(orphan).unwrap();
    assert!(!orphan.dir.exists());
    assert!(data.orphans().is_empty());
    // Discarding twice (another instance got there first) is fine.
    data.discard_orphan(orphan).unwrap();
    assert!(running.dir().exists());
}

#[test]
fn orphans_are_listed_oldest_first() {
    let temp = tempfile::tempdir().unwrap();
    let data = AppData::at(temp.path());
    run_child("crash-with-snapshot", temp.path(), "first", None);
    run_child("crash-with-snapshot", temp.path(), "second", None);
    let base = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    for orphan in data.orphans() {
        let offset = if orphan.snapshot.project.name == "second" {
            0
        } else {
            60
        };
        // Make "second" the older snapshot whatever the creation order.
        let file = fs::File::options()
            .write(true)
            .open(orphan.dir.join("snapshot.json"))
            .unwrap();
        file.set_modified(base + Duration::from_secs(offset))
            .unwrap();
    }
    let names: Vec<String> = data
        .orphans()
        .into_iter()
        .map(|o| o.snapshot.project.name)
        .collect();
    assert_eq!(names, ["second", "first"]);
}

#[test]
fn crashed_sessions_without_a_snapshot_are_cleaned_up() {
    let temp = tempfile::tempdir().unwrap();
    let data = AppData::at(temp.path());
    run_child("crash-without-snapshot", temp.path(), "", None);
    assert_eq!(session_dirs(temp.path()).len(), 1);
    assert!(data.orphans().is_empty());
    assert!(session_dirs(temp.path()).is_empty());
}

#[test]
fn unreadable_snapshots_are_left_for_a_version_that_can_read_them() {
    let temp = tempfile::tempdir().unwrap();
    let data = AppData::at(temp.path());
    run_child("crash-with-snapshot", temp.path(), "x", None);
    let dir = session_dirs(temp.path()).remove(0);
    fs::write(
        dir.join("snapshot.json"),
        "{\"format\": \"from the future\"}",
    )
    .unwrap();
    assert!(data.orphans().is_empty());
    assert!(dir.join("snapshot.json").exists());
}

#[test]
fn discarding_refuses_running_sessions_and_foreign_directories() {
    let temp = tempfile::tempdir().unwrap();
    let data = AppData::at(temp.path().join("data"));
    let running = data.start_recovery().unwrap();
    let pretend = |dir: &Path| Orphan {
        dir: dir.to_path_buf(),
        snapshot: snapshot("x"),
    };
    assert!(data.discard_orphan(&pretend(running.dir())).is_err());
    assert!(running.dir().exists());

    let elsewhere = temp.path().join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    let error = data.discard_orphan(&pretend(&elsewhere)).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(elsewhere.exists());

    // `recovery/..` has the recovery area as its parent but is the data
    // directory itself.
    let recovery = data.root().join("recovery");
    let error = data
        .discard_orphan(&pretend(&recovery.join("..")))
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(running.dir().exists());

    // A directory without a session lock was not created by a session.
    let foreign = recovery.join("session-foreign");
    fs::create_dir(&foreign).unwrap();
    fs::write(foreign.join("snapshot.json"), b"{}").unwrap();
    let error = data.discard_orphan(&pretend(&foreign)).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(foreign.join("snapshot.json").exists());
}

#[test]
fn clear_removes_the_snapshot_but_keeps_the_session() {
    let temp = tempfile::tempdir().unwrap();
    let data = AppData::at(temp.path());
    let session = data.start_recovery().unwrap();
    session.clear().unwrap(); // nothing to clear yet
    session.write(&snapshot("work")).unwrap();
    let file = session.dir().join("snapshot.json");
    assert!(file.exists());
    let stored: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(stored["project"]["name"], "work");
    session.clear().unwrap();
    assert!(!file.exists());
    assert!(session.dir().join("lock").exists());
}

#[test]
fn snapshots_must_be_restorable() {
    let temp = tempfile::tempdir().unwrap();
    let session = AppData::at(temp.path()).start_recovery().unwrap();
    let mut bad = snapshot("bad");
    bad.project.circuits.clear();
    let error = session.write(&bad).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert!(!session.dir().join("snapshot.json").exists());
}

#[test]
fn half_created_sessions_are_removed_but_foreign_snapshots_kept() {
    let temp = tempfile::tempdir().unwrap();
    let data = AppData::at(temp.path());
    let recovery = temp.path().join("recovery");
    // A creator that crashed between making its directory and its lock.
    let half_created = recovery.join("session-crashed");
    fs::create_dir_all(&half_created).unwrap();
    // A snapshot without a lock file was not written by this protocol.
    let foreign = recovery.join("session-foreign");
    fs::create_dir_all(&foreign).unwrap();
    let text = serde_json::to_string(&snapshot("foreign")).unwrap();
    fs::write(foreign.join("snapshot.json"), text).unwrap();
    // Stray files in the recovery area are ignored.
    fs::write(recovery.join("notes.txt"), b"x").unwrap();

    assert!(data.orphans().is_empty());
    assert!(!half_created.exists());
    assert!(foreign.join("snapshot.json").exists());
    assert!(recovery.join("notes.txt").exists());
}
