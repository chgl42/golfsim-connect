//! The details tile: all values of one shot.

use super::*;

/// The details tile: the shot clicked last, or the latest.
pub(super) struct Details;

impl TileKind for Details {
    fn id(&self) -> &'static str {
        "details"
    }

    fn name(&self) -> &'static str {
        tr("Details")
    }

    fn body(&self, ui: &mut egui::Ui, _tile: &mut Tile, ctx: &TileCtx) {
        details(ui, ctx.focus, ctx);
    }
}

/// All values of one shot: the one clicked last, or the latest.
fn details(ui: &mut egui::Ui, shot: Option<&ShotRecord>, ctx: &TileCtx) {
    let Some(s) = shot else {
        ui.label(RichText::new(no_shots(ctx)).color(MUTED));
        return;
    };
    let u = ctx.units;
    let estimate = flight::fly(&s.ball);
    let gspro = s.gspro.as_ref().filter(|_| ctx.gspro);
    let f = history::Distances::new(&estimate, gspro);
    let descent = gspro.map_or(estimate.descent, |g| g.descent);
    let c = s.club.as_ref();
    let deg = |v: f64| format!("{}°", num(v, 1));
    let side = |v: f64| {
        let d = u.dist(v);
        if d.abs() < 0.5 { format!("0 {}", u.dist_unit()) } else { format!("{:.0} {} {}", d.abs(), u.dist_unit(), if d > 0.0 { "R" } else { "L" }) }
    };
    // Estimated values are marked with *, GSPro's aren't.
    let mark = |name: &str| if f.from_gspro { name.to_string() } else { format!("{name}*") };
    let mut values: Vec<(String, String)> = vec![
        (mark(tr("Carry")), format!("{:.0} {}", u.dist(f.carry), u.dist_unit())),
        (mark(tr("Total")), format!("{:.0} {}", u.dist(f.total), u.dist_unit())),
        (mark(tr("Side")), side(f.carry_side)),
        (mark(tr("Apex")), format!("{:.0} {}", u.dist(f.apex), u.dist_unit())),
        (mark(tr("Descent")), format!("{descent:.0}°")),
        (tr("Ball Speed").into(), format!("{:.1} {}", u.speed(s.ball.speed), u.speed_unit())),
        ("VLA".into(), deg(s.ball.vla)),
        ("HLA".into(), deg(s.ball.hla)),
        (tr("Total Spin").into(), format!("{:.0} rpm", s.ball.total_spin)),
        (tr("Spin Axis").into(), deg(s.ball.spin_axis)),
        (tr("Shape").into(), shape_name(Shape::of(s.ball.hla, s.ball.spin_axis, false))),
    ];
    if let Some(c) = c {
        values.extend([
            (tr("Club Speed").into(), format!("{:.1} {}", u.speed(c.speed), u.speed_unit())),
            (tr("Smash Factor").into(), s.smash().map_or("–".into(), |v| format!("{v:.2}"))),
            (tr("Club AoA").into(), dir(c.angle_of_attack, "U", "D")),
            (tr("Club Path").into(), dir(c.path, "I-O", "O-I")),
            (tr("Face to Target").into(), dir(c.face_to_target, "O", "C")),
            (tr("Face to Path").into(), dir(c.face_to_target - c.path, "O", "C")),
        ]);
    }
    if let Some(t) = s.tempo {
        values.push((tr("Tempo").into(), format!("{:.1} : 1", t.ratio())));
    }
    // The tile's width, before the header row could widen the layout.
    let width = ui.available_width();
    let name = s.club_code.as_deref().map_or(tr("Unknown").to_string(), club_name);
    let time = if ctx.all_time { s.time.get(..16).unwrap_or(&s.time).replace('T', " ") } else { s.clock().to_string() };
    ui.horizontal(|ui| {
        let (dot, _) = ui.allocate_exact_size(vec2(10.0, 16.0), Sense::hover());
        ui.painter().circle_filled(dot.center(), 4.0, (ctx.color_of)(&s.club_code));
        ui.label(RichText::new(name).size(13.0).color(WHITE));
        ui.label(RichText::new(time).size(12.0).color(MUTED));
        if s.excluded {
            let label = ui.label(RichText::new(tr("excluded")).size(12.0).color(AMBER));
            if let Some(m) = s.misread {
                label.on_hover_text(tr("Left out on its own: possible misread ({r}). Count takes it back.").replace("{r}", misread_name(m)));
            }
        }
        if f.from_gspro {
            ui.label(RichText::new(tr("from GSPro")).size(12.0).color(GREEN))
                .on_hover_text(tr("Carry, total, apex, side and landing angle as GSPro computed them"));
        }
    });
    ui.add_space(2.0);
    // Two columns when there is room.
    let columns = if width >= 360.0 { 2 } else { 1 };
    let per = values.len().div_ceil(columns);
    egui::ScrollArea::vertical().id_salt(("details", s.shot_number)).auto_shrink([false, false]).max_width(width).show(ui, |ui| {
        // Keep the values clear of the scroll bar on the right.
        let col_w = (width - 12.0 - (columns - 1) as f32 * 18.0) / columns as f32;
        let (rect, _) = ui.allocate_exact_size(vec2(width, per as f32 * 18.0), Sense::hover());
        let p = ui.painter();
        for (n, (k, v)) in values.iter().enumerate() {
            let x = rect.left() + (n / per) as f32 * (col_w + 18.0);
            let y = rect.top() + (n % per) as f32 * 18.0 + 9.0;
            p.text(pos2(x, y), Align2::LEFT_CENTER, k, FontId::proportional(12.5), LABEL);
            p.text(pos2(x + col_w, y), Align2::RIGHT_CENTER, v, FontId::proportional(12.5), WHITE);
        }
    });
}
