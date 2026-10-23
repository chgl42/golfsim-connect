//! Shot shape from the start direction (HLA) and the curve (spin axis), with the names golfers
//! use: straight, draw, fade, hook, slice, push, pull and their combinations (push-draw …).
//! Names are a right-hander's; a left-hander's shots are mirrored first.

/// Start direction counts as straight within this many degrees of the target line (3° are about
/// 8 m at 150 m).
const STRAIGHT_START: f64 = 3.0;
/// Spin axis (degrees) up to which the ball flies straight, and from which it hooks or slices.
const NO_CURVE: f64 = 3.0;
const HARD_CURVE: f64 = 10.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    Left,
    Straight,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Curve {
    HardLeft,
    Left,
    None,
    Right,
    HardRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shape {
    pub start: Start,
    pub curve: Curve,
}

impl Shape {
    /// From GSPro's HLA and spin axis (degrees, positive to the right / curving right).
    #[must_use]
    pub fn of(hla: f64, spin_axis: f64, left_handed: bool) -> Self {
        let (hla, axis) = if left_handed { (-hla, -spin_axis) } else { (hla, spin_axis) };
        let start = if hla <= -STRAIGHT_START {
            Start::Left
        } else if hla >= STRAIGHT_START {
            Start::Right
        } else {
            Start::Straight
        };
        let curve = match axis {
            a if a <= -HARD_CURVE => Curve::HardLeft,
            a if a <= -NO_CURVE => Curve::Left,
            a if a >= HARD_CURVE => Curve::HardRight,
            a if a >= NO_CURVE => Curve::Right,
            _ => Curve::None,
        };
        Self { start, curve }
    }

    /// The name, e.g. "Draw", "Push", "Pull-Hook", "Straight" (English; golfers use these in
    /// German too, except for "Straight").
    #[must_use]
    pub fn name(self) -> String {
        let start = match self.start {
            Start::Left => Some("Pull"),
            Start::Straight => None,
            Start::Right => Some("Push"),
        };
        let curve = match self.curve {
            Curve::HardLeft => Some("Hook"),
            Curve::Left => Some("Draw"),
            Curve::None => None,
            Curve::Right => Some("Fade"),
            Curve::HardRight => Some("Slice"),
        };
        match (start, curve) {
            (None, None) => "Straight".into(),
            (Some(s), None) => s.into(),
            (None, Some(c)) => c.into(),
            (Some(s), Some(c)) => format!("{s}-{c}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_of_the_nine_classic_shapes() {
        let name = |hla, axis| Shape::of(hla, axis, false).name();
        assert_eq!(name(0.5, 1.0), "Straight");
        assert_eq!(name(0.0, -5.0), "Draw");
        assert_eq!(name(0.0, 6.0), "Fade");
        assert_eq!(name(-1.0, -14.0), "Hook");
        assert_eq!(name(1.0, 15.0), "Slice");
        assert_eq!(name(3.5, 0.0), "Push");
        assert_eq!(name(-3.0, 2.0), "Pull");
        assert_eq!(name(4.0, -6.0), "Push-Draw");
        assert_eq!(name(-4.0, 12.0), "Pull-Slice");
    }

    #[test]
    fn left_handers_are_mirrored() {
        // A left-hander's draw curves right and a push starts left.
        assert_eq!(Shape::of(0.0, 5.0, true).name(), "Draw");
        assert_eq!(Shape::of(-3.0, 0.0, true).name(), "Push");
    }

    #[test]
    fn thresholds_are_inclusive() {
        assert_eq!(Shape::of(3.0, 3.0, false).name(), "Push-Fade");
        assert_eq!(Shape::of(2.9, 2.9, false).name(), "Straight");
        assert_eq!(Shape::of(0.0, 10.0, false).name(), "Slice");
    }
}
