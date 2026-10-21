//! Shot history: every real shot is kept for the session and appended to
//! `shots.jsonl` next to the settings, so it survives restarts and can be
//! exported as CSV.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use crate::mapping::{GsBall, GsClub, Tempo};
use crate::settings::Settings;

/// Write shots to disk (off in demo mode, so screenshots don't pollute the history).
pub static PERSIST: AtomicBool = AtomicBool::new(true);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ShotRecord {
    /// Local time, RFC 3339.
    pub time: String,
    pub shot_number: u32,
    /// Club selected in GSPro at the time of the shot (e.g. `DR`, `I7`).
    pub club_code: Option<String>,
    pub delivered: bool,
    pub ball: GsBall,
    pub club: Option<GsClub>,
    /// Swing timing; missing in history files written before 0.9.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tempo: Option<Tempo>,
}

impl ShotRecord {
    #[must_use]
    pub fn new(ball: GsBall, club: Option<GsClub>, club_code: Option<String>, delivered: bool, shot_number: u32) -> Self {
        Self {
            time: chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
            shot_number,
            club_code,
            delivered,
            ball,
            club,
            tempo: None,
        }
    }

    /// Ball speed / club speed, if the club was tracked.
    #[must_use]
    pub fn smash(&self) -> Option<f64> {
        smash(&self.ball, self.club.as_ref())
    }

    /// Clock time (`HH:MM:SS`) for display.
    #[must_use]
    pub fn clock(&self) -> &str {
        self.time.get(11..19).unwrap_or(&self.time)
    }
}

/// Smash factor: ball speed / club speed.
#[must_use]
pub fn smash(ball: &GsBall, club: Option<&GsClub>) -> Option<f64> {
    club.filter(|c| c.speed > 0.0).map(|c| ball.speed / c.speed)
}

fn path() -> Option<PathBuf> {
    Some(Settings::data_dir()?.join("shots.jsonl"))
}

/// Append one shot to `shots.jsonl`.
///
/// # Errors
/// Returns a message if the file cannot be written.
pub fn append(rec: &ShotRecord) -> Result<(), String> {
    if !PERSIST.load(Ordering::Relaxed) {
        return Ok(());
    }
    let path = path().ok_or("HOME not set")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut line = serde_json::to_string(rec).map_err(|e| e.to_string())?;
    line.push('\n');
    let mut file = std::fs::OpenOptions::new().create(true).read(true).append(true).open(&path).map_err(|e| e.to_string())?;
    // After a cut-off last line (crash, full disk), start on a new line so this shot stays readable.
    if ends_mid_line(&mut file).map_err(|e| e.to_string())? {
        line.insert(0, '\n');
    }
    file.write_all(line.as_bytes()).map_err(|e| e.to_string())
}

fn ends_mid_line(file: &mut std::fs::File) -> std::io::Result<bool> {
    if file.metadata()?.len() == 0 {
        return Ok(false);
    }
    file.seek(SeekFrom::End(-1))?;
    let mut last = [0u8; 1];
    file.read_exact(&mut last)?;
    Ok(last[0] != b'\n')
}

/// All shots ever recorded (skips unreadable lines).
#[must_use]
pub fn load_all() -> Vec<ShotRecord> {
    path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| s.lines().filter_map(|l| serde_json::from_str(l).ok()).collect())
        .unwrap_or_default()
}

const CSV_HEADER: &str = "time,shot_number,club,delivered,ball_speed_mph,vla_deg,hla_deg,total_spin_rpm,\
spin_axis_deg,back_spin_rpm,side_spin_rpm,club_speed_mph,aoa_deg,path_deg,face_to_target_deg,face_to_path_deg,smash,\
backswing_s,downswing_s,tempo";

/// Shots as CSV (speeds in mph, angles in degrees, as sent to GSPro).
#[must_use]
pub fn to_csv(shots: &[ShotRecord]) -> String {
    let opt = |v: Option<f64>, digits: usize| v.map_or_else(String::new, |v| crate::num(v, digits));
    let mut out = String::from(CSV_HEADER);
    out.push('\n');
    for s in shots {
        let b = &s.ball;
        let c = s.club.as_ref();
        let fields = [
            s.time.clone(),
            s.shot_number.to_string(),
            s.club_code.clone().unwrap_or_default(),
            s.delivered.to_string(),
            format!("{:.1}", b.speed),
            crate::num(b.vla, 1),
            crate::num(b.hla, 1),
            format!("{:.0}", b.total_spin),
            crate::num(b.spin_axis, 1),
            crate::num(b.back_spin, 0),
            crate::num(b.side_spin, 0),
            opt(c.map(|c| c.speed), 1),
            opt(c.map(|c| c.angle_of_attack), 1),
            opt(c.map(|c| c.path), 1),
            opt(c.map(|c| c.face_to_target), 1),
            opt(c.map(|c| c.face_to_target - c.path), 1),
            opt(s.smash(), 2),
            opt(s.tempo.map(|t| t.backswing), 2),
            opt(s.tempo.map(|t| t.downswing), 2),
            opt(s.tempo.map(|t| t.ratio()), 1),
        ];
        out.push_str(&fields.join(","));
        out.push('\n');
    }
    out
}

