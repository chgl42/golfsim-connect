//! Helpers for the unit tests: temp folders and a lock around the shared state.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard};

/// A fresh, empty folder under the system's temp dir, removed again when dropped.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(name: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("openlaunch-test-{}-{n}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A shot history in a temp folder for this test thread (`history::TEST_FILE`), gone when dropped.
pub struct TestHistory {
    pub file: PathBuf,
    _dir: TempDir,
}

impl TestHistory {
    pub fn new(name: &str) -> Self {
        let dir = TempDir::new(name);
        let file = dir.path().join("shots.jsonl");
        crate::history::TEST_FILE.set(Some(file.clone()));
        Self { file, _dir: dir }
    }

    pub fn text(&self) -> String {
        std::fs::read_to_string(&self.file).unwrap_or_default()
    }
}

impl Drop for TestHistory {
    fn drop(&mut self) {
        crate::history::TEST_FILE.set(None);
    }
}

/// Held by the tests that record shots or look at `state::STATE`'s session and last shot, so they
/// don't see each other's shots.
pub fn state_lock() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A fixture from `tests/fixtures/`.
#[macro_export]
macro_rules! fixture {
    ($name:literal) => {
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/", $name)).trim_ascii_end()
    };
}
