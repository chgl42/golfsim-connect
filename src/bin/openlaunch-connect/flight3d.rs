//! The 3D flight tile: the shots' estimated flights over a driving range, drawn in perspective
//! with the painter – sky and haze, mown fairway, target greens with flags, distance boards,
//! shadows on the ground, flights that fade with distance, landing rings and bounces.

use super::*;

/// Mean flight per club: points per flight after resampling.
const MEAN_POINTS: usize = 48;

/// A perspective camera looking from `eye` at `at` (metres; x right, y up, z down the range).
pub(super) struct View {
    eye: [f32; 3],
    right: [f32; 3],
    up: [f32; 3],
    forward: [f32; 3],
}

impl View {
    pub(super) fn new(eye: [f32; 3], at: [f32; 3]) -> Self {
        let norm = |v: [f32; 3]| {
            let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-6);
            [v[0] / l, v[1] / l, v[2] / l]
        };
        let cross = |a: [f32; 3], b: [f32; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
        let forward = norm([at[0] - eye[0], at[1] - eye[1], at[2] - eye[2]]);
        let right = norm(cross([0.0, 1.0, 0.0], forward));
        let up = cross(forward, right);
        Self { eye, right, up, forward }
    }

    /// Point on the image plane at distance 1 (x right, y up); `None` behind the camera.
    pub(super) fn project(&self, p: [f32; 3]) -> Option<egui::Vec2> {
        let d = [p[0] - self.eye[0], p[1] - self.eye[1], p[2] - self.eye[2]];
        let dot = |a: [f32; 3]| a[0] * d[0] + a[1] * d[1] + a[2] * d[2];
        let z = dot(self.forward);
        (z > 0.5).then(|| vec2(dot(self.right) / z, dot(self.up) / z))
    }

    /// Distance from the camera along its view.
    fn depth(&self, p: [f32; 3]) -> f32 {
        (p[0] - self.eye[0]) * self.forward[0] + (p[1] - self.eye[1]) * self.forward[1] + (p[2] - self.eye[2]) * self.forward[2]
    }
}

/// A camera: where it is, what it looks at, how much the picture may stretch.
#[derive(Clone, Copy)]
pub(super) struct Cam {
    eye: [f32; 3],
    at: [f32; 3],
    stretch: egui::Vec2,
}

impl Cam {
    /// Turned around `at`: `yaw` sideways, `pitch` up or down (radians), staying above the ground.
    fn orbit(self, yaw: f32, pitch: f32) -> Self {
        let v = [self.eye[0] - self.at[0], self.eye[1] - self.at[1], self.eye[2] - self.at[2]];
        let r = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-3);
        let el = ((v[1] / r).asin() + pitch).clamp(0.03, 1.5);
        let az = v[0].atan2(-v[2]) + yaw;
        let eye = [self.at[0] + r * el.cos() * az.sin(), self.at[1] + r * el.sin(), self.at[2] - r * el.cos() * az.cos()];
        Self { eye, ..self }
    }

    /// Part of the way (`t` from 0 to 1) from here to `to`.
    fn lerp(self, to: Self, t: f32) -> Self {
        let l = |a: [f32; 3], b: [f32; 3]| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
        Self { eye: l(self.eye, to.eye), at: l(self.at, to.at), stretch: self.stretch + (to.stretch - self.stretch) * t }
    }
}

/// A 3D tile's camera as the user left it, and a move between presets under way. Kept for the
/// session.
pub(super) struct TileView {
    yaw: f32,
    pitch: f32,
    zoom: f32,
    camera: Option<Camera>,
    shown: Option<Cam>,
    from: Option<(Cam, std::time::Instant)>,
}

#[cfg(test)]
impl TileView {
    /// Sideways turn, up/down turn and zoom, for tests.
    pub(super) fn orbit_state(&self) -> (f32, f32, f32) {
        (self.yaw, self.pitch, self.zoom)
    }
}

impl Default for TileView {
    fn default() -> Self {
        Self { yaw: 0.0, pitch: 0.0, zoom: 1.0, camera: None, shown: None, from: None }
    }
}

/// Seconds per point of a flight path (the integration step times the thinning, see `FlightPath`).
const PATH_STEP: f32 = 0.08;