/// CSV layouts for the export.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    /// Our own columns (see [`to_csv`]).
    #[default]
    OpenLaunch,
    /// Garmin Golf app range session export (`DrivingRange-*.csv`).
    GarminGolf,
    /// Awesome Golf session export (`ags-shots-*.csv`).
    AwesomeGolf,
}

impl ExportFormat {
    pub const ALL: [Self; 3] = [Self::OpenLaunch, Self::GarminGolf, Self::AwesomeGolf];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::OpenLaunch => "OpenLaunch",
            Self::GarminGolf => "Garmin Golf",
            Self::AwesomeGolf => "Awesome Golf",
        }
    }

    /// File name in the style of the original app.
    #[must_use]
    pub fn file_name(self, now: chrono::DateTime<chrono::Local>) -> String {
        match self {
            Self::OpenLaunch => format!("openlaunch-shots-{}.csv", now.format("%Y-%m-%d")),
            Self::GarminGolf => {
                format!("DrivingRange-{}_+0000.csv", now.with_timezone(&chrono::Utc).format("%Y-%m-%d_%H-%M-%S"))
            }
            Self::AwesomeGolf => format!("ags-shots-{}.csv", now.format("%Y-%m-%d")),
        }
    }

    /// File contents.
    #[must_use]
    pub fn export(self, shots: &[ShotRecord]) -> String {
        match self {
            Self::OpenLaunch => to_csv(shots),
            Self::GarminGolf => to_garmin_csv(shots),
            Self::AwesomeGolf => to_awesome_golf_csv(shots),
        }
    }
}

/// GSPro club code → club type as the Garmin Golf and Awesome Golf exports write it.
fn club_type(code: Option<&str>) -> String {
    let Some(code) = code else { return String::new() };
    let num = code.get(1..).unwrap_or("");
    match code {
        "DR" => "Driver".into(),
        "PW" => "Pitching Wedge".into(),
        "GW" => "Gap Wedge".into(),
        "SW" => "Sand Wedge".into(),
        "LW" => "Lob Wedge".into(),
        "PT" => "Putter".into(),
        c if c.starts_with('W') && !num.is_empty() => format!("{num} Wood"),
        c if c.starts_with('H') && !num.is_empty() => format!("{num} Hybrid"),
        c if c.starts_with('I') && !num.is_empty() => format!("{num} Iron"),
        c => c.to_string(),
    }
}

/// Reformat the stored RFC 3339 time (falls back to the raw string).
fn format_time(time: &str, fmt: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(time).map_or_else(|_| time.to_string(), |t| t.format(fmt).to_string())
}

const GARMIN_HEADER: &str = "Date,Player,Club Name,Brand/Model,Club Type,Club Speed,Attack Angle,Club Path,Club Face,\
Face to Path,Ball Speed,Smash Factor,Launch Angle,Launch Direction,Backspin,Sidespin,Spin Rate,Spin Rate Type,Spin Axis,\
Apex Height,Carry Distance,Carry Deviation Angle,Carry Deviation Distance,Total Distance,Total Deviation Angle,\
Total Deviation Distance,Target Total Distance,Target Carry Distance,Note,Tag,Air Density,Temperature,Air Pressure,\
Relative Humidity,Back Stroke Length,Target Backswing Time,Target Downswing Time,Forward Stroke Length,Backswing Time,\
Downswing Time,Target Tempo,Swing Tempo";
const GARMIN_UNITS: &str = ",,,,,[mph],[deg],[deg],[deg],[deg],[mph],,[deg],[deg],[rpm],[rpm],[rpm],,[deg],[yds],[Yards],\
[deg],[Yards],[Yards],[deg],[Yards],[Yards],[Yards],,,[g/L],[deg F],[kPa],[%],[Inches],[sec],[sec],[Inches],[sec],[sec],,";

