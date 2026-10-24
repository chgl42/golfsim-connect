//! Where GSPro is – its menus, the practice range or a course – from the log Unity writes next to
//! its database (`Player.log` in the CrossOver bottle). GSPro doesn't tell OpenConnect clients,
//! but it logs loading the range, loading a course and going back to the menu. A watcher follows
//! the log, keeps the latest of these in `state.gspro_mode` and publishes `Event::GsproMode`.
//! Only these markers are looked at: the log also holds the player's name, which never goes into
//! the app's log.

use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::events::{self, Event};
use crate::{log, state};

/// How often the log is looked at.
const POLL: Duration = Duration::from_secs(1);

/// Where GSPro is.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum GameMode {
    /// The main menu and its settings (also while a range or course is loading).
    Menu,
    /// The practice range.
    Range,
    /// A course: a round or practice on the course.
    Course,
}

impl GameMode {
    /// The English source text for the UI (translated there) and the log.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Menu => "Menu",
            Self::Range => "Range",
            Self::Course => "Course",
        }
    }
}

/// The mode a line of GSPro's log starts, if it starts one.
fn mode_of(line: &str) -> Option<GameMode> {
    let line = line.trim_end();
    if line == "RangeLevelLoaded()" || line == "Loading course Practice" {
        return Some(GameMode::Range);
    }
    if line.starts_with("Loading course ") || line.starts_with("Loading standard cour") {
        return Some(GameMode::Course);
    }
    // GSPro starting, a round ended, back from the range.
    if line.starts_with("Starting Main Init") || line == "End Round Button Click" || line == "Round End CleanUp" || line == "Reset Scene Complete" {
        return Some(GameMode::Menu);
    }
    None
}

/// GSPro's log next to its database, in the most recently used bottle.
fn find() -> Option<PathBuf> {
    crate::gspro_db::find().into_iter().map(|db| db.with_file_name("Player.log")).find(|p| p.exists())
}

/// Follows a log file: reads what was added since the last look, starting over when the file is
/// new (GSPro moves the old one to `Player-prev.log` when it starts).
struct Follower {
    path: PathBuf,
    offset: u64,
    /// The file read (GSPro's restart replaces it; the new one may already be longer).
    file_id: Option<u64>,
    mode: Option<GameMode>,
}

impl Follower {
    fn new(path: PathBuf) -> Self {
        Self { path, offset: 0, file_id: None, mode: None }
    }

    /// The mode after the lines added since the last call.
    fn poll(&mut self) -> std::io::Result<Option<GameMode>> {
        let mut file = File::open(&self.path)?;
        let meta = file.metadata()?;
        let id = file_id(&meta);
        if meta.len() < self.offset || id != self.file_id {
            self.file_id = id;
            self.offset = 0;
            self.mode = None;
        }
        file.seek(SeekFrom::Start(self.offset))?;
        let mut reader = BufReader::new(file);
        let mut line = Vec::new();
        loop {
            line.clear();
            let n = reader.read_until(b'\n', &mut line)?;
            // A line still being written is read again next time.
            if n == 0 || line.last() != Some(&b'\n') {
                break;
            }
            self.offset += n as u64;
            // The log isn't always UTF-8 (paths, names); markers are ASCII.
            if let Some(mode) = mode_of(&String::from_utf8_lossy(&line)) {
                self.mode = Some(mode);
            }
        }
        Ok(self.mode)
    }
}

#[cfg(unix)]
#[expect(clippy::unnecessary_wraps, reason = "other platforms have no file id; callers treat both alike")]
fn file_id(meta: &std::fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(meta.ino())
}

#[cfg(not(unix))]
fn file_id(_meta: &std::fs::Metadata) -> Option<u64> {
    None
}

/// Watch GSPro's log and keep where GSPro is in the state. Runs for the lifetime of the app; the
/// mode is `None` while there is no log (GSPro not installed) or nothing in it says yet.
pub fn watch() {
    thread::Builder::new()
        .name("gspro-log".into())
        .spawn(|| {
            let mut follower: Option<Follower> = None;
            let mut shown = None;
            loop {
                if follower.as_ref().is_none_or(|f| !f.path.exists()) {
                    follower = find().map(Follower::new);
                }
                let mode = follower.as_mut().and_then(|f| f.poll().ok().flatten());
                if mode != shown {
                    if let Some(m) = mode {
                        log::gspro(&format!("GSPro is in: {}", m.name()));
                    }
                    shown = mode;
                    state::with(|s| s.gspro_mode = mode);
                    events::publish(Event::GsproMode { mode });
                }
                thread::sleep(POLL);
            }
        })
        .expect("spawn gspro-log thread");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn markers_give_the_mode() {
        assert_eq!(mode_of("RangeLevelLoaded()\r\n"), Some(GameMode::Range));
        assert_eq!(mode_of("Loading course Practice"), Some(GameMode::Range));
        assert_eq!(mode_of("Loading course Pebble Creek"), Some(GameMode::Course));
        assert_eq!(mode_of("Loading standard coure proc"), Some(GameMode::Course));
        assert_eq!(mode_of("Reset Scene Complete"), Some(GameMode::Menu));
        assert_eq!(mode_of("Starting Main Init"), Some(GameMode::Menu));
        assert_eq!(mode_of("LaunchExt - Regular Mode"), None);
        assert_eq!(mode_of("Loading Light Settings v1"), None);
    }

    #[test]
    fn follows_the_log_and_starts_over_with_a_new_one() {
        let dir = crate::test_util::TempDir::new("gspro-log");
        let path = dir.path().join("Player.log");
        std::fs::write(&path, "Starting Main Init\nGSPro Multi Range\n").unwrap();
        let mut f = Follower::new(path.clone());
        assert_eq!(f.poll().unwrap(), Some(GameMode::Menu));
        let mut file = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        // Half a line: not read yet.
        write!(file, "RangeLevelLoaded()").unwrap();
        assert_eq!(f.poll().unwrap(), Some(GameMode::Menu));
        writeln!(file).unwrap();
        writeln!(file, "Loading course Practice").unwrap();
        assert_eq!(f.poll().unwrap(), Some(GameMode::Range));
        writeln!(file, "End Round Button Click").unwrap();
        assert_eq!(f.poll().unwrap(), Some(GameMode::Menu));
        // GSPro restarted: the log moved away, a new one in its place.
        std::fs::rename(&path, path.with_extension("prev")).unwrap();
        std::fs::write(&path, "Some longer first line of the new log, longer than the old one was\n").unwrap();
        assert_eq!(f.poll().unwrap(), None);
    }
}
