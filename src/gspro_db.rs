//! GSPro's own result of each shot – carry, total, apex, offline, descent and the distance left
//! to the pin. GSPro doesn't send these back over OpenConnect, but it keeps the latest shot in its
//! SQLite database in the CrossOver bottle (table `LastShot`, one JSON row whose ID grows with
//! every shot). A watcher reads it (read-only) and adds the result to the matching shot.

use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{log, recorder, state};

/// How often the database is looked at.
const POLL: Duration = Duration::from_secs(1);
/// Metres per yard (GSPro writes yards when it shows imperial units).
const YARD: f64 = 0.9144;
const MPH_TO_KMH: f64 = 1.609_344;

/// GSPro's result of a shot, in metres and degrees.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct GsproResult {
    pub carry: f64,
    pub total: f64,
    /// Lateral offset where the ball stopped, right positive.
    pub side: f64,
    pub apex: f64,
    pub descent: f64,
    /// Distance left to the pin after the shot (on a course).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_pin: Option<f64>,
}

/// The `ShotData` JSON GSPro writes (only what is used). Lengths are in GSPro's display units.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ShotData {
    carry: f64,
    total_distance: f64,
    ball_speed: f64,
    #[serde(rename = "VLA")]
    vla: f64,
    /// Sic: GSPro's spelling.
    #[serde(default)]
    decent: f64,
    #[serde(default)]
    peak_height: f64,
    #[serde(default)]
    offline: f64,
    #[serde(default)]
    distance_to_pin: Option<f64>,
}

/// GSPro's databases in the CrossOver bottles, the most recently changed first.
#[must_use]
pub fn find() -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("HOME") else { return Vec::new() };
    let bottles = PathBuf::from(home).join("Library/Application Support/CrossOver/Bottles");
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(&bottles)
        .into_iter()
        .flatten()
        .flatten()
        .flat_map(|bottle| {
            std::fs::read_dir(bottle.path().join("drive_c/users")).into_iter().flatten().flatten().map(|user| {
                user.path().join("AppData/LocalLow/GSPro/GSPro/GSPro.db")
            })
        })
        .filter_map(|p| Some((p.metadata().ok()?.modified().ok()?, p)))
        .collect();
    found.sort_by_key(|f| std::cmp::Reverse(f.0));
    found.into_iter().map(|(_, p)| p).collect()
}