/// Garmin Golf range export: UTF-8 BOM, header + units row, imperial units, R10 sign
/// conventions (spin axis not inverted). Flight results (carry, apex, …) stay empty –
/// the R10 only measures launch data, the Garmin app computes the rest.
#[must_use]
pub fn to_garmin_csv(shots: &[ShotRecord]) -> String {
    let mut out = format!("\u{feff}{GARMIN_HEADER}\n{GARMIN_UNITS}\n");
    let cols = GARMIN_HEADER.split(',').count();
    for s in shots {
        let b = &s.ball;
        let c = s.club.as_ref();
        // `+ 0.0` turns -0.0 into 0.0, so no "-0".
        let n = |v: f64| (v + 0.0).to_string();
        let opt = |v: Option<f64>| v.map_or_else(String::new, n);
        let mut fields = vec![
            format_time(&s.time, "%-m/%-d/%y %H:%M:%S"),
            String::new(),
            String::new(),
            String::new(),
            club_type(s.club_code.as_deref()),
            opt(c.map(|c| c.speed)),
            opt(c.map(|c| c.angle_of_attack)),
            opt(c.map(|c| c.path)),
            opt(c.map(|c| c.face_to_target)),
            opt(c.map(|c| r2(c.face_to_target - c.path))),
            n(b.speed),
            opt(s.smash().map(r2)),
            n(b.vla),
            n(b.hla),
            n(b.back_spin),
            // Spin axis and sidespin in the R10's convention (GSPro's inverted), same sign.
            n(-b.side_spin),
            n(b.total_spin),
            String::new(),
            n(-b.spin_axis),
        ];
        fields.resize(cols, String::new());
        if let Some(t) = &s.tempo {
            // Despite the "[sec]" units row, Garmin writes these in milliseconds (e.g. 549.0) and the tempo with one decimal.
            let col = |name: &str| GARMIN_HEADER.split(',').position(|h| h == name).expect("Garmin column");
            fields[col("Backswing Time")] = format!("{:.1}", (t.backswing * 1000.0).round());
            fields[col("Downswing Time")] = format!("{:.1}", (t.downswing * 1000.0).round());
            fields[col("Swing Tempo")] = format!("{:.1}", t.ratio());
        }
        out.push_str(&fields.join(","));
        out.push('\n');
    }
    out
}

const AWESOME_HEADER: &str = "Date,Club Type,Club Description,Altitude,Club Speed,Ball Speed,Carry Distance,\
Total Distance,Roll Distance,Smash,Vertical Launch,Peak Height,Descent Angle,Horizontal Launch,Carry Lateral Distance,\
Total Lateral Distance,Carry Curve Distance,Total Curve Distance,Attack Angle,Dynamic Loft,Spin Loft,Spin Rate,\
Spin Axis,Spin Reading,Low Point,Club Path,Face Path,Face Target,Swing Plane Tilt,Swing Plane Rotation,\
Shot Classification";
const AWESOME_UNITS: &str = ",,,[ft],[mph],[mph],[yd],[yd],[yd],,[deg],[ft],[deg],[deg],[yd],[yd],[yd],[yd],[deg],\
[deg],[deg],[rpm],[deg],,[in],[deg],[deg],[deg],[deg],[deg],";

/// Awesome Golf export: header + units row, imperial units, two decimals,
/// spin axis positive = right (like GSPro). Flight results stay empty.
#[must_use]
pub fn to_awesome_golf_csv(shots: &[ShotRecord]) -> String {
    let mut out = format!("{AWESOME_HEADER}\n{AWESOME_UNITS}\n");
    let f2 = |v: f64| crate::num(v, 2);
    let opt = |v: Option<f64>| v.map_or_else(String::new, f2);
    for s in shots {
        let b = &s.ball;
        let c = s.club.as_ref();
        let none = String::new;
        let fields = [
            format_time(&s.time, "%Y-%m-%d %H:%M:%S"),
            club_type(s.club_code.as_deref()),
            none(),
            none(),
            opt(c.map(|c| c.speed)),
            f2(b.speed),
            none(),
            none(),
            none(),
            opt(s.smash()),
            f2(b.vla),
            none(),
            none(),
            f2(b.hla),
            none(),
            none(),
            none(),
            none(),
            opt(c.map(|c| c.angle_of_attack)),
            none(),
            none(),
            format!("{:.0}", b.total_spin),
            f2(b.spin_axis),
            none(),
            none(),
            opt(c.map(|c| c.path)),
            opt(c.map(|c| c.face_to_target - c.path)),
            opt(c.map(|c| c.face_to_target)),
            none(),
            none(),
            none(),
        ];
        out.push_str(&fields.join(","));
        out.push('\n');
    }
    out
}