/// One flight to draw: through the air, the bounces and roll, and who it belongs to.
struct Track {
    air: Vec<[f32; 3]>,
    rest: [f32; 3],
    color: Color32,
    /// 1 = normal; below for shots in the background (excluded, or behind a selection or the means).
    weight: f32,
    selected: bool,
    shadow: bool,
    /// A club's mean flight: drawn bold.
    bold: bool,
    /// Still in the air (animated): only the part flown so far, with the ball at its head.
    flying: bool,
}

/// The mean flight of each club's shots that count: every flight resampled to the same number of
/// points, then averaged point by point.
fn mean_tracks(ctx: &TileCtx) -> Vec<Track> {
    let mut groups: Vec<(Option<String>, Vec<&FlightPath>)> = Vec::new();
    for (s, path) in ctx.shown.iter().zip(ctx.paths) {
        if s.excluded || path.air.len() < 2 {
            continue;
        }
        match groups.iter_mut().find(|g| g.0 == s.club_code) {
            Some(g) => g.1.push(path),
            None => groups.push((s.club_code.clone(), vec![path])),
        }
    }
    let resample = |air: &[[f32; 3]], t: f32| {
        let x = t * (air.len() - 1) as f32;
        let (k, f) = (x.floor() as usize, x.fract());
        let (a, b) = (air[k], air[(k + 1).min(air.len() - 1)]);
        [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f, a[2] + (b[2] - a[2]) * f]
    };
    groups
        .into_iter()
        .map(|(code, paths)| {
            let n = paths.len() as f32;
            let air = (0..MEAN_POINTS)
                .map(|k| {
                    let t = k as f32 / (MEAN_POINTS - 1) as f32;
                    paths.iter().map(|p| resample(&p.air, t)).fold([0.0; 3], |a, q| [a[0] + q[0] / n, a[1] + q[1] / n, a[2] + q[2] / n])
                })
                .collect();
            let rest = paths.iter().fold([0.0; 3], |a, p| [a[0] + p.rest[0] / n, 0.0, a[2] + p.rest[2] / n]);
            Track { air, rest, color: (ctx.color_of)(&code), weight: 1.0, selected: false, shadow: true, bold: true, flying: false }
        })
        .collect()
}

/// Points of the bounces and roll from the landing point to where the ball stops: two hops,
/// the second a third as high, then rolling.
fn bounces(land: [f32; 3], rest: [f32; 3]) -> Vec<[f32; 3]> {
    let len = ((rest[0] - land[0]).powi(2) + (rest[2] - land[2]).powi(2)).sqrt();
    let h1 = (len * 0.12).min(2.5);
    let at = |t: f32, h: f32| [land[0] + (rest[0] - land[0]) * t, h, land[2] + (rest[2] - land[2]) * t];
    let mut pts = Vec::new();
    for (t0, t1, h) in [(0.0, 0.35, h1), (0.35, 0.55, h1 / 3.0)] {
        for k in 0..=8 {
            let s = k as f32 / 8.0;
            pts.push(at(t0 + (t1 - t0) * s, h * 4.0 * s * (1.0 - s)));
        }
    }
    pts.push(at(1.0, 0.0));
    pts
}

/// Mix `a` towards `b` by `t` (0–1).
fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t.clamp(0.0, 1.0)).round() as u8;
    Color32::from_rgba_unmultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
}

