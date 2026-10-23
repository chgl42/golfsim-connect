//! The 3D flight tile: the shots' estimated flights over a driving range, drawn in perspective
//! with the painter – sky and haze, mown fairway, target greens with flags, distance boards,
//! shadows on the ground, flights that fade with distance, landing rings and bounces.

use std::rc::Rc;

use serde::{Deserialize, Serialize};

use super::*;

/// The 3D flight tile (id `flight`).
pub(super) struct Flight3d;

/// A 3D flight tile's settings.
#[derive(Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Config {
    /// Where the camera looks from.
    camera: Camera,
    /// The mean flight per club instead of the single shots.
    averages: bool,
}

impl TileKind for Flight3d {
    fn id(&self) -> &'static str {
        "flight"
    }

    fn name(&self) -> &'static str {
        tr("3D flight")
    }

    fn options(&self, ui: &mut egui::Ui, tile: &mut Tile, _units: Units) {
        tile.change_config(|cfg: &mut Config| {
            setting_row(ui, tr("Camera"), |ui| {
                segments(ui, &mut cfg.camera, &Camera::ALL.map(|c| (c, camera_name(c))));
            });
            setting_row(ui, tr("Shows"), |ui| {
                segments(ui, &mut cfg.averages, &[(false, tr("Shots")), (true, tr("Mean"))]);
            });
        });
    }

    fn header(&self, ui: &mut egui::Ui, tile: &mut Tile, _ctx: &TileCtx) {
        tile.change_config(|cfg: &mut Config| {
            for camera in Camera::ALL.into_iter().rev() {
                let color = if cfg.camera == camera { WHITE } else { MUTED };
                if link(ui, camera_name(camera), color).clicked() {
                    cfg.camera = camera;
                }
            }
        });
    }

    fn title_from(&self) -> f32 {
        330.0
    }

    fn body(&self, ui: &mut egui::Ui, tile: &mut Tile, ctx: &TileCtx) {
        let paths = paths(ctx);
        if let Some(k) = flight_view(ui, tile, ctx, &paths) {
            ctx.clicked.set(Some(k));
        }
    }
}

/// Camera of a 3D flight tile.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Camera {
    /// Behind the tee, looking down the range.
    #[default]
    Tee,
    /// From the side: height over distance.
    Side,
    /// From above and behind, at an angle.
    Above,
}

impl Camera {
    const ALL: [Self; 3] = [Self::Tee, Self::Side, Self::Above];
}

fn camera_name(camera: Camera) -> &'static str {
    match camera {
        Camera::Tee => tr("Tee"),
        Camera::Side => tr("Side view"),
        Camera::Above => tr("Above"),
    }
}

/// A flight for drawing: points through the air (thinned out) and where the ball comes to rest.
struct FlightPath {
    air: Vec<[f32; 3]>,
    rest: [f32; 3],
}

impl FlightPath {
    /// The estimated path bent onto GSPro's numbers `to` (from the estimate `from`): as far, as
    /// high and landing and stopping where GSPro says.
    fn fitted(&self, from: &FlightSummary, to: &FlightSummary) -> Self {
        let carry = from.carry.max(1.0) as f32;
        let kz = to.carry as f32 / carry;
        let ky = to.apex as f32 / from.apex.max(0.5) as f32;
        let shift = (to.carry_side - from.carry_side) as f32;
        let air = self.air.iter().map(|q| [q[0] + shift * (q[2] / carry), q[1] * ky, q[2] * kz]).collect();
        Self { air, rest: [to.total_side as f32, 0.0, to.total as f32] }
    }
}

impl From<&Flight> for FlightPath {
    fn from(f: &Flight) -> Self {
        let last = f.path.len().saturating_sub(1);
        let air = f
            .path
            .iter()
            .enumerate()
            .filter(|(k, _)| k % 4 == 0 || *k == last)
            .map(|(_, p)| [p[0] as f32, p[1] as f32, p[2] as f32])
            .collect();
        Self { air, rest: [f.total_side as f32, 0.0, f.total as f32] }
    }
}

