//! Conversion from R10 shot data to GSPro OpenConnect ball/club data.
//!
//! Conventions follow mholow/gsp-r10-adapter (gspro-r10), which has been used
//! with real R10 hardware and GSPro for years:
//! - speeds: m/s -> mph
//! - HLA = launch direction, VLA = launch angle (unchanged)
//! - spin axis is inverted (R10 and GSPro use opposite signs)
//! - back/side spin are derived from total spin and the inverted axis
//! - face/path/attack angle are passed through unchanged

use serde::{Deserialize, Serialize};
use tenover::proto::{BallData, ClubData, SwingData};

/// Metres per second to miles per hour.
pub const MS_TO_MPH: f64 = 2.236_936;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct GsBall {
    pub speed: f64,
    pub spin_axis: f64,
    pub total_spin: f64,
    pub back_spin: f64,
    pub side_spin: f64,
    #[serde(rename = "HLA")]
    pub hla: f64,
    #[serde(rename = "VLA")]
    pub vla: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct GsClub {
    pub speed: f64,
    pub speed_at_impact: f64,
    pub angle_of_attack: f64,
    pub face_to_target: f64,
    pub path: f64,
}

/// Swing timing measured by the R10, in seconds. Not sent to GSPro (OpenConnect has no field for it).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Tempo {
    pub backswing: f64,
    pub downswing: f64,
}

impl Tempo {
    /// Backswing : downswing, e.g. 3.0 for the classic 3:1 tempo.
    #[must_use]
    pub fn ratio(&self) -> f64 {
        self.backswing / self.downswing
    }
}

/// Swing tempo from the R10's timestamps, or `None` if the swing wasn't timed
/// (missing or implausible values).
#[must_use]
pub fn tempo(s: &SwingData) -> Option<Tempo> {
    let back = s.downswing_start.checked_sub(s.backswing_start)?;
    let down = s.impact.checked_sub(s.downswing_start)?;
    ((300..=3000).contains(&back) && (100..=1000).contains(&down))
        .then(|| Tempo { backswing: f64::from(back) / 1000.0, downswing: f64::from(down) / 1000.0 })
}

/// Round to one decimal place so GSPro logs stay readable.
fn r1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

#[must_use]
pub fn ball(b: &BallData) -> GsBall {
    let axis = -f64::from(b.spin_axis);
    let total = f64::from(b.total_spin);
    let axis_rad = axis.to_radians();
    GsBall {
        speed: r1(f64::from(b.ball_speed) * MS_TO_MPH),
        spin_axis: r1(axis),
        total_spin: total.round(),
        back_spin: (total * axis_rad.cos()).round(),
        side_spin: (total * axis_rad.sin()).round(),
        hla: r1(f64::from(b.launch_direction)),
        vla: r1(f64::from(b.launch_angle)),
    }
}

/// Club data, or `None` when the R10 did not track the club
/// (it reports all zeros on slow swings).
#[must_use]
pub fn club(c: &ClubData) -> Option<GsClub> {
    if c.club_head_speed <= 0.0 {
        return None;
    }
    let speed = r1(f64::from(c.club_head_speed) * MS_TO_MPH);
    Some(GsClub {
        speed,
        speed_at_impact: speed,
        angle_of_attack: r1(f64::from(c.attack_angle)),
        face_to_target: r1(f64::from(c.face_angle)),
        path: r1(f64::from(c.path_angle)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tenover::proto::SpinCalcType;

    fn sample_ball(spin_axis: f32) -> BallData {
        BallData {
            launch_angle: 16.3,
            launch_direction: -1.2,
            ball_speed: 53.6, // ~120 mph
            spin_axis,
            total_spin: 7000.0,
            backspin: 0.0,
            sidespin: 0.0,
            spin_calc_type: SpinCalcType::Ratio,
        }
    }

    #[test]
    fn converts_speed_and_angles() {
        let g = ball(&sample_ball(0.0));
        assert!((g.speed - 119.9).abs() < 0.2, "speed {}", g.speed);
        assert_eq!(g.vla, 16.3);
        assert_eq!(g.hla, -1.2);
        assert_eq!(g.total_spin, 7000.0);
        assert_eq!(g.back_spin, 7000.0);
        assert_eq!(g.side_spin, 0.0);
    }

    #[test]
    fn inverts_spin_axis_and_derives_side_spin() {
        let g = ball(&sample_ball(10.0));
        assert_eq!(g.spin_axis, -10.0);
        assert!(g.side_spin < 0.0);
        let expected_side = 7000.0 * (-10f64).to_radians().sin();
        assert!((g.side_spin - expected_side.round()).abs() < 1.0);
    }

    #[test]
    fn untracked_club_is_none() {
        let c = ClubData { club_head_speed: 0.0, face_angle: 0.0, path_angle: 0.0, attack_angle: 0.0 };
        assert!(club(&c).is_none());
        let c = ClubData { club_head_speed: 38.0, face_angle: 1.5, path_angle: -2.0, attack_angle: -4.1 };
        let g = club(&c).unwrap();
        assert!((g.speed - 85.0).abs() < 0.2);
        assert_eq!(g.face_to_target, 1.5);
        assert_eq!(g.path, -2.0);
        assert_eq!(g.angle_of_attack, -4.1);
    }

    #[test]
    fn tempo_from_swing_timestamps() {
        let swing = |b, d, i| SwingData { backswing_start: b, downswing_start: d, impact: i, follow_through_end: i + 500 };
        let t = tempo(&swing(10_000, 10_810, 11_080)).unwrap();
        assert!((t.backswing - 0.81).abs() < 1e-9);
        assert!((t.downswing - 0.27).abs() < 1e-9);
        assert!((t.ratio() - 3.0).abs() < 1e-9);
        assert!(tempo(&swing(0, 0, 0)).is_none(), "untimed swing");
        assert!(tempo(&swing(10_000, 9_000, 11_000)).is_none(), "out of order");
        assert!(tempo(&swing(10_000, 10_100, 10_400)).is_none(), "implausibly short backswing");
    }

    #[test]
    fn json_field_names_match_openconnect() {
        let json = serde_json::to_value(ball(&sample_ball(5.0))).unwrap();
        for key in ["Speed", "SpinAxis", "TotalSpin", "BackSpin", "SideSpin", "HLA", "VLA"] {
            assert!(json.get(key).is_some(), "missing {key}");
        }
    }
}