/// The shots' estimated flights in 3D, fitted into the tile. Returns the shot clicked (at its
/// landing point).
pub(super) fn flight_view(ui: &mut egui::Ui, i: usize, tile: &Tile, ctx: &TileCtx, edits: &mut Vec<Edit>) -> Option<Key> {
    let camera = tile.camera;
    let mean = tile.averages;
    let rect = ui.available_rect_before_wrap();
    let u = ctx.units;
    // Range shown: a little beyond the longest shot, at least 100 m; half as wide as long, at most 60 m.
    let far = ctx.paths.iter().map(|p| p.rest[2].max(p.air.last().map_or(0.0, |q| q[2]))).fold(100.0_f32, f32::max) * 1.08;
    let apex = ctx.paths.iter().flat_map(|p| p.air.iter().map(|q| q[1])).fold(10.0_f32, f32::max);
    let half = ctx.paths.iter().map(|p| p.rest[0].abs()).fold(15.0_f32, f32::max).max(far * 0.12).min(60.0) * 1.3;
    // Diagonally from above the range is as wide as a real one, so it covers the whole tile.
    let half = if camera == Camera::Above { half.max(far * 0.45) } else { half };
    // Landing and resting points of the shots shown.
    let lands: Vec<[f32; 3]> = ctx.paths.iter().flat_map(|p| p.air.last().copied().into_iter().chain([p.rest])).collect();
    // The camera of the preset: where it is, what it looks at, and how much the picture may
    // stretch where the tile has room (from the tee the spread is narrow and the flights tall,
    // from the side it is the other way round).
    let preset = match camera {
        Camera::Tee => Cam { eye: [0.0, 2.0, -12.0], at: [0.0, apex * 0.35, far * 0.6], stretch: vec2(3.0, 1.0) },
        // From the right of the range, so the distance grows to the right.
        Camera::Side => Cam { eye: [far * 1.3, apex * 0.6, far * 0.5], at: [0.0, apex * 0.4, far * 0.5], stretch: vec2(1.0, 2.5) },
        // Diagonally from the top right: 45° above the range, turned from behind to the right –
        // 35° in a tall tile up to 65° in a wide one, so the flights run along the tile – onto
        // the middle of the landing area.
        Camera::Above => {
            let aspect = rect.width() / rect.height().max(1.0);
            let turn = 35.0 + 30.0 * ((aspect - 0.6) / 1.4).clamp(0.0, 1.0);
            let zc = if lands.is_empty() { far * 0.6 } else { lands.iter().map(|q| q[2]).sum::<f32>() / lands.len() as f32 };
            let (dist, el, az) = (far * 0.8, 45_f32.to_radians(), turn.to_radians());
            Cam {
                eye: [dist * el.cos() * az.sin(), dist * el.sin(), zc - dist * el.cos() * az.cos()],
                at: [0.0, 0.0, zc],
                stretch: vec2(1.4, 1.4),
            }
        }
    };

    // Dragging turns the camera around what it looks at, scrolling zooms, a double click goes
    // back to the preset; switching presets glides there.
    let resp = ui.interact(rect, egui::Id::new(("flight", i)), Sense::click_and_drag());
    let mut views = ctx.views.borrow_mut();
    let tv = views.entry((ctx.page, i)).or_default();
    if resp.dragged() {
        let d = resp.drag_delta();
        tv.yaw -= d.x * 0.008;
        tv.pitch = (tv.pitch + d.y * 0.006).clamp(-1.2, 1.2);
    }
    if resp.hovered() {
        let scroll = ui.input(|inp| inp.smooth_scroll_delta.y);
        if scroll != 0.0 {
            tv.zoom = (tv.zoom * (scroll * 0.004).exp()).clamp(0.5, 8.0);
        }
    }
    if resp.double_clicked() {
        (tv.yaw, tv.pitch, tv.zoom) = (0.0, 0.0, 1.0);
    }
    if tv.camera != Some(camera) {
        if let (Some(_), Some(shown)) = (tv.camera, tv.shown) {
            tv.from = Some((shown, std::time::Instant::now()));
        }
        (tv.camera, tv.yaw, tv.pitch, tv.zoom) = (Some(camera), 0.0, 0.0, 1.0);
    }
    let target = preset.orbit(tv.yaw, tv.pitch);
    let cam = match tv.from {
        Some((from, start)) => {
            let t = start.elapsed().as_secs_f32() / 0.45;
            if t >= 1.0 {
                tv.from = None;
                target
            } else {
                ui.ctx().request_repaint();
                from.lerp(target, t * t * (3.0 - 2.0 * t))
            }
        }
        None => target,
    };
    tv.shown = Some(cam);
    let zoom = tv.zoom;
    drop(views);
    let view = View::new(cam.eye, cam.at);
    let mut fit: Vec<egui::Vec2> = Vec::new();
    if camera == Camera::Above {
        // Zoomed onto where the balls land and stop, with a little room; the flights come in
        // from outside and the range runs on beyond.
        fit.extend(lands.iter().flat_map(|q| {
            [[q[0] - 6.0, 0.0, q[2] - 6.0], [q[0] + 6.0, 0.0, q[2] + 6.0], [q[0] - 6.0, 0.0, q[2] + 6.0], [q[0] + 6.0, 0.0, q[2] - 6.0]]
        }).filter_map(|q| view.project(q)));
        if fit.is_empty() {
            fit.extend([[0.0, 0.0, 0.0], [0.0, 0.0, far / 1.08]].iter().filter_map(|q| view.project(*q)));
        }
    } else {
        // The flights and the range; the near corners would be right below the camera.
        fit.extend(ctx.paths.iter().flat_map(|p| p.air.iter().chain([&p.rest])).filter_map(|q| view.project(*q)));
        fit.extend([[-half, 0.0, far], [half, 0.0, far], [0.0, apex, far * 0.5], [0.0, 0.0, 6.0]].iter().filter_map(|q| view.project(*q)));
    }
    let (lo, hi) = fit.iter().fold((vec2(f32::MAX, f32::MAX), vec2(f32::MIN, f32::MIN)), |(lo, hi), q| (lo.min(*q), hi.max(*q)));
    let inner = rect.shrink(12.0);
    let (sx, sy) = (inner.width() / (hi.x - lo.x).max(1e-3), inner.height() / (hi.y - lo.y).max(1e-3));
    // Same scale both ways, with the camera's stretch where the tile has room; then the zoom.
    let base = sx.min(sy);
    let scale = vec2(sx.min(base * cam.stretch.x), sy.min(base * cam.stretch.y)) * zoom;
    let mid = (lo + hi) / 2.0;
    let to_screen = |q: [f32; 3]| view.project(q).map(|v| pos2(inner.center().x + (v.x - mid.x) * scale.x, inner.center().y - (v.y - mid.y) * scale.y));
    // Lines get thinner and fainter with distance (1 at the middle of the range).
    let ref_depth = view.depth([0.0, 0.0, far * 0.5]).max(1.0);
    let depth_factor = |q: [f32; 3]| (ref_depth / view.depth(q).max(1.0)).clamp(0.6, 1.25);

    let p = ui.painter_at(rect);
    // Where the flights peak on average: the tee view's height lines stand there.
    let peaks: Vec<f32> = ctx.paths.iter().filter_map(|p| p.air.iter().max_by(|a, b| a[1].total_cmp(&b[1])).map(|q| q[2])).collect();
    let apex_z = if peaks.is_empty() { far * 0.5 } else { peaks.iter().sum::<f32>() / peaks.len() as f32 };
    draw_range(&p, rect, camera, (far, half, apex, apex_z), u, &to_screen);

    // What to draw: the single shots, or only the mean flight per club.
    let any_selected = ctx.shown.iter().any(|s| ctx.selected.contains(&key(s)));
    let mut tracks: Vec<Track> = if mean { mean_tracks(ctx) } else { Vec::new() };
    // A shot being animated flies in real time; the others step back meanwhile.
    let flying = ctx.anim.filter(|(k, t)| {
        !mean && ctx.shown.iter().zip(ctx.paths).any(|(s, p)| &key(s) == *k && *t < p.air.len() as f32 * PATH_STEP)
    });
    if flying.is_some() {
        ui.ctx().request_repaint();
    }
    let singles: Vec<Track> = ctx
        .shown
        .iter()
        .zip(ctx.paths)
        .map(|(s, path)| {
            let selected = ctx.selected.contains(&key(s));
            let this_flying = flying.is_some_and(|(k, _)| k == &key(s));
            let weight = if s.excluded {
                0.25
            } else if flying.is_some() && !this_flying {
                0.35
            } else if any_selected && !selected {
                0.5
            } else {
                1.0
            };
            let air = match flying.filter(|_| this_flying) {
                Some((_, t)) => path.air[..=((t / PATH_STEP) as usize).min(path.air.len() - 1)].to_vec(),
                None => path.air.clone(),
            };
            Track {
                air,
                rest: path.rest,
                color: (ctx.color_of)(&s.club_code),
                weight,
                selected: selected || this_flying,
                shadow: !s.excluded,
                bold: false,
                flying: this_flying,
            }
        })
        .filter(|_| !mean)
        .collect();
    tracks.extend(singles);
    // Background first, the selection on top.
    tracks.sort_by(|a, b| (a.selected, a.weight).partial_cmp(&(b.selected, b.weight)).unwrap_or(std::cmp::Ordering::Equal));

    // Shadows on the ground.
    for t in tracks.iter().filter(|t| t.shadow) {
        let ground: Vec<egui::Pos2> = t.air.iter().filter_map(|q| to_screen([q[0], 0.0, q[2]])).collect();
        p.add(egui::Shape::line(ground, Stroke::new(1.0, Color32::from_black_alpha((50.0 * t.weight) as u8))));
    }
    for t in &tracks {
        draw_track(&p, t, &to_screen, &depth_factor);
    }

    // The selected shot: drop line from the apex, carry and apex next to the flight.
    for ((s, path), f) in ctx.shown.iter().zip(ctx.paths).zip(ctx.flights) {
        if !ctx.selected.contains(&key(s)) || mean {
            continue;
        }
        let Some(top) = path.air.iter().copied().max_by(|a, b| a[1].total_cmp(&b[1])) else { continue };
        if let (Some(a), Some(b)) = (to_screen(top), to_screen([top[0], 0.0, top[2]]))
        {
            p.extend(egui::Shape::dashed_line(&[a, b], Stroke::new(1.0, WHITE.gamma_multiply(0.7)), 3.0, 3.0));
            badge(&p, a + vec2(0.0, -10.0), &format!("{}* {:.0} {}", tr("Apex"), u.dist(f.apex), u.dist_unit()));
        }
        if let Some(land) = path.air.last().and_then(|q| to_screen(*q)) {
            badge(&p, land + vec2(0.0, 14.0), &format!("{}* {:.0} {}", tr("Carry"), u.dist(f.carry), u.dist_unit()));
        }
    }

    // Landing points: hover shows the shot, a click selects it.
    let dots: Vec<Dot> = ctx
        .shown
        .iter()
        .zip(ctx.paths)
        .zip(ctx.flights)
        .filter_map(|((s, path), f)| {
            let land = path.air.last().and_then(|q| to_screen(*q))?;
            Some(Dot {
                pos: land,
                color: (ctx.color_of)(&s.club_code),
                selected: ctx.selected.contains(&key(s)),
                excluded: s.excluded,
                key: key(s),
                tip: format!(
                    "{} · {}\n{}* {:.0} {} · {}* {:.0} {}",
                    s.clock(),
                    s.club_code.as_deref().unwrap_or("–"),
                    tr("Carry"),
                    u.dist(f.carry),
                    u.dist_unit(),
                    tr("Apex"),
                    u.dist(f.apex),
                    u.dist_unit(),
                ),
            })
        })
        .collect();
    // No dots: the landing rings show where the balls came down. None for the means.
    let dots = if mean { Vec::new() } else { dots };
    let clicked = draw_dots_quiet(ui, &resp, rect, &dots);

    // Replay the selected (or latest) shot, bottom left in the view.
    let replay = ctx.shown.iter().rev().find(|s| ctx.selected.contains(&key(s))).or_else(|| ctx.shown.last());
    if let Some(s) = replay.filter(|_| !mean) {
        let r = Rect::from_center_size(pos2(rect.left() + 18.0, rect.bottom() - 18.0), vec2(24.0, 24.0));
        let b = ui.interact(r, egui::Id::new(("flight-replay", i)), Sense::click());
        p.circle(r.center(), 11.0, PAGE.gamma_multiply(0.8), Stroke::new(1.0, if b.hovered() { ICON } else { BUTTON_LINE }));
        let c = r.center() + vec2(1.0, 0.0);
        p.add(egui::Shape::convex_polygon(vec![c + vec2(-3.5, -5.0), c + vec2(5.0, 0.0), c + vec2(-3.5, 5.0)], WHITE, Stroke::NONE));
        if b.on_hover_text(tr("Play the flight again")).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            edits.push(Edit::Replay(key(s)));
        }
    }

    // Shots or means, top right in the view (after the landing points, so it gets the clicks).
    let options = [(false, tr("Shots")), (true, tr("Mean"))];
    let font = FontId::proportional(11.0);
    let widths: Vec<f32> = options.iter().map(|(_, l)| p.layout_no_wrap(l.to_string(), font.clone(), WHITE).size().x + 14.0).collect();
    let total: f32 = widths.iter().sum();
    let pill = Rect::from_min_size(pos2(rect.right() - total - 8.0, rect.top() + 6.0), vec2(total, 20.0));
    p.rect(pill, 10.0, PAGE.gamma_multiply(0.8), Stroke::new(1.0, BUTTON_LINE), StrokeKind::Inside);
    let mut x = pill.left();
    for ((on, label), w) in options.into_iter().zip(widths) {
        let r = Rect::from_min_size(pos2(x, pill.top()), vec2(w, pill.height()));
        let resp = ui.interact(r, egui::Id::new(("flight-mode", i, on)), Sense::click());
        if mean == on {
            p.rect_filled(r.shrink(1.5), 9.0, ACCENT);
        }
        let color = if mean == on { WHITE } else if resp.hovered() { SOFT } else { MUTED };
        p.text(r.center(), Align2::CENTER_CENTER, label, font.clone(), color);
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && mean != on {
            edits.push(Edit::Averages(i, on));
        }
        x += w;
    }
    clicked
}