/// The estimated path and flight of every shot seen, computed once (for all 3D tiles).
#[derive(Default)]
struct Paths(HashMap<Key, (Rc<FlightPath>, FlightSummary)>);

/// The paths of the shots shown, in their order: bent onto GSPro's numbers where those are shown.
fn paths(ctx: &TileCtx) -> Vec<Rc<FlightPath>> {
    let mut cache = ctx.shared::<Paths>();
    ctx.shown
        .iter()
        .zip(ctx.flights)
        .map(|(s, f)| {
            let (path, estimate) = cache.0.entry(key(s)).or_insert_with(|| {
                let fl = flight::fly(&s.ball);
                (Rc::new(FlightPath::from(&fl)), FlightSummary::from(&fl))
            });
            if ctx.gspro && s.gspro.is_some() { Rc::new(path.fitted(estimate, f)) } else { path.clone() }
        })
        .collect()
}

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

    /// A point in camera space: right, up, forward (metres).
    fn camera_space(&self, p: [f32; 3]) -> [f32; 3] {
        let d = [p[0] - self.eye[0], p[1] - self.eye[1], p[2] - self.eye[2]];
        let dot = |a: [f32; 3]| a[0] * d[0] + a[1] * d[1] + a[2] * d[2];
        [dot(self.right), dot(self.up), dot(self.forward)]
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

/// From the camera's image plane to the tile: a fixed origin and scale, so turning the camera
/// moves the picture but never re-fits it.
struct Screen {
    view: View,
    origin: egui::Pos2,
    scale: egui::Vec2,
}

impl Screen {
    /// Nearest distance in front of the camera that is drawn.
    const NEAR: f32 = 0.5;

    fn plane(&self, v: egui::Vec2) -> egui::Pos2 {
        pos2(self.origin.x + v.x * self.scale.x, self.origin.y - v.y * self.scale.y)
    }

    fn point(&self, q: [f32; 3]) -> Option<egui::Pos2> {
        self.view.project(q).map(|v| self.plane(v))
    }

    /// A flat shape on the screen, cut where it passes behind the camera (so ground near and
    /// behind the camera never folds over).
    fn polygon(&self, pts: &[[f32; 3]]) -> Vec<egui::Pos2> {
        let cam: Vec<[f32; 3]> = pts.iter().map(|q| self.view.camera_space(*q)).collect();
        let mut clipped: Vec<[f32; 3]> = Vec::new();
        for k in 0..cam.len() {
            let (a, b) = (cam[k], cam[(k + 1) % cam.len()]);
            let (ina, inb) = (a[2] >= Self::NEAR, b[2] >= Self::NEAR);
            if ina {
                clipped.push(a);
            }
            if ina != inb {
                let t = (Self::NEAR - a[2]) / (b[2] - a[2]);
                clipped.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, Self::NEAR]);
            }
        }
        clipped.iter().map(|c| self.plane(vec2(c[0] / c[2], c[1] / c[2]))).collect()
    }

    /// Height of the horizon on the screen (the camera never rolls, so it is level).
    fn horizon_y(&self) -> f32 {
        let f = self.view.forward;
        let l = (f[0] * f[0] + f[2] * f[2]).sqrt().max(1e-6);
        let e = self.view.eye;
        self.point([e[0] + f[0] / l * 1e6, 0.0, e[2] + f[2] / l * 1e6]).map_or(f32::MIN, |q| q.y)
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
    from: Option<(Cam, Instant)>,
}

#[cfg(test)]
impl TileView {
    /// Sideways turn, up/down turn and zoom, for tests.
    pub(super) fn orbit_state(&self) -> (f32, f32, f32) {
        (self.yaw, self.pitch, self.zoom)
    }

    /// A camera turned by hand, for screenshots.
    pub(super) fn turned(yaw: f32, pitch: f32, zoom: f32) -> Self {
        Self { yaw, pitch, zoom, ..Self::default() }
    }
}

impl Default for TileView {
    fn default() -> Self {
        Self { yaw: 0.0, pitch: 0.0, zoom: 1.0, camera: None, shown: None, from: None }
    }
}

