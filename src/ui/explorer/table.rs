//! The table tile: the shots, latest first, or the averages per club.

use serde::{Deserialize, Serialize};

use crate::history::Stat;

use super::*;

const ROW_H: f32 = 21.0;

/// The table tile.
pub(super) struct Table;

/// A table tile's settings.
#[derive(Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Config {
    /// Averages per club instead of single shots.
    averages: bool,
}

impl TileKind for Table {
    fn id(&self) -> &'static str {
        "table"
    }

    fn name(&self) -> &'static str {
        tr("Table")
    }

    fn options(&self, ui: &mut egui::Ui, tile: &mut Tile, _units: Units) {
        tile.change_config(|cfg: &mut Config| {
            setting_row(ui, tr("Shows"), |ui| {
                segments(ui, &mut cfg.averages, &[(false, tr("Shots")), (true, tr("Averages"))]);
            });
        });
    }

    fn header(&self, ui: &mut egui::Ui, tile: &mut Tile, _ctx: &TileCtx) {
        tile.change_config(|cfg: &mut Config| {
            for (on, label) in [(true, tr("Averages")), (false, tr("Shots"))] {
                let color = if cfg.averages == on { WHITE } else { MUTED };
                if link(ui, label, color).clicked() {
                    cfg.averages = on;
                }
            }
        });
    }

    fn title_from(&self) -> f32 {
        250.0
    }

    fn body(&self, ui: &mut egui::Ui, tile: &mut Tile, ctx: &TileCtx) {
        if tile.config::<Config>().averages {
            averages_table(ui, ctx.index, ctx);
        } else if let Some(k) = shots_table(ui, ctx.index, ctx) {
            ctx.clicked.set(Some(k));
        }
    }
}

/// One row of a table tile.
struct Row {
    cells: Vec<String>,
    color: Option<Color32>,
    selected: bool,
    excluded: bool,
    /// Not delivered to GSPro: the first cell in amber.
    warn: bool,
}

/// Lateral distance for a table: "8 R", "3 L" or "0" (already in the unit shown).
fn left_right(v: f64) -> String {
    if v.abs() < 0.5 { "0".into() } else { format!("{:.0} {}", v.abs(), if v > 0.0 { "R" } else { "L" }) }
}

/// The shots, latest first; wide tiles show every column. Returns the shot clicked.
fn shots_table(ui: &mut egui::Ui, i: usize, ctx: &TileCtx) -> Option<Key> {
    let u = ctx.units;
    let carry = format!("{}*", tr("Carry"));
    let total = format!("{}*", tr("Total"));
    // Header, width of the values and priority (lower first) of every column.
    let side = format!("{}*", tr("Side"));
    let all: [(&str, f32, u8); 15] = [
        (tr("Time"), if ctx.all_time { 84.0 } else { 52.0 }, 0),
        (tr("Club"), 24.0, 0),
        (&carry, 28.0, 0),
        (&total, 28.0, 3),
        (&side, 34.0, 2),
        (tr("Ball"), 36.0, 1),
        ("VLA", 30.0, 1),
        ("HLA", 30.0, 4),
        (tr("Spin"), 34.0, 1),
        (tr("Axis"), 30.0, 5),
        (tr("Club"), 36.0, 6),
        (tr("Smash"), 30.0, 2),
        (tr("Face to Path"), 40.0, 7),
        (tr("Tempo"), 24.0, 8),
        (tr("Shape"), 66.0, 3),
    ];
    let keep = fit_columns(ui, &all);
    let cols: Vec<(&str, f32)> = keep.iter().map(|&k| (all[k].0, all[k].1)).collect();
    let shown: Vec<&ShotRecord> = ctx.shown.iter().rev().copied().collect();
    let row = |s: &ShotRecord| {
        let f = history::Distances::of(s, ctx.gspro);
        let c = s.club.as_ref();
        let time = if ctx.all_time { s.time.get(5..16).unwrap_or(&s.time).replace('T', " ") } else { s.clock().to_string() };
        let cells = [
            time,
            s.club_code.clone().unwrap_or_else(|| "–".into()),
            format!("{:.0}", u.dist(f.carry)),
            format!("{:.0}", u.dist(f.total)),
            left_right(u.dist(f.carry_side)),
            format!("{:.1}", u.speed(s.ball.speed)),
            num(s.ball.vla, 1),
            num(s.ball.hla, 1),
            format!("{:.0}", s.ball.total_spin),
            num(s.ball.spin_axis, 1),
            c.map_or("–".into(), |c| format!("{:.1}", u.speed(c.speed))),
            s.smash().map_or("–".into(), |v| format!("{v:.2}")),
            c.map_or("–".into(), |c| dir(c.face_to_target - c.path, "O", "C")),
            s.tempo.map_or("–".into(), |t| format!("{:.1}", t.ratio())),
            shape_name(Shape::of(s.ball.hla, s.ball.spin_axis, false)),
        ];
        Row {
            cells: keep.iter().map(|&k| cells[k].clone()).collect(),
            color: Some((ctx.color_of)(&s.club_code)),
            selected: ctx.selected.contains(&key(s)),
            excluded: s.excluded,
            warn: !s.delivered,
        }
    };
    if shown.is_empty() {
        ui.label(RichText::new(no_shots(ctx)).color(MUTED));
        return None;
    }
    table(ui, egui::Id::new(("shots-table", i)), &cols, shown.len(), |r| row(shown[r])).map(|r| key(shown[r]))
}

