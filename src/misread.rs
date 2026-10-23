//! Plausibility check for the R10's readings. A shot that is very likely a misread (e.g. no spin
//! measured, a launch angle no club gives) still goes to GSPro, but is marked and left out of the
//! stats and exports; *Count* on the shots page takes it back.

use serde::{Deserialize, Serialize};

use crate::history;
use crate::mapping::{GsBall, GsClub};

/// Below this ball speed (mph) nothing is checked: chips and putts read all sorts of values.
const MIN_BALL_SPEED: f64 = 40.0;
/// Fewer rpm than this at full speed: the R10 didn't measure the spin.
const MIN_SPIN: f64 = 300.0;
const MAX_SPIN: f64 = 13_000.0;
/// Launch angle (degrees): above a lob wedge's, or into the ground.
const MAX_VLA: f64 = 60.0;
const MIN_VLA: f64 = -10.0;
/// Start direction and spin axis (degrees, either side).
const MAX_HLA: f64 = 30.0;
const MAX_SPIN_AXIS: f64 = 50.0;
/// Smash factor: physics stops near 1.5 (a misread club speed shows up here).
const MAX_SMASH: f64 = 1.58;
const MIN_SMASH: f64 = 0.8;

/// Which reading looks wrong.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Misread {
    /// No spin measured, or far too much.
    Spin,
    /// Launch angle.
    Launch,
    /// Start direction.
    Direction,
    SpinAxis,
    /// Ball and club speed don't fit (usually the club speed).
    Smash,
}

impl Misread {
    /// The English source text for the UI (translated there) and the log.
    #[must_use]
    pub fn reason(self) -> &'static str {
        match self {
            Self::Spin => "spin",
            Self::Launch => "launch angle",
            Self::Direction => "start direction",
            Self::SpinAxis => "spin axis",
            Self::Smash => "smash factor",
        }
    }
}

/// The first reading of the shot that is very likely wrong, if any.
#[must_use]
pub fn check(ball: &GsBall, club: Option<&GsClub>) -> Option<Misread> {
    if ball.speed < MIN_BALL_SPEED {
        return None;
    }
    if ball.total_spin < MIN_SPIN || ball.total_spin > MAX_SPIN {
        return Some(Misread::Spin);
    }
    if !(MIN_VLA..=MAX_VLA).contains(&ball.vla) {
        return Some(Misread::Launch);
    }
    if ball.hla.abs() > MAX_HLA {
        return Some(Misread::Direction);
    }
    if ball.spin_axis.abs() > MAX_SPIN_AXIS {
        return Some(Misread::SpinAxis);
    }
    if let Some(smash) = history::smash(ball, club)
        && !(MIN_SMASH..=MAX_SMASH).contains(&smash)
    {
        return Some(Misread::Smash);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_shots_pass() {
        let (ball, club) = crate::engine::test_shot();
        assert_eq!(check(&ball, Some(&club)), None);
        for s in crate::engine::demo_range() {
            assert_eq!(check(&s.ball, s.club.as_ref()), None, "{s:?}");
        }
    }

    #[test]
    fn misreads_are_found() {
        let (ball, club) = crate::engine::test_shot();
        let with = |f: &dyn Fn(&mut GsBall)| {
            let mut b = ball;
            f(&mut b);
            check(&b, Some(&club))
        };
        assert_eq!(with(&|b| b.total_spin = 0.0), Some(Misread::Spin));
        assert_eq!(with(&|b| b.vla = 72.0), Some(Misread::Launch));
        assert_eq!(with(&|b| b.hla = -35.0), Some(Misread::Direction));
        assert_eq!(with(&|b| b.spin_axis = 60.0), Some(Misread::SpinAxis));
        let slow_club = GsClub { speed: ball.speed / 1.7, ..club };
        assert_eq!(check(&ball, Some(&slow_club)), Some(Misread::Smash));
        // A chip with little spin is not checked.
        assert_eq!(with(&|b| {
            b.speed = 25.0;
            b.total_spin = 100.0;
        }), None);
    }
}
