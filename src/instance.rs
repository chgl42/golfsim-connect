//! One instance per data folder (ADR-012): the app and the CLI claim the data folder at start with
//! an exclusive lock on `instance.lock` in it, held until the process ends, and only then move an
//! earlier data folder in. A second instance doesn't get the lock and stops without writing, and so
//! does an instance that finds a build of an earlier name still holding the lock of its folder.

use std::fmt;
use std::fs::{File, OpenOptions, TryLockError};
use std::path::Path;
use std::sync::OnceLock;

use crate::settings::{DATA_DIR, OLD_DATA_DIRS, Settings};

/// The lock file in the data folder. The OS releases the lock when the process ends, so it is
/// never stale; the file itself stays.
pub const LOCK_FILE: &str = "instance.lock";

/// Another process holds the data folder, or the earlier data folder that would move into it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlreadyRunning {
    /// The app of an earlier name (named as its data folder, e.g. `"OpenLaunch Connect"`) that is
    /// still running with its own folder; `None` if it is another instance of this app.
    pub earlier: Option<&'static str>,
}

impl fmt::Display for AlreadyRunning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.earlier {
            None => f.write_str("already running: another instance uses the data folder"),
            Some(name) => write!(f, "already running: {name} still uses the earlier data folder"),
        }
    }
}

impl std::error::Error for AlreadyRunning {}

/// The lock of this process, held until it ends; `None` inside if there could be none.
static CLAIM: OnceLock<Option<File>> = OnceLock::new();

impl Settings {
    /// Claims the data folder for this process: locks it, then moves an earlier data folder in
    /// (see `migrate_data_dir_in`). Call it at start, before the settings are read and before
    /// anything writes to the data folder. A call after a claim or after "no lock possible" does
    /// nothing; after [`AlreadyRunning`] it tries again.
    ///
    /// Where there can be no lock (no `HOME`, the folder can't be created), it logs that and goes
    /// on without one, as before the lock.
    ///
    /// # Errors
    /// [`AlreadyRunning`] if another process holds the data folder, or a build of an earlier name
    /// still holds the earlier folder (then it names that app); then nothing was written.
    pub fn claim_data_dir() -> Result<(), AlreadyRunning> {
        if CLAIM.get().is_some() {
            return Ok(());
        }
        let Some(home) = std::env::var_os("HOME") else {
            crate::log::info("Data folder: no HOME, running without the instance lock");
            let _ = CLAIM.set(None);
            return Ok(());
        };
        let lock = claim_data_dir_in(Path::new(&home))?;
        let _ = CLAIM.set(lock);
        Ok(())
    }
}

/// [`Settings::claim_data_dir`] under the home folder `home`: makes sure no build of an earlier name
/// still holds its folder, locks the data folder, then moves an earlier one in (also without a lock
/// where there can be none, as before the lock). The lock file, to be kept open for the whole run.
///
/// The locks of the earlier folders are held until the move is done, so such a build started
/// meanwhile is turned away too. A build from before the lock can't be seen.
pub(crate) fn claim_data_dir_in(home: &Path) -> Result<Option<File>, AlreadyRunning> {
    let earlier = earlier_locks_in(home)?;
    let lock = claim_in(home)?;
    Settings::migrate_data_dir_in(home);
    drop(earlier);
    Ok(lock)
}

/// Tries the lock of every earlier data folder under `home` that has a lock file, without creating
/// or changing anything. The locks taken, to be held while the move runs; [`AlreadyRunning`] naming
/// the earlier app if one of them is held.
fn earlier_locks_in(home: &Path) -> Result<Vec<File>, AlreadyRunning> {
    let mut locks = Vec::new();
    for name in OLD_DATA_DIRS {
        let Ok(file) = File::open(Settings::dir_in(home, name).join(LOCK_FILE)) else { continue };
        match file.try_lock() {
            Ok(()) => locks.push(file),
            Err(TryLockError::WouldBlock) => {
                crate::log::info(&format!("Data folder: \"{name}\" is in use, {name} is still running; nothing moves"));
                return Err(AlreadyRunning { earlier: Some(name) });
            }
            Err(TryLockError::Error(_)) => {}
        }
    }
    Ok(locks)
}