/// Seconds per point of a flight path (the integration step times the thinning, see `FlightPath`).
const PATH_STEP: f32 = 0.08;
/// Seconds the animated ball takes for its bounces and roll.
const ROLL_TIME: f32 = 1.6;

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
    /// Animated: seconds since the shot; the part flown (and rolled) so far is drawn, with the ball.
    ball: Option<f32>,
}

/// The mean flight of each club's shots that count: every flight resampled to the same number of
/// points, then averaged point by point.
fn mean_tracks(ctx: &TileCtx, paths: &[Rc<FlightPath>]) -> Vec<Track> {
    let mut groups: Vec<(Option<String>, Vec<&FlightPath>)> = Vec::new();
    for (s, path) in ctx.shown.iter().zip(paths) {
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
            Track { air, rest, color: (ctx.color_of)(&code), weight: 1.0, selected: false, shadow: true, bold: true, ball: None }
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
fn flight_view(ui: &mut egui::Ui, tile: &mut Tile, ctx: &TileCtx, paths: &[Rc<FlightPath>]) -> Option<Key> {
    let Config { camera, averages: mean } = tile.config();
    let i = ctx.index;
    let rect = ui.available_rect_before_wrap();
    let u = ctx.units;
    // Range shown: a little beyond the longest shot, at least 100 m; half as wide as long, at most 60 m.
    let far = paths.iter().map(|p| p.rest[2].max(p.air.last().map_or(0.0, |q| q[2]))).fold(100.0_f32, f32::max) * 1.08;
    let apex = paths.iter().flat_map(|p| p.air.iter().map(|q| q[1])).fold(10.0_f32, f32::max);
    let half = paths.iter().map(|p| p.rest[0].abs()).fold(15.0_f32, f32::max).max(far * 0.12).min(60.0) * 1.3;
    // Diagonally from above the range is as wide as a real one, so it covers the whole tile.
    let half = if camera == Camera::Above { half.max(far * 0.45) } else { half };
    // Landing and resting points of the shots shown.
    let lands: Vec<[f32; 3]> = paths.iter().flat_map(|p| p.air.last().copied().into_iter().chain([p.rest])).collect();
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
    let mut tv = ctx.state::<TileView>();
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
    // A different preset glides there and starts from it unturned.
    if tv.camera.is_some_and(|c| c != camera) {
        if let Some(shown) = tv.shown {
            tv.from = Some((shown, Instant::now()));
        }
        (tv.yaw, tv.pitch, tv.zoom) = (0.0, 0.0, 1.0);
    }
    tv.camera = Some(camera);
    let target = preset;
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
    let (zoom, yaw, pitch) = (tv.zoom, tv.yaw, tv.pitch);
    drop(tv);
    // The picture is fitted with the camera as preset; turning it keeps that fit.
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
        fit.extend(paths.iter().flat_map(|p| p.air.iter().chain([&p.rest])).filter_map(|q| view.project(*q)));
        fit.extend([[-half, 0.0, far], [half, 0.0, far], [0.0, apex, far * 0.5], [0.0, 0.0, 6.0]].iter().filter_map(|q| view.project(*q)));
    }
    let (lo, hi) = fit.iter().fold((vec2(f32::MAX, f32::MAX), vec2(f32::MIN, f32::MIN)), |(lo, hi), q| (lo.min(*q), hi.max(*q)));
    let inner = rect.shrink(12.0);
    let (sx, sy) = (inner.width() / (hi.x - lo.x).max(1e-3), inner.height() / (hi.y - lo.y).max(1e-3));
    // Same scale both ways, with the camera's stretch where the tile has room. Turned away from
    // the preset the stretch eases out, so the perspective stays true; then the zoom.
    let base = sx.min(sy);
    let stretched = vec2(sx.min(base * cam.stretch.x), sy.min(base * cam.stretch.y));
    let turned = ((yaw.abs() + pitch.abs()) / 0.3).clamp(0.0, 1.0);
    let scale = stretched + (vec2(base, base) - stretched) * turned;
    let mid = (lo + hi) / 2.0;
    let c = inner.center();
    let origin = pos2(c.x - mid.x * scale.x, c.y + mid.y * scale.y);
    let screen = Screen { view: View::new(cam.orbit(yaw, pitch).eye, cam.at), origin: c + (origin - c) * zoom, scale: scale * zoom };
    let view = &screen.view;
    let to_screen = |q: [f32; 3]| screen.point(q);
    // Lines get thinner and fainter with distance (1 at the middle of the range).
    let ref_depth = view.depth([0.0, 0.0, far * 0.5]).max(1.0);
    let depth_factor = |q: [f32; 3]| (ref_depth / view.depth(q).max(1.0)).clamp(0.6, 1.25);

    let p = ui.painter_at(rect);
    // Where the flights peak on average: the tee view's height lines stand there.
    let peaks: Vec<f32> = paths.iter().filter_map(|p| p.air.iter().max_by(|a, b| a[1].total_cmp(&b[1])).map(|q| q[2])).collect();
    let apex_z = if peaks.is_empty() { far * 0.5 } else { peaks.iter().sum::<f32>() / peaks.len() as f32 };
    draw_range(&p, rect, camera, (far, half, apex, apex_z), u, &screen);

    // What to draw: the single shots, or only the mean flight per club.
    let any_selected = ctx.shown.iter().any(|s| ctx.selected.contains(&key(s)));
    let mut tracks: Vec<Track> = if mean { mean_tracks(ctx, paths) } else { Vec::new() };
    // A shot being animated flies in real time; the others step back meanwhile.
    let flying = ctx.anim.filter(|(k, t)| {
        !mean && ctx.shown.iter().zip(paths).any(|(s, p)| &key(s) == *k && *t < p.air.len() as f32 * PATH_STEP + ROLL_TIME)
    });
    if flying.is_some() {
        ui.ctx().request_repaint();
    }
    let singles: Vec<Track> = ctx
        .shown
        .iter()
        .zip(paths)
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
            Track {
                air: path.air.clone(),
                rest: path.rest,
                color: (ctx.color_of)(&s.club_code),
                weight,
                selected: selected || this_flying,
                shadow: !s.excluded,
                bold: false,
                ball: flying.filter(|_| this_flying).map(|(_, t)| t),
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
        draw_track(&p, t, &screen, &depth_factor);
    }

    // The selected shot: drop line from the apex, carry and apex next to the flight.
    for ((s, path), f) in ctx.shown.iter().zip(paths).zip(ctx.flights) {
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
        .zip(paths)
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
    if let Some(s) = replay.filter(|_| !mean && !ctx.display) {
        let r = Rect::from_center_size(pos2(rect.left() + 18.0, rect.bottom() - 18.0), vec2(24.0, 24.0));
        let b = ui.interact(r, egui::Id::new(("flight-replay", i)), Sense::click());
        p.circle(r.center(), 11.0, PAGE.gamma_multiply(0.8), Stroke::new(1.0, if b.hovered() { ICON } else { BUTTON_LINE }));
        let c = r.center() + vec2(1.0, 0.0);
        p.add(egui::Shape::convex_polygon(vec![c + vec2(-3.5, -5.0), c + vec2(5.0, 0.0), c + vec2(-3.5, 5.0)], WHITE, Stroke::NONE));
        if b.on_hover_text(tr("Play the flight again")).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            ctx.replay(s);
        }
    }

    // Shots or means, top right in the view (after the landing points, so it gets the clicks).
    if ctx.display {
        return clicked;
    }
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
            tile.change_config(|cfg: &mut Config| cfg.averages = on);
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
fn draw_track(p: &egui::Painter, t: &Track, screen: &Screen, depth: &dyn Fn([f32; 3]) -> f32) {
    let to_screen = |q: [f32; 3]| screen.point(q);
    // Animated: in the air only the part flown so far; then the bounces and roll bit by bit.
    let air_time = t.air.len() as f32 * PATH_STEP;
    let air: &[[f32; 3]] = match t.ball {
        Some(s) if s < air_time => &t.air[..=((s / PATH_STEP) as usize).min(t.air.len().saturating_sub(1))],
        _ => &t.air,
    };
    let n = air.len();
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
        let glow: Vec<egui::Pos2> = air.iter().filter_map(|q| to_screen(*q)).collect();
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
        let pts: Vec<egui::Pos2> = air[from..=to].iter().filter_map(|q| to_screen(*q)).collect();
        let along = (from + to) as f32 / 2.0 / (n - 1) as f32;
        let d = depth(air[usize::midpoint(from, to)]);
        let alpha = (0.6 + 0.4 * along.powf(0.7)) * t.weight * d.clamp(0.8, 1.1);
        p.add(egui::Shape::line(pts, Stroke::new(base_w * d, color.gamma_multiply(alpha.min(1.0)))));
    }
    let ball = |at: [f32; 3]| {
        if let Some(head) = to_screen(at) {
            p.circle_filled(head, 7.0, WHITE.gamma_multiply(0.25));
            p.circle_filled(head, 3.2, WHITE);
        }
    };
    let Some(&head) = air.last() else { return };
    if t.ball.is_some_and(|s| s < air_time) {
        ball(head);
        return;
    }
    // Landing ring flat on the ground.
    let ring: Vec<[f32; 3]> = (0..20)
        .map(|k| {
            let a = k as f32 / 20.0 * std::f32::consts::TAU;
            [head[0] + 1.6 * a.cos(), 0.0, head[2] + 1.6 * a.sin()]
        })
        .collect();
    let ring = screen.polygon(&ring);
    if ring.len() > 3 {
        p.add(egui::Shape::convex_polygon(ring.clone(), color.gamma_multiply(0.35 * t.weight), Stroke::NONE));
        p.add(egui::Shape::closed_line(ring, Stroke::new(1.0, color.gamma_multiply(0.9 * t.weight))));
    }
    let hops3 = bounces(head, t.rest);
    // Rolling out: quick at first, slowing down to where it stops.
    let shown = match t.ball {
        Some(s) => {
            let f = ((s - air_time) / ROLL_TIME).clamp(0.0, 1.0);
            let f = 1.0 - (1.0 - f) * (1.0 - f);
            ((f * (hops3.len() - 1) as f32).round() as usize).min(hops3.len() - 1)
        }
        None => hops3.len() - 1,
    };
    let hops: Vec<egui::Pos2> = hops3[..=shown].iter().copied().filter_map(to_screen).collect();
    p.add(egui::Shape::line(hops, Stroke::new(base_w * 0.7, color.gamma_multiply(0.75 * t.weight))));
    if t.ball.is_some() {
        ball(hops3[shown]);
    }
}

/// Sky with haze at the horizon, rough and fairway with mowing stripes, target greens with flags,
/// distance boards and the target line; from the side, height lines every 10 m.
fn draw_range(
    p: &egui::Painter,
    rect: Rect,
    camera: Camera,
    (far, half, apex, apex_z): (f32, f32, f32, f32),
    u: Units,
    screen: &Screen,
) {
    let to_screen = |q: [f32; 3]| screen.point(q);
    let sky_top = Color32::from_rgb(0x14, 0x26, 0x42);
    let haze = Color32::from_rgb(0x4a, 0x6e, 0x8e);
    let rough = Color32::from_rgb(0x22, 0x47, 0x2c);
    let fairway = Color32::from_rgb(0x2c, 0x5c, 0x37);
    let stripe = Color32::from_rgb(0x33, 0x68, 0x3e);
    let end = far * 1.4;
    let horizon = screen.horizon_y().clamp(rect.top(), rect.bottom());

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

    // Ground in bands, cut where they pass behind the camera; far away it fades into the haze.
    let band = |x0: f32, x1: f32, z0: f32, z1: f32, color: Color32| {
        let pts = screen.polygon(&[[x0, 0.0, z0], [x1, 0.0, z0], [x1, 0.0, z1], [x0, 0.0, z1]]);
        if pts.len() > 2 {
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
        let green: Vec<[f32; 3]> = (0..24)
            .map(|k| {
                let a = k as f32 / 24.0 * std::f32::consts::TAU;
                [gx + r * a.cos(), 0.0, dz + r * 0.8 * a.sin()]
            })
            .collect();
        let green = screen.polygon(&green);
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