/// A small dark label with white text, centred on `at`.
fn badge(p: &egui::Painter, at: egui::Pos2, text: &str) {
    let galley = p.layout_no_wrap(text.to_string(), FontId::proportional(10.5), WHITE);
    let r = Rect::from_center_size(at, galley.size() + vec2(10.0, 4.0));
    p.rect_filled(r, 4.0, PAGE.gamma_multiply(0.85));
    p.galley(r.min + vec2(5.0, 2.0), galley, WHITE);
}

/// Hover and click on the landing points without drawing dots (the tracks show them).
fn draw_dots_quiet(ui: &egui::Ui, resp: &egui::Response, rect: Rect, dots: &[Dot]) -> Option<Key> {
    let near = resp.hover_pos().and_then(|at| {
        dots.iter().map(|d| (d, d.pos.distance(at))).filter(|(_, dist)| *dist <= 9.0).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(d, _)| d)
    });
    let clicked = resp.clicked();
    let d = near?;
    ui.painter_at(rect).circle_stroke(d.pos, 6.5, Stroke::new(1.5, WHITE));
    resp.clone().on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(&d.tip);
    clicked.then(|| d.key.clone())
}

/// One flight: a glow under a selected one, colour fading in from the tee, width and opacity by
/// distance, a ring where it lands, then bounces and roll.
fn draw_track(p: &egui::Painter, t: &Track, to_screen: &dyn Fn([f32; 3]) -> Option<egui::Pos2>, depth: &dyn Fn([f32; 3]) -> f32) {
    let n = t.air.len();
    let base_w = if t.bold {
        2.2
    } else if t.selected {
        1.8
    } else if t.weight >= 1.0 {
        1.0
    } else {
        0.8
    };
    let color = if t.selected { WHITE } else { t.color };
    if t.selected {
        let glow: Vec<egui::Pos2> = t.air.iter().filter_map(|q| to_screen(*q)).collect();
        p.add(egui::Shape::line(glow, Stroke::new(4.5, WHITE.gamma_multiply(0.14))));
    }
    // In a few joined pieces (smooth, no gaps at the bends): fainter near the tee, full towards
    // the landing, thinner and fainter far away.
    // (A flight just starting has a single point: nothing to draw yet but the ball.)
    const PIECES: usize = 8;
    for piece in (0..PIECES).filter(|_| n >= 2) {
        let from = piece * (n - 1) / PIECES;
        let to = ((piece + 1) * (n - 1) / PIECES).min(n - 1);
        if to <= from {
            continue;
        }
        let pts: Vec<egui::Pos2> = t.air[from..=to].iter().filter_map(|q| to_screen(*q)).collect();
        let along = (from + to) as f32 / 2.0 / (n - 1) as f32;
        let d = depth(t.air[(from + to) / 2]);
        let alpha = (0.6 + 0.4 * along.powf(0.7)) * t.weight * d.clamp(0.8, 1.1);
        p.add(egui::Shape::line(pts, Stroke::new(base_w * d, color.gamma_multiply(alpha.min(1.0)))));
    }
    let Some(&land) = t.air.last() else { return };
    if t.flying {
        // The ball at the head of its tracer.
        if let Some(head) = to_screen(land) {
            p.circle_filled(head, 7.0, WHITE.gamma_multiply(0.25));
            p.circle_filled(head, 3.2, WHITE);
        }
        return;
    }
    // Landing ring flat on the ground.
    let ring: Vec<egui::Pos2> = (0..=20)
        .filter_map(|k| {
            let a = k as f32 / 20.0 * std::f32::consts::TAU;
            to_screen([land[0] + 1.6 * a.cos(), 0.0, land[2] + 1.6 * a.sin()])
        })
        .collect();
    if ring.len() > 3 {
        p.add(egui::Shape::convex_polygon(ring.clone(), color.gamma_multiply(0.35 * t.weight), Stroke::NONE));
        p.add(egui::Shape::closed_line(ring, Stroke::new(1.0, color.gamma_multiply(0.9 * t.weight))));
    }
    let hops: Vec<egui::Pos2> = bounces(land, t.rest).into_iter().filter_map(to_screen).collect();
    p.add(egui::Shape::line(hops, Stroke::new(base_w * 0.7, color.gamma_multiply(0.75 * t.weight))));
}