/// Locks the data folder under `home`, creating it and its lock file if needed. `Ok(None)` (logged)
/// if there can be no lock; then the caller goes on without one.
///
/// A second instance finds folder and lock file there, so it creates and changes nothing: it only
/// opens the lock file to try the lock. The lock belongs to the open file, so a second call in the
/// same process fails too while the first file is open.
pub(crate) fn claim_in(home: &Path) -> Result<Option<File>, AlreadyRunning> {
    let dir = Settings::dir_in(home, DATA_DIR);
    let file = std::fs::create_dir_all(&dir).and_then(|()| {
        OpenOptions::new().read(true).write(true).create(true).truncate(false).open(dir.join(LOCK_FILE))
    });
    let file = match file {
        Ok(file) => file,
        Err(e) => {
            crate::log::info(&format!("Data folder: no instance lock in \"{DATA_DIR}\" ({e}), running without it"));
            return Ok(None);
        }
    };
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Err(AlreadyRunning { earlier: None }),
        Err(TryLockError::Error(e)) => {
            crate::log::info(&format!("Data folder: instance lock not supported ({e}), running without it"));
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::*;
    use crate::test_util::TempDir;

    /// Every file under `dir` with its bytes and every folder, to compare a folder before and after.
    fn snapshot(dir: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
        let mut all = BTreeMap::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                all.insert(path.clone(), None);
                all.extend(snapshot(&path));
            } else {
                all.insert(path.clone(), Some(std::fs::read(&path).unwrap()));
            }
        }
        all
    }

    #[test]
    fn first_instance_takes_the_lock() {
        let home = TempDir::new("instance-first");
        assert!(claim_in(home.path()).unwrap().is_some());
        assert!(Settings::dir_in(home.path(), DATA_DIR).join(LOCK_FILE).is_file());
    }

    /// A second instance is turned away and leaves the data folder byte for byte as it was; once
    /// the first one ends, the lock is free again.
    #[test]
    fn second_instance_is_turned_away_and_writes_nothing() {
        let home = TempDir::new("instance-second");
        let dir = Settings::dir_in(home.path(), DATA_DIR);
        let first = claim_in(home.path()).unwrap();
        assert!(first.is_some());
        std::fs::write(dir.join("settings.json"), "{}").unwrap();
        std::fs::write(dir.join("shots.jsonl"), "{\"shot\":1}\n").unwrap();
        let before = snapshot(home.path());
        assert_eq!(claim_in(home.path()).unwrap_err(), AlreadyRunning { earlier: None });
        assert_eq!(snapshot(home.path()), before);
        drop(first);
        assert!(claim_in(home.path()).unwrap().is_some(), "the lock ends with its holder");
    }

    /// No folder can be made (here the home is a file): no lock, and the caller runs without one.
    #[test]
    fn without_a_folder_there_is_no_lock() {
        let home = TempDir::new("instance-impossible");
        let file_home = home.path().join("not-a-folder");
        std::fs::write(&file_home, "").unwrap();
        assert!(claim_in(&file_home).unwrap().is_none());
    }

    /// The claim locks, then moves the history in; a second claim is turned away and moves nothing.
    #[test]
    fn the_history_moves_only_under_the_lock() {
        let home = TempDir::new("instance-migrate");
        let old = Settings::dir_in(home.path(), "OpenR10 Connect");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("shots.jsonl"), "{\"shot\":1}\n{\"shot\":2}\n").unwrap();
        // Held by another instance: the claim is turned away before it moves anything.
        let other = claim_in(home.path()).unwrap();
        let before = snapshot(home.path());
        assert_eq!(claim_data_dir_in(home.path()).unwrap_err(), AlreadyRunning { earlier: None });
        assert_eq!(snapshot(home.path()), before, "no move without the lock");
        drop(other);
        let first = claim_data_dir_in(home.path()).unwrap();
        assert!(first.is_some(), "the first claim holds the lock");
        let new = Settings::dir_in(home.path(), DATA_DIR);
        assert_eq!(std::fs::read_to_string(new.join("shots.jsonl")).unwrap().lines().count(), 2);
        assert!(new.join(LOCK_FILE).is_file());
        assert!(!old.exists());
        let before = snapshot(home.path());
        assert_eq!(claim_data_dir_in(home.path()).unwrap_err(), AlreadyRunning { earlier: None });
        assert_eq!(snapshot(home.path()), before, "a second claim changes nothing");
    }

    /// A build of the earlier name still running (it holds the lock of its folder): nothing moves,
    /// nothing is written, and the error names that app. Once it ends, its folder moves in.
    #[test]
    fn a_running_earlier_build_stops_the_move() {
        let home = TempDir::new("instance-earlier-running");
        let old = Settings::dir_in(home.path(), "OpenLaunch Connect");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("shots.jsonl"), "{\"shot\":1}\n").unwrap();
        let held = File::create(old.join(LOCK_FILE)).unwrap();
        held.try_lock().unwrap();
        let before = snapshot(home.path());
        let err = claim_data_dir_in(home.path()).unwrap_err();
        assert_eq!(err, AlreadyRunning { earlier: Some("OpenLaunch Connect") });
        assert!(err.to_string().contains("already running"), "{err}");
        assert_eq!(snapshot(home.path()), before, "no write while the earlier build runs");
        assert!(!Settings::dir_in(home.path(), DATA_DIR).exists());
        drop(held);
        let lock = claim_data_dir_in(home.path()).unwrap();
        assert!(lock.is_some());
        let new = Settings::dir_in(home.path(), DATA_DIR);
        assert_eq!(std::fs::read_to_string(new.join("shots.jsonl")).unwrap(), "{\"shot\":1}\n");
        assert!(!old.exists(), "the earlier lock file stays behind only while it is held");
    }

    /// A fresh install: the data folder is made with its lock, and no earlier folder appears.
    #[test]
    fn a_fresh_install_makes_only_the_data_folder() {
        let home = TempDir::new("instance-fresh");
        assert!(claim_data_dir_in(home.path()).unwrap().is_some());
        assert!(Settings::dir_in(home.path(), DATA_DIR).join(LOCK_FILE).is_file());
        for name in OLD_DATA_DIRS {
            assert!(!Settings::dir_in(home.path(), name).exists());
        }
    }
}
