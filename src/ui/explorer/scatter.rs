//! The correlation tile: two values of the shots against each other.

use serde::{Deserialize, Serialize};

use super::*;

/// The correlation tile (id `scatter`).
pub(super) struct Scatter;

/// A correlation tile's settings: the values on the horizontal and vertical axis.
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Config {
    pub(super) x: Metric,
    pub(super) y: Metric,
}

impl Default for Config {
    fn default() -> Self {
        Self { x: Metric::Hla, y: Metric::SpinAxis }
    }
}

impl TileKind for Scatter {
    fn id(&self) -> &'static str {
        "scatter"
    }

    fn name(&self) -> &'static str {
        tr("Correlation")
    }

    fn options(&self, ui: &mut egui::Ui, tile: &mut Tile, u: Units) {
        tile.change_config(|cfg: &mut Config| {
            setting_row(ui, tr("Axes"), |ui| {
                for (axis, value) in [("x", &mut cfg.x), ("y", &mut cfg.y)] {
                    egui::ComboBox::from_id_salt(("tile-overlay-axis", axis)).width(130.0).selected_text(metric_label(*value, u)).show_ui(ui, |ui| {
                        for m in Metric::ALL {
                            ui.selectable_value(value, m, metric_label(m, u));
                        }
                    });
                }
            });
        });
    }

    fn header(&self, ui: &mut egui::Ui, tile: &mut Tile, ctx: &TileCtx) {
        let cfg: Config = tile.config();
        if let Some(r) = correlation(ctx, cfg.x, cfg.y) {
            ui.label(RichText::new(format!("r = {r:.2}")).size(12.5).color(SOFT))
                .on_hover_text(tr("Correlation of the two values (−1 to 1) over the shots shown, excluded ones left out"));
        }
    }

    fn body(&self, ui: &mut egui::Ui, tile: &mut Tile, ctx: &TileCtx) {
        if let Some(k) = scatter(ui, ctx.index, tile, ctx) {
            ctx.clicked.set(Some(k));
        }
    }
}

/// A value of a shot that a correlation tile can plot.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Metric {
    BallSpeed,
    ClubSpeed,
    Smash,
    Vla,
    Hla,
    Spin,
    SpinAxis,
    AngleOfAttack,
    Path,
    FaceToTarget,
    FaceToPath,
    Carry,
    Total,
    Side,
    Apex,
    Tempo,
}

impl Metric {
    const ALL: [Self; 16] = [
        Self::BallSpeed,
        Self::ClubSpeed,
        Self::Smash,
        Self::Vla,
        Self::Hla,
        Self::Spin,
        Self::SpinAxis,
        Self::AngleOfAttack,
        Self::Path,
        Self::FaceToTarget,
        Self::FaceToPath,
        Self::Carry,
        Self::Total,
        Self::Side,
        Self::Apex,
        Self::Tempo,
    ];
}

/// A value of a shot in the units shown; `None` where the shot doesn't have it.
fn metric_value(m: Metric, s: &ShotRecord, f: &FlightSummary, u: Units) -> Option<f64> {
    let c = s.club.as_ref();
    Some(match m {
        Metric::BallSpeed => u.speed(s.ball.speed),
        Metric::ClubSpeed => u.speed(c?.speed),
        Metric::Smash => s.smash()?,
        Metric::Vla => s.ball.vla,
        Metric::Hla => s.ball.hla,
        Metric::Spin => s.ball.total_spin,
        Metric::SpinAxis => s.ball.spin_axis,
        Metric::AngleOfAttack => c?.angle_of_attack,
        Metric::Path => c?.path,
        Metric::FaceToTarget => c?.face_to_target,
        Metric::FaceToPath => c?.face_to_target - c?.path,
        Metric::Carry => u.dist(f.carry),
        Metric::Total => u.dist(f.total),
        Metric::Side => u.dist(f.total_side),
        Metric::Apex => u.dist(f.apex),
        Metric::Tempo => s.tempo?.ratio(),
    })
}