/// Sky with haze at the horizon, rough and fairway with mowing stripes, target greens with flags,
/// distance boards and the target line; from the side, height lines every 10 m.
#[allow(clippy::too_many_arguments)]
fn draw_range(
    p: &egui::Painter,
    rect: Rect,
    camera: Camera,
    (far, half, apex, apex_z): (f32, f32, f32, f32),
    u: Units,
    to_screen: &dyn Fn([f32; 3]) -> Option<egui::Pos2>,
) {
    let sky_top = Color32::from_rgb(0x14, 0x26, 0x42);
    let haze = Color32::from_rgb(0x4a, 0x6e, 0x8e);
    let rough = Color32::from_rgb(0x22, 0x47, 0x2c);
    let fairway = Color32::from_rgb(0x2c, 0x5c, 0x37);
    let stripe = Color32::from_rgb(0x33, 0x68, 0x3e);
    let end = far * 1.4;
    let horizon = to_screen([0.0, 0.0, end]).map_or(rect.top(), |h| h.y.clamp(rect.top(), rect.bottom()));

    // Sky: dark at the top, hazy at the horizon.
    let mut sky = egui::Mesh::default();
    let (tl, tr_, bl, br) = (rect.left_top(), rect.right_top(), pos2(rect.left(), horizon), pos2(rect.right(), horizon));
    for (pos, c) in [(tl, sky_top), (tr_, sky_top), (br, haze), (bl, haze)] {
        sky.colored_vertex(pos, c);
    }
    sky.add_triangle(0, 1, 2);
    sky.add_triangle(0, 2, 3);
    p.add(sky);
    p.rect_filled(Rect::from_min_max(pos2(rect.left(), horizon), rect.max), 0.0, rough);

    // Ground, subdivided along the range so near points stay in front of the camera; far away it
    // fades into the haze.
    let band = |x0: f32, x1: f32, z0: f32, z1: f32, color: Color32| {
        let mut pts = Vec::new();
        for k in 0..=6 {
            pts.extend(to_screen([x0, 0.0, z0 + (z1 - z0) * k as f32 / 6.0]));
        }
        for k in (0..=6).rev() {
            pts.extend(to_screen([x1, 0.0, z0 + (z1 - z0) * k as f32 / 6.0]));
        }
        if pts.len() > 3 {
            let fade = (z0 / end).powf(1.5) * 0.65;
            p.add(egui::Shape::convex_polygon(pts, mix(color, haze, fade), Stroke::NONE));
        }
    };
    let step = 10.0;
    let mut z = 0.0;
    let mut k = 0;
    while z < end {
        band(-half * 4.0, half * 4.0, z, z + step, rough);
        // The mown range is as wide as the range shown.
        band(-half, half, z, z + step, if k % 2 == 0 { fairway } else { stripe });
        z += step;
        k += 1;
    }

    // Height lines every 10 m (yd): from the side along the range, from the tee across it where
    // the flights peak.
    let step_h = if u.meters { 10.0 } else { 10.0 / M_TO_YD as f32 };
    let mut n = 1;
    while camera != Camera::Above && step_h * n as f32 <= apex * 1.15 {
        let h = step_h * n as f32;
        let (from, to) = match camera {
            Camera::Tee => ([-half * 1.5, h, apex_z], [half * 1.5, h, apex_z]),
            _ => ([0.0, h, 0.0], [0.0, h, far]),
        };
        if let (Some(a), Some(b)) = (to_screen(from), to_screen(to)) {
            p.line_segment([a, b], Stroke::new(1.0, Color32::from_white_alpha(18)));
            let label = format!("{} {}", 10 * n, u.dist_unit());
            p.text(pos2(a.x.max(rect.left()) + 4.0, a.y - 2.0), Align2::LEFT_BOTTOM, label, FontId::proportional(9.5), Color32::from_white_alpha(110));
        }
        n += 1;
    }

    // Target line from the tee.
    if let (Some(a), Some(b)) = (to_screen([0.0, 0.0, 0.0]), to_screen([0.0, 0.0, end])) {
        p.line_segment([a, b], Stroke::new(1.0, Color32::from_white_alpha(40)));
    }

    // Target greens with a flag every 50 m (yd), left and right of the line in turn, and a board
    // at the edge of the fairway.
    let step_m = if u.meters { 50.0 } else { 50.0 / M_TO_YD as f32 };
    let mut n = 1;
    let mut last_board: Option<egui::Pos2> = None;
    while step_m * n as f32 <= far {
        let dz = step_m * n as f32;
        let gx = if n % 2 == 0 { -half * 0.18 } else { half * 0.14 };
        let r = (4.0 + dz * 0.025).min(9.0);
        let green: Vec<egui::Pos2> = (0..24)
            .filter_map(|k| {
                let a = k as f32 / 24.0 * std::f32::consts::TAU;
                to_screen([gx + r * a.cos(), 0.0, dz + r * 0.8 * a.sin()])
            })
            .collect();
        if green.len() > 3 {
            let fade = (dz / end).powf(1.5) * 0.6;
            p.add(egui::Shape::convex_polygon(green, mix(Color32::from_rgb(0x4c, 0x9a, 0x5a), haze, fade), Stroke::NONE));
        }
        if let (Some(foot), Some(top)) = (to_screen([gx, 0.0, dz]), to_screen([gx, 2.6, dz])) {
            p.line_segment([foot, top], Stroke::new(1.0, Color32::from_white_alpha(200)));
            let len = (foot.y - top.y).abs().clamp(3.0, 14.0);
            let flag = vec![top, top + vec2(len * 0.7, len * 0.18), top + vec2(0.0, len * 0.36)];
            p.add(egui::Shape::convex_polygon(flag, Color32::from_rgb(0xf2, 0x5c, 0x4a), Stroke::NONE));
        }
        let board_at = match camera {
            Camera::Side => to_screen([0.0, 0.0, dz]).map(|q| q + vec2(0.0, 12.0)),
            Camera::Above => to_screen([(far * 0.1).max(18.0), 0.0, dz]),
            Camera::Tee => to_screen([half * 0.55, 0.0, dz]).map(|q| q + vec2(0.0, -6.0)),
        };
        if let Some(at) = board_at.filter(|at| rect.contains(*at))
            // Apart enough; from the tee the boards line up towards the horizon, so only by height.
            && last_board.is_none_or(|l| (l.y - at.y).abs() > 14.0 || (camera != Camera::Tee && (l.x - at.x).abs() > 40.0))
        {
            let text = format!("{}", 50 * n);
            let galley = p.layout_no_wrap(text, FontId::proportional(10.0), WHITE);
            let r = Rect::from_center_size(at, galley.size() + vec2(8.0, 3.0));
            // Kept inside the tile.
            let r = r.translate(vec2((rect.right() - 4.0 - r.right()).min(0.0), 0.0));
            p.rect(r, 2.0, Color32::from_rgb(0x1c, 0x3a, 0x6b), Stroke::new(1.0, Color32::from_white_alpha(120)), StrokeKind::Inside);
            p.galley(r.min + vec2(4.0, 1.5), galley, WHITE);
            last_board = Some(at);
        }
        n += 1;
    }
}
