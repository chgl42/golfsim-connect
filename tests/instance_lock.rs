//! One instance per data folder (ADR-012), end to end: the real `openlaunch` binary, its own
//! process-wide claim, against a data folder whose lock another process holds.

use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use openlaunch_connect::instance::LOCK_FILE;

/// A fresh home folder for the binary, removed at the end.
struct TempHome(PathBuf);

impl TempHome {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("openlaunch-it-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The data folder under `home`, where the app puts it (`Settings::dir_in`).
fn data_dir(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/OpenLaunch Connect")
    } else {
        home.join(".config/openlaunch connect")
    }
}

/// The names in `dir`, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> =
        std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

/// While another process holds the data folder, the CLI says "already running", exits with 1 and
/// writes nothing.
#[test]
fn second_cli_is_turned_away() {
    let home = TempHome::new("second-cli");
    let dir = data_dir(&home.0);
    std::fs::create_dir_all(&dir).unwrap();
    let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(dir.join(LOCK_FILE)).unwrap();
    lock.try_lock().unwrap();
    let before = names(&dir);

    let mut child = Command::new(env!("CARGO_BIN_EXE_openlaunch"))
        .env("HOME", &home.0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // A binary that got past the claim would go on to look for an R10: stop it, then fail.
    let start = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if start.elapsed() > Duration::from_secs(20) {
            child.kill().unwrap();
            panic!("the second instance kept running");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let out = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_eq!(out.status.code(), Some(1), "stderr: {stderr}");
    assert!(stderr.contains("already running"), "stderr: {stderr}");
    assert_eq!(names(&dir), before, "the second instance writes nothing");
    drop(lock);
}