/// The latest row of `LastShot`: its ID and the shot data.
fn read_last(db: &Path) -> Result<Option<(i64, String)>, String> {
    let conn = rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .map_err(|e| e.to_string())?;
    conn.busy_timeout(Duration::from_millis(200)).map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare("SELECT ID, ShotData FROM LastShot ORDER BY ID DESC LIMIT 1").map_err(|e| e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    match rows.next().map_err(|e| e.to_string())? {
        Some(row) => Ok(Some((row.get(0).map_err(|e| e.to_string())?, row.get(1).map_err(|e| e.to_string())?))),
        None => Ok(None),
    }
}

/// GSPro's result for one of our shots, if `json` belongs to it: the ball speed must match the one
/// sent (in km/h when GSPro shows metric units, in mph otherwise) and so must the launch angle.
fn result_for(json: &str, ball_mph: f64, vla: f64) -> Option<GsproResult> {
    let d: ShotData = serde_json::from_str(json).ok()?;
    let close = |a: f64, b: f64| (a - b).abs() <= b.abs() * 0.02 + 0.3;
    let metric = close(d.ball_speed, ball_mph * MPH_TO_KMH);
    if !(metric || close(d.ball_speed, ball_mph)) || (d.vla - vla).abs() > 0.6 {
        return None;
    }
    let m = |v: f64| if metric { v } else { v * YARD };
    Some(GsproResult {
        carry: m(d.carry),
        total: m(d.total_distance),
        side: m(d.offline),
        apex: m(d.peak_height),
        descent: d.decent,
        to_pin: d.distance_to_pin.filter(|v| *v > 0.0).map(m),
    })
}

/// Watch GSPro's database for new results and add them to the matching shots of the session
/// (and the shot history). Runs for the lifetime of the app; does nothing without GSPro.
pub fn watch() {
    thread::Builder::new()
        .name("gspro-db".into())
        .spawn(|| {
            let mut seen: Option<i64> = None;
            let mut reported = false;
            loop {
                thread::sleep(POLL);
                let Some(db) = find().into_iter().next() else { continue };
                let (id, json) = match read_last(&db) {
                    Ok(Some(row)) => row,
                    Ok(None) => continue,
                    Err(e) => {
                        // GSPro may be writing; only the first failure is worth a line.
                        if !reported {
                            log::gspro(&format!("Could not read GSPro's results ({e})"));
                            reported = true;
                        }
                        continue;
                    }
                };
                // The row there on start belongs to an earlier shot.
                if seen.replace(id).is_none_or(|old| old == id) {
                    continue;
                }
                attach(&json);
            }
        })
        .expect("spawn gspro-db thread");
}

/// Add GSPro's result to the newest recent shot it belongs to (test shots aren't in the
/// session: their result is only logged). Storing it is the recorder's job.
fn attach(json: &str) {
    let found = state::with(|s| {
        let n = s.session.len();
        s.session.iter().skip(n.saturating_sub(5)).rev().filter(|r| r.gspro.is_none()).find_map(|r| {
            let result = result_for(json, r.ball.speed, r.ball.vla)?;
            Some(((r.time.clone(), r.shot_number), result))
        })
    });
    let describe = |r: &GsproResult| {
        format!(
            "carry {:.1} m, total {:.1} m, apex {:.1} m, offline {:.1} m{}",
            r.carry,
            r.total,
            r.apex,
            r.side,
            r.to_pin.map_or(String::new(), |p| format!(", {p:.0} m to the pin")),
        )
    };
    match found {
        Some((shot, result)) => {
            log::gspro(&format!("GSPro result: {}", describe(&result)));
            recorder::gspro_result(Some(shot), result);
        }
        // A test shot: matched with the last shot sent, not kept.
        None => {
            let last = state::with(|s| s.last_shot.as_ref().map(|l| (l.ball.speed, l.ball.vla)));
            if let Some(result) = last.and_then(|(mph, vla)| result_for(json, mph, vla)) {
                log::gspro(&format!("GSPro result of a shot not in the session (e.g. a test shot): {}", describe(&result)));
                recorder::gspro_result(None, result);
            }
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::history;

    /// The row GSPro wrote for a shot of 118.7 mph at 16.8° (metric display).
    const SAMPLE: &str = crate::fixture!("gspro-lastshot.json");

    #[test]
    fn metric_row_matches_its_shot() {
        let r = result_for(SAMPLE, 191.029_129 / MPH_TO_KMH, 16.8).expect("matches");
        assert!((r.carry - 150.04).abs() < 0.01 && (r.total - 152.77).abs() < 0.01);
        assert!((r.apex - 28.69).abs() < 0.01 && (r.side - 5.01).abs() < 0.01 && (r.descent - 47.4).abs() < 0.01);
        assert_eq!(r.to_pin.map(f64::round), Some(239.0));
    }

    #[test]
    fn other_shots_dont_match() {
        assert_eq!(result_for(SAMPLE, 100.0, 16.8), None, "other ball speed");
        assert_eq!(result_for(SAMPLE, 118.7, 20.0), None, "other launch angle");
        assert_eq!(result_for("not json", 118.7, 16.8), None);
    }

    #[test]
    fn imperial_rows_are_converted_to_metres() {
        let yards = SAMPLE.replace("191.029129", "118.7");
        let r = result_for(&yards, 118.7, 16.8).expect("matches in mph");
        assert!((r.carry - 150.04 * YARD).abs() < 0.01);
    }

    #[test]
    fn reads_the_latest_row() {
        let path = std::env::temp_dir().join(format!("gspro-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE LastShot(ID integer primary key autoincrement not null, DateCreated datetime, ShotData varchar);").unwrap();
        conn.execute("INSERT INTO LastShot(DateCreated, ShotData) VALUES ('2026-10-19 20:40:04', ?1)", [SAMPLE]).unwrap();
        drop(conn);
        let (id, json) = read_last(&path).unwrap().unwrap();
        assert_eq!((id, json.as_str()), (1, SAMPLE));
        std::fs::remove_file(&path).unwrap();
    }

    /// A session shot; `speed` and `vla` of 118.7 mph and 16.8° match `SAMPLE`.
    fn shot(n: u32, speed: f64, vla: f64) -> history::ShotRecord {
        let (ball, club) = crate::engine::test_shot();
        let ball = crate::mapping::GsBall { speed, vla, ..ball };
        history::ShotRecord::new(ball, Some(club), Some("DR".into()), true, n)
    }

    const MPH: f64 = 191.029_129 / MPH_TO_KMH;

    /// GSPro's result goes to the newest of the last five session shots it matches that has none
    /// yet, into the shot history and the state; the others stay as they were.
    #[test]
    fn result_is_attached_to_the_newest_matching_shot() {
        let _state = crate::test_util::state_lock();
        let _history = crate::test_util::TestHistory::new("gspro-db-attach");
        let session = vec![shot(1, MPH, 16.8), shot(2, MPH, 16.8), shot(3, 90.0, 22.0), shot(4, MPH, 16.8), shot(5, 80.0, 25.0)];
        for r in &session {
            history::append(r).unwrap();
        }
        state::with(|s| {
            s.session.clone_from(&session);
            s.gspro_result = None;
        });
        attach(SAMPLE);
        let (now, result) = state::with(|s| (s.session.clone(), s.gspro_result));
        let want = result_for(SAMPLE, MPH, 16.8).unwrap();
        assert_eq!(result, Some(want));
        let with_result: Vec<u32> = now.iter().filter(|r| r.gspro.is_some()).map(|r| r.shot_number).collect();
        assert_eq!(with_result, [4], "the newest matching shot");
        assert_eq!(now[3].gspro, Some(want));
        let saved = history::load_all();
        assert_eq!(saved[3].gspro, Some(want), "kept in the history");
        assert!(saved.iter().enumerate().all(|(i, r)| i == 3 || r.gspro.is_none()));

        // The same row again (GSPro wrote it twice): the next older match without a result gets it.
        attach(SAMPLE);
        let with_result: Vec<u32> = state::with(|s| s.session.iter().filter(|r| r.gspro.is_some()).map(|r| r.shot_number).collect());
        assert_eq!(with_result, [2, 4]);
    }

    /// Only the last five shots are looked at; a result that matches none of them belongs to the
    /// last shot sent (a test shot): shown, not kept.
    #[test]
    fn result_of_a_shot_not_in_the_session_is_only_shown() {
        let _state = crate::test_util::state_lock();
        let _history = crate::test_util::TestHistory::new("gspro-db-test-shot");
        let mut session = vec![shot(1, MPH, 16.8)];
        session.extend((2..=6).map(|n| shot(n, 90.0, 22.0)));
        let (ball, club) = crate::engine::test_shot();
        let ball = crate::mapping::GsBall { speed: MPH, vla: 16.8, ..ball };
        state::with(|s| {
            s.session.clone_from(&session);
            s.gspro_result = None;
            s.last_shot = Some(state::LastShot {
                ball,
                club: Some(club),
                tempo: None,
                at: std::time::Instant::now(),
                delivered: true,
                shot_number: 7,
                misread: None,
            });
        });
        attach(SAMPLE);
        let (now, result) = state::with(|s| (s.session.clone(), s.gspro_result));
        assert_eq!(now, session, "the old matching shot is out of reach");
        assert_eq!(result, result_for(SAMPLE, MPH, 16.8));
        assert!(history::load_all().is_empty());

        // Matches nothing at all: nothing changes.
        state::with(|s| s.gspro_result = None);
        attach(&SAMPLE.replace("191.029129", "50.0"));
        assert_eq!(state::with(|s| s.gspro_result), None);
    }
}