/// Mean ± spread per club of the shots shown (excluded shots left out).
fn averages_table(ui: &mut egui::Ui, i: usize, ctx: &TileCtx) {
    let u = ctx.units;
    let shots: Vec<ShotRecord> = ctx.shown.iter().map(|s| (*s).clone()).collect();
    let stats = history::club_stats_with(&shots, ctx.gspro);
    if stats.is_empty() {
        ui.label(RichText::new(no_shots(ctx)).color(MUTED));
        return;
    }
    // The spread only from two shots on.
    let fmt = |st: Option<Stat>, digits: usize, f: &dyn Fn(f64) -> f64, n: usize| {
        st.map_or_else(
            || "–".to_string(),
            |st| if n > 1 { format!("{} ±{}", num(f(st.mean), digits), num(f(st.sd), digits)) } else { num(f(st.mean), digits) },
        )
    };
    let id = |v: f64| v;
    let speed = |v: f64| u.speed(v);
    let dist = |v: f64| u.dist(v);
    let carry = format!("{}* ({})", tr("Carry"), u.dist_unit());
    let total = format!("{}* ({})", tr("Total"), u.dist_unit());
    let side = format!("{}* ({})", tr("Side"), u.dist_unit());
    let width = format!("{}* ({})", tr("Width"), u.dist_unit());
    let ball = format!("{} ({})", tr("Ball"), u.speed_unit());
    let club = format!("{} ({})", tr("Club"), u.speed_unit());
    let all: [(&str, f32, u8); 14] = [
        (tr("Club"), 90.0, 0),
        (tr("Shots"), 20.0, 0),
        (&carry, 50.0, 0),
        (&total, 50.0, 2),
        (&side, 56.0, 1),
        (&width, 30.0, 1),
        (&ball, 70.0, 1),
        (&club, 70.0, 4),
        (tr("Smash"), 70.0, 3),
        ("VLA", 56.0, 3),
        (tr("Spin"), 70.0, 3),
        ("HLA", 56.0, 5),
        (tr("Face to Path"), 56.0, 6),
        (tr("Tempo"), 50.0, 7),
    ];
    let keep = fit_columns(ui, &all);
    let cols: Vec<(&str, f32)> = keep.iter().map(|&k| (all[k].0, all[k].1)).collect();
    table(ui, egui::Id::new(("averages-table", i)), &cols, stats.len(), |r| {
        let c = &stats[r];
        let cells = [
            c.club_code.as_deref().map_or(tr("Unknown").to_string(), club_name),
            c.shots.to_string(),
            fmt(c.carry, 0, &dist, c.shots),
            fmt(c.total, 0, &dist, c.shots),
            c.side.map_or("–".into(), |st| {
                let mean = left_right(u.dist(st.mean));
                if c.shots > 1 { format!("{mean} ±{:.0}", u.dist(st.sd)) } else { mean }
            }),
            c.side_width.map_or("–".into(), |w| format!("{:.0}", u.dist(w))),
            fmt(c.ball_speed, 1, &speed, c.shots),
            fmt(c.club_speed, 1, &speed, c.shots),
            fmt(c.smash, 2, &id, c.shots),
            fmt(c.vla, 1, &id, c.shots),
            fmt(c.total_spin, 0, &id, c.shots),
            fmt(c.hla, 1, &id, c.shots),
            fmt(c.face_to_path, 1, &id, c.shots),
            fmt(c.tempo, 1, &id, c.shots),
        ];
        Row {
            cells: keep.iter().map(|&k| cells[k].clone()).collect(),
            color: Some((ctx.color_of)(&c.club_code)),
            selected: false,
            excluded: false,
            warn: false,
        }
    });
}

