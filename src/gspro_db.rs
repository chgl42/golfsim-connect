//! GSPro's own result of each shot – carry, total, apex, offline, descent and the distance left
//! to the pin. GSPro doesn't send these back over OpenConnect, but it keeps the latest shot in its
//! SQLite database in the CrossOver bottle (table `LastShot`, one JSON row whose ID grows with
//! every shot). A watcher reads it (read-only) and adds the result to the matching shot.

use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{history, log, state};

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
/// session: their result is only logged).
fn attach(json: &str) {
    let found = state::with(|s| {
        let n = s.session.len();
        s.session.iter_mut().skip(n.saturating_sub(5)).rev().filter(|r| r.gspro.is_none()).find_map(|r| {
            let result = result_for(json, r.ball.speed, r.ball.vla)?;
            r.gspro = Some(result);
            Some((r.time.clone(), r.shot_number, result))
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
        Some((time, number, result)) => {
            state::with(|s| s.gspro_result = Some(result));
            log::gspro(&format!("GSPro result: {}", describe(&result)));
            if let Err(e) = history::set_gspro(&time, number, result) {
                log::gspro(&format!("Could not save GSPro's result: {e}"));
            }
        }
        // A test shot: matched with the last shot sent, not kept.
        None => {
            let last = state::with(|s| s.last_shot.as_ref().map(|l| (l.ball.speed, l.ball.vla)));
            if let Some(result) = last.and_then(|(mph, vla)| result_for(json, mph, vla)) {
                state::with(|s| s.gspro_result = Some(result));
                log::gspro(&format!("GSPro result of a shot not in the session (e.g. a test shot): {}", describe(&result)));
            }
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    /// The row GSPro wrote for a shot of 118.7 mph at 16.8° (metric display).
    const SAMPLE: &str = r#"{"Carry":150.037933,"TotalDistance":152.765411,"BallSpeed":191.029129,"BackSpin":7106.0,"SideSpin":211.0,"HLA":1.0,"VLA":16.8,"Decent":47.39972,"PeakHeight":28.6936054,"Offline":5.00848,"rawSpinAxis":1.7,"rawCarryGame":150.027908,"rawCarryLM":0.0,"DistanceToPin":239.375977,"club":"DR","ClubSpeed":135.50676,"Path":-1.2,"AoA":-3.9,"FaceToTarget":0.7,"FaceToPath":-1.9000001,"Lie":0.0,"Loft":0.0,"DynamicLoft":0.0,"CR":0.0,"HI":0.0,"VI":0.0,"SmashFactor":1.40973878}"#;

    #[test]
    fn metric_row_matches_its_shot() {
        let r = result_for(SAMPLE, 191.029129 / MPH_TO_KMH, 16.8).expect("matches");
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
}