/// Name of a value with its unit; estimated ones are marked with *.
fn metric_label(m: Metric, u: Units) -> String {
    let (name, unit) = match m {
        Metric::BallSpeed => (tr("Ball Speed"), u.speed_unit()),
        Metric::ClubSpeed => (tr("Club Speed"), u.speed_unit()),
        Metric::Smash => (tr("Smash Factor"), ""),
        Metric::Vla => ("VLA", "°"),
        Metric::Hla => ("HLA", "°"),
        Metric::Spin => (tr("Total Spin"), "rpm"),
        Metric::SpinAxis => (tr("Spin Axis"), "°"),
        Metric::AngleOfAttack => (tr("Club AoA"), "°"),
        Metric::Path => (tr("Club Path"), "°"),
        Metric::FaceToTarget => (tr("Face to Target"), "°"),
        Metric::FaceToPath => (tr("Face to Path"), "°"),
        Metric::Carry => return format!("{}* ({})", tr("Carry"), u.dist_unit()),
        Metric::Total => return format!("{}* ({})", tr("Total"), u.dist_unit()),
        Metric::Side => return format!("{}* ({})", tr("Side"), u.dist_unit()),
        Metric::Apex => return format!("{}* ({})", tr("Apex"), u.dist_unit()),
        Metric::Tempo => (tr("Tempo"), ""),
    };
    if unit.is_empty() { name.to_string() } else { format!("{name} ({unit})") }
}

/// Whether a value has a sign (left/right, up/down, open/closed …): its chart axis is centred on 0.
fn metric_signed(m: Metric) -> bool {
    matches!(
        m,
        Metric::Hla | Metric::SpinAxis | Metric::AngleOfAttack | Metric::Path | Metric::FaceToTarget | Metric::FaceToPath | Metric::Side
    )
}

/// Range of a chart axis for values from `lo` to `hi`, with some room; values with a sign get a
/// range centred on 0, so the quadrants stay in place.
fn axis_range(lo: f64, hi: f64, signed: bool) -> (f64, f64) {
    if lo > hi {
        return if signed { (-1.0, 1.0) } else { (0.0, 1.0) };
    }
    if signed {
        let m = lo.abs().max(hi.abs()).max(0.5) * 1.1;
        return (-m, m);
    }
    if hi - lo < 1e-9 {
        return (lo - 1.0, hi + 1.0);
    }
    let pad = (hi - lo) * 0.08;
    (lo - pad, hi + pad)
}

/// Decimals a value is shown with.
fn metric_digits(m: Metric) -> usize {
    match m {
        Metric::Smash => 2,
        Metric::Spin | Metric::Carry | Metric::Total | Metric::Side | Metric::Apex => 0,
        _ => 1,
    }
}

/// Correlation coefficient of two values over the shots shown that count.
fn correlation(ctx: &TileCtx, x: Metric, y: Metric) -> Option<f64> {
    let pairs: Vec<(f64, f64)> = ctx
        .shown
        .iter()
        .zip(ctx.flights)
        .filter(|(s, _)| !s.excluded)
        .filter_map(|(s, f)| Some((metric_value(x, s, f, ctx.units)?, metric_value(y, s, f, ctx.units)?)))
        .collect();
    pearson(&pairs)
}

fn pearson(pairs: &[(f64, f64)]) -> Option<f64> {
    if pairs.len() < 3 {
        return None;
    }
    let n = pairs.len() as f64;
    let (mx, my) = pairs.iter().fold((0.0, 0.0), |(a, b), (x, y)| (a + x / n, b + y / n));
    let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
    for (x, y) in pairs {
        sxy += (x - mx) * (y - my);
        sxx += (x - mx) * (x - mx);
        syy += (y - my) * (y - my);
    }
    (sxx > 0.0 && syy > 0.0).then(|| sxy / (sxx * syy).sqrt())
}

