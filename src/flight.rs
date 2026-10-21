//! Estimated ball flight from the launch data. GSPro doesn't send its flight results back, so
//! carry, roll-out, apex and lateral are estimated here: drag and lift from the spin factor,
//! integrated in 3D (no wind, sea level, 20 °C). Fit to typical launch-monitor carries
//! (~213 m for a 149 mph driver, ~159 m for a 120 mph 7-iron); the roll-out is a rough guess.
//!
//! Coordinates in metres: x to the right, y up, z towards the target.

use crate::mapping::GsBall;

/// Ball mass (kg) and radius (m), air density (kg/m³).
const MASS: f64 = 0.045_93;
const RADIUS: f64 = 0.021_35;
const AIR: f64 = 1.2;
const GRAVITY: f64 = 9.81;
/// Integration step (s) and the longest flight considered.
const DT: f64 = 0.02;
const MAX_TIME: f64 = 12.0;
/// Spin lost per second in flight.
const SPIN_DECAY: f64 = 0.04;
const MPH_TO_MS: f64 = 0.447_04;

/// Estimated flight of one shot.
#[derive(Debug, Clone, PartialEq)]
pub struct Flight {
    /// Points along the flight, one per integration step, from the tee to the landing point.
    pub path: Vec<[f64; 3]>,
    /// Distance towards the target where the ball lands (m).
    pub carry: f64,
    /// Lateral offset where the ball lands, right positive (m).
    pub carry_side: f64,
    /// Highest point (m).
    pub apex: f64,
    /// Time in the air (s).
    pub hang_time: f64,
    /// Angle of descent at landing (degrees).
    pub descent: f64,
    /// Distance towards the target after the roll-out (m).
    pub total: f64,
    /// Lateral offset after the roll-out, right positive (m).
    pub total_side: f64,
}

/// Estimate the flight of a ball with GSPro's conventions (speed in mph, angles in degrees,
/// HLA and spin axis positive to the right).
#[must_use]
pub fn fly(ball: &GsBall) -> Flight {
    let area = std::f64::consts::PI * RADIUS * RADIUS;
    let (vla, hla, axis) = (ball.vla.to_radians(), ball.hla.to_radians(), ball.spin_axis.to_radians());
    let speed = ball.speed * MPH_TO_MS;
    let mut v = [speed * vla.cos() * hla.sin(), speed * vla.sin(), speed * vla.cos() * hla.cos()];
    let mut p = [0.0_f64; 3];
    let mut spin = ball.total_spin.max(0.0) * std::f64::consts::TAU / 60.0;
    // Spin axis direction: backspin around −x, tilted by the spin axis (positive curves right).
    let (ox, oy) = (-axis.cos(), axis.sin());
    let (mut t, mut apex) = (0.0, 0.0_f64);
    let mut path = vec![p];
    while p[1] >= 0.0 && t < MAX_TIME {
        let sp = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(0.1);
        let s = spin * RADIUS / sp;
        let cd = 0.19 + 0.3 * s;
        let cl = (1.6 * s / (1.0 + 5.0 * s) + 0.05).min(0.3);
        let k = 0.5 * AIR * area * sp / MASS;
        // Lift along ω × v (unscaled; |ω| = 1).
        let lift = [oy * v[2], -ox * v[2], ox * v[1] - oy * v[0]];
        v[0] += (-k * cd * v[0] + k * cl * lift[0]) * DT;
        v[1] += (-k * cd * v[1] + k * cl * lift[1] - GRAVITY) * DT;
        v[2] += (-k * cd * v[2] + k * cl * lift[2]) * DT;
        for i in 0..3 {
            p[i] += v[i] * DT;
        }
        t += DT;
        spin *= 1.0 - SPIN_DECAY * DT;
        apex = apex.max(p[1]);
        path.push([p[0], p[1].max(0.0), p[2]]);
    }

    // Roll-out (rough): faster and flatter landings with less spin roll further, along the
    // landing direction.
    let vh = (v[0] * v[0] + v[2] * v[2]).sqrt();
    let descent = (-v[1]).atan2(vh).to_degrees();
    let rpm = spin * 60.0 / std::f64::consts::TAU;
    let roll = vh * 2.5 * (1.0 - descent / 65.0).max(0.0) * (1.0 - rpm / 9000.0).max(0.25);
    let (dx, dz) = if vh > 0.0 { (v[0] / vh, v[2] / vh) } else { (0.0, 1.0) };
    Flight {
        path,
        carry: p[2],
        carry_side: p[0],
        apex,
        hang_time: t,
        descent,
        total: p[2] + roll * dz,
        total_side: p[0] + roll * dx,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ball(speed: f64, vla: f64, spin: f64, hla: f64, axis: f64) -> GsBall {
        GsBall { speed, spin_axis: axis, total_spin: spin, back_spin: spin, side_spin: 0.0, hla, vla }
    }

    #[test]
    fn carries_match_typical_launch_monitor_numbers() {
        let driver = fly(&ball(149.0, 10.5, 2600.0, 0.0, 0.0));
        let iron = fly(&ball(120.0, 16.3, 7100.0, 0.0, 0.0));
        let wedge = fly(&ball(100.0, 24.0, 9000.0, 0.0, 0.0));
        for (f, carry) in [(&driver, 213.0), (&iron, 159.0), (&wedge, 120.0)] {
            assert!((f.carry - carry).abs() < 8.0, "carry {:.1} m, expected ~{carry} m", f.carry);
            assert!(f.total > f.carry, "rolls out");
            assert!(f.carry_side.abs() < 1e-9 && f.total_side.abs() < 1e-9, "straight");
        }
        // Higher, steeper and shorter roll with the shorter clubs.
        assert!(driver.apex < wedge.apex + 15.0 && driver.apex > 15.0);
        assert!(driver.descent < iron.descent && iron.descent < wedge.descent);
        assert!(driver.total - driver.carry > wedge.total - wedge.carry);
    }

    #[test]
    fn start_direction_and_spin_axis_move_the_ball_right() {
        let start_right = fly(&ball(120.0, 16.0, 7000.0, 3.0, 0.0));
        let fade = fly(&ball(120.0, 16.0, 7000.0, 0.0, 10.0));
        let draw = fly(&ball(120.0, 16.0, 7000.0, 0.0, -10.0));
        assert!(start_right.carry_side > 5.0);
        assert!(fade.carry_side > 5.0, "positive axis curves right: {:.1}", fade.carry_side);
        assert!((fade.carry_side + draw.carry_side).abs() < 1e-6, "mirrored");
        assert!(fade.total_side > fade.carry_side, "rolls on to the right");
    }

    #[test]
    fn path_runs_from_the_tee_to_the_landing_point() {
        let f = fly(&ball(149.0, 10.5, 2600.0, 0.0, 0.0));
        assert_eq!(f.path.first(), Some(&[0.0, 0.0, 0.0]));
        let last = f.path.last().unwrap();
        assert!((last[2] - f.carry).abs() < 1e-9 && last[1] == 0.0);
        assert!(f.hang_time > 5.0 && f.hang_time < 8.0);
    }

    #[test]
    fn a_dribble_does_not_hang() {
        let f = fly(&ball(5.0, -10.0, 0.0, 0.0, 0.0));
        assert!(f.carry >= 0.0 && f.carry < 1.0);
        assert!(f.hang_time <= DT * 2.0);
    }
}
