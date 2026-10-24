//! The dispersion tile: where the balls land (or stop), from above, with the spread per club.

use serde::{Deserialize, Serialize};

use super::*;

/// The dispersion tile.
pub(super) struct Dispersion;

/// A dispersion tile's settings.
#[derive(Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Config {
    /// Where the ball comes to rest instead of where it lands.
    total: bool,
}

impl TileKind for Dispersion {
    fn id(&self) -> &'static str {
        "dispersion"
    }

    fn name(&self) -> &'static str {
        tr("Dispersion")
    }

    fn options(&self, ui: &mut egui::Ui, tile: &mut Tile, _units: Units) {
        tile.change_config(|cfg: &mut Config| {
            setting_row(ui, tr("Shows"), |ui| {
                segments(ui, &mut cfg.total, &[(false, tr("Carry")), (true, tr("Total"))]);
            });
        });
    }

    fn header(&self, ui: &mut egui::Ui, tile: &mut Tile, _ctx: &TileCtx) {
        tile.change_config(|cfg: &mut Config| {
            for (on, label) in [(true, tr("Total")), (false, tr("Carry"))] {
                let color = if cfg.total == on { WHITE } else { MUTED };
                if link(ui, label, color).clicked() {
                    cfg.total = on;
                }
            }
        });
    }

    fn title_from(&self) -> f32 {
        250.0
    }

    fn body(&self, ui: &mut egui::Ui, tile: &mut Tile, ctx: &TileCtx) {
        if let Some(k) = dispersion(ui, ctx.index, tile.config::<Config>().total, ctx) {
            ctx.clicked.set(Some(k));
        }
    }
}

/// Landing points (or where the balls stopped) from above, on a range. Returns the shot clicked.
fn dispersion(ui: &mut egui::Ui, i: usize, total: bool, ctx: &TileCtx) -> Option<Key> {
    let rect = ui.available_rect_before_wrap();
    let u = ctx.units;
    let at = |f: &FlightSummary| if total { (u.dist(f.total_side), u.dist(f.total)) } else { (u.dist(f.carry_side), u.dist(f.carry)) };
    let pts: Vec<(f64, f64)> = ctx.flights.iter().map(at).collect();
    // Distance lines every 25 (m or yd) around the shots; the width follows the widest shot.
    let step = 25.0;
    let (near, far) = pts.iter().fold((f64::MAX, f64::MIN), |(lo, hi), &(_, d)| (lo.min(d), hi.max(d)));
    let (y0, y1) = if pts.is_empty() { (0.0, 250.0) } else { (((near - 15.0) / step).floor().max(0.0) * step, ((far + 10.0) / step).ceil() * step) };
    let half = pts.iter().fold(20.0_f64, |m, &(x, _)| m.max(x.abs() * 1.25));
    let pos = |x: f64, y: f64| {
        pos2(
            rect.center().x + (x / half) as f32 * rect.width() / 2.0,
            rect.bottom() - ((y - y0) / (y1 - y0)) as f32 * rect.height(),
        )
    };
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 4.0, Color32::from_rgb(0x23, 0x50, 0x2f));
    let fairway = Rect::from_x_y_ranges(rect.center().x - rect.width() / 4.0..=rect.center().x + rect.width() / 4.0, rect.y_range());
    p.rect_filled(fairway, 0.0, Color32::from_rgb(0x2c, 0x5c, 0x37));
    let grass = Color32::from_rgb(0xcf, 0xe3, 0xd4);
    p.line_segment([pos2(rect.center().x, rect.top()), pos2(rect.center().x, rect.bottom())], Stroke::new(1.0, grass.gamma_multiply(0.35)));
    let mut d = y0;
    while d <= y1 {
        let y = pos(0.0, d).y;
        let major = (d / (2.0 * step)).fract() == 0.0;
        p.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], Stroke::new(1.0, if major { Color32::from_rgb(0x5d, 0x8f, 0x6a) } else { Color32::from_rgb(0x3f, 0x6b, 0x4a) }));
        if d > y0 {
            p.text(pos2(rect.left() + 4.0, y - 2.0), Align2::LEFT_BOTTOM, format!("{d:.0} {}", u.dist_unit()), FontId::proportional(10.5), grass);
        }
        d += step;
    }
    draw_spread(&p, &club_groups(ctx, |n| Some(pos(pts[n].0, pts[n].1))));
    let what = if total { tr("Total") } else { tr("Carry") };
    let dots: Vec<Dot> = ctx
        .shown
        .iter()
        .zip(&pts)
        .map(|(s, &(x, y))| Dot {
            pos: pos(x, y),
            color: (ctx.color_of)(&s.club_code),
            selected: ctx.selected.contains(&key(s)),
            excluded: s.excluded,
            key: key(s),
            tip: format!(
                "{} · {}\n{}* {y:.0} {} · {}",
                s.clock(),
                s.club_code.as_deref().unwrap_or("–"),
                what,
                u.dist_unit(),
                side_text(x, u),
            ),
        })
        .collect();
    draw_dots(ui, egui::Id::new(("dispersion", i)), rect, &dots)
}

/// Lateral distance as "12 m R" / "3 yd L" (already in the unit shown).
fn side_text(v: f64, u: Units) -> String {
    if v.abs() < 0.5 { format!("0 {}", u.dist_unit()) } else { format!("{:.0} {} {}", v.abs(), u.dist_unit(), if v > 0.0 { "R" } else { "L" }) }
}