fn r2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// Mean and standard deviation of a series.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stat {
    pub mean: f64,
    pub sd: f64,
}

impl Stat {
    fn of(values: impl IntoIterator<Item = f64>) -> Option<Self> {
        let v: Vec<f64> = values.into_iter().collect();
        if v.is_empty() {
            return None;
        }
        let n = v.len() as f64;
        let mean = v.iter().sum::<f64>() / n;
        let sd = if v.len() > 1 {
            (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0)).sqrt()
        } else {
            0.0
        };
        Some(Self { mean, sd })
    }
}

/// Averages and spread for one club.
#[derive(Debug, Clone, PartialEq)]
pub struct ClubStats {
    /// GSPro club code, or `None` for shots before GSPro reported a club.
    pub club_code: Option<String>,
    pub shots: usize,
    pub ball_speed: Option<Stat>,
    pub club_speed: Option<Stat>,
    pub smash: Option<Stat>,
    pub vla: Option<Stat>,
    pub total_spin: Option<Stat>,
    pub hla: Option<Stat>,
    pub face_to_path: Option<Stat>,
    /// Backswing : downswing ratio.
    pub tempo: Option<Stat>,
}

/// Per-club statistics, in order of first use.
#[must_use]
pub fn club_stats(shots: &[ShotRecord]) -> Vec<ClubStats> {
    let mut codes: Vec<Option<String>> = Vec::new();
    for s in shots {
        if !codes.contains(&s.club_code) {
            codes.push(s.club_code.clone());
        }
    }
    codes
        .into_iter()
        .map(|code| {
            let group: Vec<&ShotRecord> = shots.iter().filter(|s| s.club_code == code).collect();
            let clubs = || group.iter().filter_map(|s| s.club.as_ref());
            ClubStats {
                shots: group.len(),
                ball_speed: Stat::of(group.iter().map(|s| s.ball.speed)),
                club_speed: Stat::of(clubs().map(|c| c.speed)),
                smash: Stat::of(group.iter().filter_map(|s| s.smash())),
                vla: Stat::of(group.iter().map(|s| s.ball.vla)),
                total_spin: Stat::of(group.iter().map(|s| s.ball.total_spin)),
                hla: Stat::of(group.iter().map(|s| s.ball.hla)),
                face_to_path: Stat::of(clubs().map(|c| c.face_to_target - c.path)),
                tempo: Stat::of(group.iter().filter_map(|s| s.tempo.map(|t| t.ratio()))),
                club_code: code,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(code: Option<&str>, speed: f64, club_speed: Option<f64>) -> ShotRecord {
        let ball = GsBall {
            speed,
            spin_axis: 1.0,
            total_spin: 6000.0,
            back_spin: 5999.0,
            side_spin: 105.0,
            hla: 0.5,
            vla: 18.0,
        };
        let club = club_speed.map(|speed| GsClub {
            speed,
            speed_at_impact: speed,
            angle_of_attack: -3.0,
            face_to_target: 1.0,
            path: -1.0,
        });
        ShotRecord {
            time: "2026-10-19T18:30:05+02:00".into(),
            shot_number: 1,
            club_code: code.map(Into::into),
            delivered: true,
            ball,
            club,
            tempo: club_speed.map(|_| Tempo { backswing: 0.81, downswing: 0.27 }),
        }
    }

    #[test]
    fn smash_factor() {
        assert_eq!(rec(None, 120.0, Some(80.0)).smash(), Some(1.5));
        assert_eq!(rec(None, 120.0, None).smash(), None);
        assert_eq!(rec(None, 120.0, Some(0.0)).smash(), None);
    }

    #[test]
    fn clock_is_extracted() {
        assert_eq!(rec(None, 1.0, None).clock(), "18:30:05");
    }

    #[test]
    fn stats_group_by_club() {
        let shots = [
            rec(Some("I7"), 118.0, Some(84.0)),
            rec(Some("DR"), 150.0, Some(100.0)),
            rec(Some("I7"), 122.0, None),
        ];
        let stats = club_stats(&shots);
        assert_eq!(stats.len(), 2);
        assert_eq!(stats[0].club_code.as_deref(), Some("I7"));
        assert_eq!(stats[0].shots, 2);
        let bs = stats[0].ball_speed.unwrap();
        assert!((bs.mean - 120.0).abs() < 1e-9);
        assert!((bs.sd - 8f64.sqrt()).abs() < 1e-9);
        // Club speed only from the tracked shot.
        assert_eq!(stats[0].club_speed, Some(Stat { mean: 84.0, sd: 0.0 }));
        assert_eq!(stats[1].face_to_path, Some(Stat { mean: 2.0, sd: 0.0 }));
    }

    #[test]
    fn csv_has_header_and_one_row_per_shot() {
        let csv = to_csv(&[rec(Some("PW"), 90.0, Some(70.0)), rec(None, 80.0, None)]);
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines.len(), 3);
        let cols = lines[0].split(',').count();
        assert!(lines.iter().all(|l| l.split(',').count() == cols));
        assert!(lines[1].starts_with("2026-10-19T18:30:05+02:00,1,PW,true,90.0,"));
        assert!(lines[1].ends_with(",1.29,0.81,0.27,3.0"), "{}", lines[1]);
        assert!(lines[2].ends_with(",,,,,,,,,"));
    }

    #[test]
    fn cut_off_last_line_is_detected() {
        let path = std::env::temp_dir().join(format!("openlaunch-cutoff-{}.jsonl", std::process::id()));
        let open = || std::fs::OpenOptions::new().create(true).read(true).append(true).open(&path).unwrap();
        std::fs::write(&path, "").unwrap();
        assert!(!ends_mid_line(&mut open()).unwrap(), "empty file");
        std::fs::write(&path, "{\"a\":1}\n").unwrap();
        assert!(!ends_mid_line(&mut open()).unwrap(), "complete line");
        std::fs::write(&path, "{\"a\":1}\n{\"b\":").unwrap();
        assert!(ends_mid_line(&mut open()).unwrap(), "cut off");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn garmin_export_matches_layout() {
        let csv = to_garmin_csv(&[rec(Some("I7"), 120.0, Some(84.0)), rec(None, 80.0, None)]);
        assert!(csv.starts_with('\u{feff}'));
        let lines: Vec<&str> = csv.trim_start_matches('\u{feff}').lines().collect();
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0].split(',').count(), 42);
        assert!(lines.iter().all(|l| l.split(',').count() == 42), "{lines:?}");
        assert!(lines[1].starts_with(",,,,,[mph],"));
        let row: Vec<&str> = lines[2].split(',').collect();
        assert_eq!(row[0], "10/19/26 18:30:05");
        assert_eq!(row[4], "7 Iron");
        assert_eq!(row[10], "120");
        assert_eq!(row[11], "1.43");
        assert_eq!(row[18], "-1", "spin axis back in R10 convention");
        assert_eq!(row[15], "-105", "sidespin with the same sign as the spin axis");

        // No "-0" for values that are zero (an R10 spin axis of 0 maps to -0.0).
        let mut straight = rec(Some("I7"), 120.0, Some(84.0));
        (straight.ball.spin_axis, straight.ball.side_spin, straight.ball.hla) = (-0.0, -0.0, -0.0);
        let csv = to_garmin_csv(&[straight]);
        let row: Vec<&str> = csv.lines().nth(2).unwrap().split(',').collect();
        assert_eq!([row[13], row[15], row[18]], ["0", "0", "0"]);
        assert_eq!(&row[38..42], ["810.0", "270.0", "", "3.0"]);
    }

    #[test]
    fn awesome_golf_export_matches_layout() {
        let csv = to_awesome_golf_csv(&[rec(Some("DR"), 150.0, Some(100.0))]);
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines.iter().all(|l| l.split(',').count() == 31), "{lines:?}");
        let row: Vec<&str> = lines[2].split(',').collect();
        assert_eq!(row[0], "2026-10-19 18:30:05");
        assert_eq!(row[1], "Driver");
        assert_eq!(row[5], "150.00");
        assert_eq!(row[9], "1.50");
        assert_eq!(row[21], "6000");
        assert_eq!(row[22], "1.00");
        assert_eq!(row[26], "2.00");
    }

    #[test]
    fn club_types() {
        assert_eq!(club_type(Some("W3")), "3 Wood");
        assert_eq!(club_type(Some("H4")), "4 Hybrid");
        assert_eq!(club_type(Some("I7")), "7 Iron");
        assert_eq!(club_type(Some("SW")), "Sand Wedge");
        assert_eq!(club_type(None), "");
    }

    #[test]
    fn history_without_tempo_still_loads() {
        let mut r = rec(Some("DR"), 150.0, None);
        r.tempo = None;
        let json = serde_json::to_string(&r).unwrap();
        assert!(!json.contains("tempo"));
        assert_eq!(serde_json::from_str::<ShotRecord>(&json).unwrap(), r);
    }

    #[test]
    fn record_roundtrips_as_json() {
        let r = rec(Some("DR"), 150.0, Some(100.0));
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<ShotRecord>(&json).unwrap(), r);
    }
}