/// Two values of the shots against each other, chosen in the tile. Returns the shot clicked.
fn scatter(ui: &mut egui::Ui, i: usize, tile: &mut Tile, ctx: &TileCtx) -> Option<Key> {
    let u = ctx.units;
    let cfg: Config = tile.config();
    let (mut x, mut y) = (cfg.x, cfg.y);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let w = ((ui.available_width() - 36.0) / 2.0).max(40.0);
        for (axis, value) in [("Y", &mut y), ("X", &mut x)] {
            ui.label(RichText::new(axis).size(11.0).color(MUTED));
            // On display only (the tile overlay) the axes are just named.
            if ctx.display {
                ui.label(RichText::new(metric_label(*value, u)).size(11.5).color(SOFT));
                ui.add_space(8.0);
                continue;
            }
            egui::ComboBox::from_id_salt(("scatter", i, axis)).width(w).selected_text(RichText::new(metric_label(*value, u)).size(11.5)).show_ui(ui, |ui| {
                for m in Metric::ALL {
                    ui.selectable_value(value, m, metric_label(m, u));
                }
            });
        }
    });
    if (x, y) != (cfg.x, cfg.y) {
        tile.change_config(|cfg: &mut Config| (cfg.x, cfg.y) = (x, y));
    }
    ui.add_space(4.0);
    let rect = ui.available_rect_before_wrap();
    let values: Vec<Option<(f64, f64)>> = ctx
        .shown
        .iter()
        .zip(ctx.flights)
        .map(|(s, f)| Some((metric_value(x, s, f, u)?, metric_value(y, s, f, u)?)))
        .collect();
    let range = |pick: fn(&(f64, f64)) -> f64, metric: Metric| {
        let (lo, hi) = values.iter().flatten().map(pick).fold((f64::MAX, f64::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)));
        axis_range(lo, hi, metric_signed(metric))
    };
    let ((x0, x1), (y0, y1)) = (range(|v| v.0, x), range(|v| v.1, y));
    let pos = |vx: f64, vy: f64| {
        pos2(
            rect.left() + ((vx - x0) / (x1 - x0)) as f32 * rect.width(),
            rect.bottom() - ((vy - y0) / (y1 - y0)) as f32 * rect.height(),
        )
    };
    let p = ui.painter_at(rect);
    p.rect(rect, 4.0, PAGE, Stroke::new(1.0, LINE), StrokeKind::Inside);
    // Axes through zero split the chart into quadrants (always for values with a sign, centred).
    let axis = Stroke::new(1.5, LABEL.gamma_multiply(0.55));
    if x0 < 0.0 && x1 > 0.0 {
        let zx = pos(0.0, y0).x;
        p.line_segment([pos2(zx, rect.top()), pos2(zx, rect.bottom())], axis);
    }
    if y0 < 0.0 && y1 > 0.0 {
        let zy = pos(x0, 0.0).y;
        p.line_segment([pos2(rect.left(), zy), pos2(rect.right(), zy)], axis);
    }
    let (dx, dy) = (metric_digits(x), metric_digits(y));
    let small = FontId::proportional(10.5);
    p.text(rect.left_bottom() + vec2(4.0, -2.0), Align2::LEFT_BOTTOM, num(x0, dx), small.clone(), MUTED);
    p.text(rect.right_bottom() + vec2(-4.0, -2.0), Align2::RIGHT_BOTTOM, num(x1, dx), small.clone(), MUTED);
    p.text(rect.left_top() + vec2(4.0, 2.0), Align2::LEFT_TOP, num(y1, dy), small, MUTED);
    if values.iter().all(Option::is_none) {
        p.text(rect.center(), Align2::CENTER_CENTER, tr("No shots with these values"), FontId::proportional(12.0), MUTED);
        return None;
    }
    draw_spread(&p, &club_groups(ctx, |n| values[n].map(|(vx, vy)| pos(vx, vy))));
    let dots: Vec<Dot> = ctx
        .shown
        .iter()
        .zip(&values)
        .filter_map(|(s, v)| {
            let (vx, vy) = (*v)?;
            Some(Dot {
                pos: pos(vx, vy),
                color: (ctx.color_of)(&s.club_code),
                selected: ctx.selected.contains(&key(s)),
                excluded: s.excluded,
                key: key(s),
                tip: format!(
                    "{} · {}\n{}: {}\n{}: {}",
                    s.clock(),
                    s.club_code.as_deref().unwrap_or("–"),
                    metric_label(x, u),
                    num(vx, dx),
                    metric_label(y, u),
                    num(vy, dy),
                ),
            })
        })
        .collect();
    draw_dots(ui, egui::Id::new(("scatter", i)), rect, &dots)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_axes_are_centred_on_zero() {
        // Only right of the target: still centred, so 0 is the middle.
        assert_eq!(axis_range(1.0, 4.0, true), (-4.4, 4.4));
        assert_eq!(axis_range(-2.0, 1.0, true), (-2.2, 2.2));
        // Unsigned values keep their own range.
        let (lo, hi) = axis_range(100.0, 200.0, false);
        assert!(lo > 90.0 && hi < 210.0);
        assert_eq!(axis_range(f64::MAX, f64::MIN, true), (-1.0, 1.0), "no values");
    }

    #[test]
    fn correlation_of_two_series() {
        let up: Vec<(f64, f64)> = (0..5).map(|k| (f64::from(k), 2.0 * f64::from(k) + 1.0)).collect();
        assert!((pearson(&up).unwrap() - 1.0).abs() < 1e-9);
        let down: Vec<(f64, f64)> = up.iter().map(|&(x, y)| (x, -y)).collect();
        assert!((pearson(&down).unwrap() + 1.0).abs() < 1e-9);
        assert_eq!(pearson(&up[..2]), None, "too few shots");
        assert_eq!(pearson(&[(1.0, 2.0), (1.0, 3.0), (1.0, 4.0)]), None, "no spread");
    }
}