/// Columns that fit the width, taken by priority (lower first) and kept in their order.
fn fit_columns(ui: &egui::Ui, all: &[(&str, f32, u8)]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..all.len()).collect();
    order.sort_by_key(|&k| all[k].2);
    let mut keep: Vec<usize> = Vec::new();
    let mut width = DOT_W;
    for k in order {
        let w = column_width(ui, all[k].0, all[k].1);
        // The first priority always stays.
        if width + w > ui.available_width() && all[k].2 > 0 {
            continue;
        }
        width += w;
        keep.push(k);
    }
    keep.sort_unstable();
    keep
}

/// A table with a fixed header and clickable rows (drawn only while visible). Column widths are
/// shared out by weight. Returns the row clicked.
fn table(ui: &mut egui::Ui, id: egui::Id, cols: &[(&str, f32)], rows: usize, mut row: impl FnMut(usize) -> Row) -> Option<usize> {
    let width = ui.available_width();
    // Each column as wide as its header or values, the rest of the width shared out evenly.
    let widths: Vec<f32> = cols.iter().map(|&(name, min)| column_width(ui, name, min)).collect();
    let spare = ((width - columns_width(ui, cols)) / cols.len() as f32).max(0.0);
    let xs: Vec<f32> = widths
        .iter()
        .scan(DOT_W, |x, w| {
            let at = *x;
            *x += w + spare;
            Some(at)
        })
        .collect();
    let font = FontId::proportional(12.5);
    let (head, _) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::hover());
    for ((name, _), x) in cols.iter().zip(&xs) {
        let r = ui.painter().text(pos2(head.left() + x, head.center().y), Align2::LEFT_CENTER, *name, FontId::proportional(12.0), MUTED);
        if name.contains('*') {
            ui.interact(r, ui.id().with(("estimated", *name)), Sense::hover())
                .on_hover_text(tr("Estimated from the launch data (no wind, sea level); GSPro may show other numbers"));
        }
    }
    let mut clicked = None;
    egui::ScrollArea::vertical().id_salt(id).auto_shrink([false, false]).show_rows(ui, ROW_H, rows, |ui, range| {
        for r in range {
            let data = row(r);
            let (rect, resp) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::click());
            let p = ui.painter();
            if data.selected {
                p.rect_filled(rect, 3.0, BUTTON_HOVER);
            } else if resp.hovered() {
                p.rect_filled(rect, 3.0, TILE_HOVER);
            } else if r % 2 == 1 {
                p.rect_filled(rect, 0.0, Color32::from_white_alpha(4));
            }
            if let Some(c) = data.color {
                p.circle_filled(pos2(rect.left() + 6.0, rect.center().y), 3.5, c);
            }
            let color = if data.excluded { MUTED } else { SOFT };
            for (n, (text, x)) in data.cells.iter().zip(&xs).enumerate() {
                let c = if n == 0 && data.warn { AMBER } else { color };
                let r = p.text(pos2(rect.left() + x, rect.center().y), Align2::LEFT_CENTER, text, font.clone(), c);
                if data.excluded {
                    p.line_segment([pos2(r.left(), r.center().y), pos2(r.right(), r.center().y)], Stroke::new(1.0, MUTED));
                }
            }
            if data.warn {
                resp.clone().on_hover_text(tr("Not delivered to GSPro"));
            }
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                clicked = Some(r);
            }
        }
    });
    clicked
}

/// Room for the club colour dot left of the first column.
const DOT_W: f32 = 14.0;

/// Width of a table column: its header or its values, whichever is wider, plus a gap.
fn column_width(ui: &egui::Ui, name: &str, min: f32) -> f32 {
    let head = ui.painter().layout_no_wrap(name.to_string(), FontId::proportional(12.0), MUTED).size().x;
    head.max(min) + 12.0
}

/// Width a table needs for `cols`.
fn columns_width(ui: &egui::Ui, cols: &[(&str, f32)]) -> f32 {
    DOT_W + cols.iter().map(|&(name, min)| column_width(ui, name, min)).sum::<f32>()
}
