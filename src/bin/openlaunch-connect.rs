//! OpenLaunch Connect — desktop app around the R10 → GSPro bridge,
//! styled after GSPro's in-game interface.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontFamily, FontId, Margin, Rect, RichText, Sense, Stroke, StrokeKind,
    pos2, vec2,
};
use openlaunch_connect::engine;
use openlaunch_connect::gspro::Cmd;
use openlaunch_connect::history::{self, ExportFormat, ShotRecord};
use openlaunch_connect::i18n::{self, Language, tr, tr_detail};
use openlaunch_connect::launcher;
use openlaunch_connect::num;
use openlaunch_connect::log;
use openlaunch_connect::misread::Misread;
use openlaunch_connect::settings::{Arrangement, DEFAULT_ARRANGEMENT, OverlayItem, OverlayPos, OverlayStyle, Settings, TileOverlay};
use openlaunch_connect::shape::{Curve, Shape};
use openlaunch_connect::state::{self, R10Status};
use openlaunch_connect::update;

// Next to this file (the crate root of this binary is src/bin/).
#[path = "openlaunch-connect/explorer.rs"]
mod explorer;

const APP_NAME: &str = "OpenLaunch Connect";
const MPH_TO_KMH: f64 = 1.609_344;
const M_TO_YD: f64 = 1.093_613;
const OVERLAY_TITLE: &str = "OpenLaunch Overlay";
const TILE_OVERLAY_TITLE: &str = "OpenLaunch Tile Overlay";
/// Size the layout is designed for. Larger windows zoom the whole UI uniformly, or, with a fixed
/// interface size, give the pages more room.
const BASE_SIZE: egui::Vec2 = vec2(1000.0, 470.0);
const OVERLAY_SIZE: egui::Vec2 = vec2(400.0, 72.0);
/// Distance of the overlay from the screen edges.
const OVERLAY_MARGIN: f32 = 14.0;
/// Transparent room around the overlay's panel inside its window.
const OVERLAY_BORDER: f32 = 2.0;

// GSPro in-game palette: deep navy chrome, slightly lighter stat tiles, blue accent.
const PAGE: Color32 = Color32::from_rgb(0x0b, 0x12, 0x20);
const BAR: Color32 = Color32::from_rgb(0x10, 0x1a, 0x2d);
const TILE: Color32 = Color32::from_rgb(0x16, 0x21, 0x3a);
const TILE_HOVER: Color32 = Color32::from_rgb(0x1d, 0x2b, 0x4a);
const LINE: Color32 = Color32::from_rgb(0x22, 0x31, 0x4f);
const WHITE: Color32 = Color32::from_rgb(0xff, 0xff, 0xff);
const SOFT: Color32 = Color32::from_rgb(0xd6, 0xde, 0xee);
const MUTED: Color32 = Color32::from_rgb(0x7f, 0x8f, 0xb0);
const ACCENT: Color32 = Color32::from_rgb(0x3d, 0x6f, 0xe6);
const GREEN: Color32 = Color32::from_rgb(0x3d, 0xdc, 0x6a);
const AMBER: Color32 = Color32::from_rgb(0xf2, 0xb3, 0x3d);
const RED: Color32 = Color32::from_rgb(0xef, 0x5b, 0x5b);
// Controls: raised buttons, dark input fields, row labels.
const BUTTON: Color32 = Color32::from_rgb(0x22, 0x33, 0x5a);
const BUTTON_HOVER: Color32 = Color32::from_rgb(0x2a, 0x3f, 0x6c);
const BUTTON_LINE: Color32 = Color32::from_rgb(0x3d, 0x55, 0x85);
const ACCENT_HOVER: Color32 = Color32::from_rgb(0x52, 0x81, 0xf0);
const ICON: Color32 = Color32::from_rgb(0x7f, 0xa6, 0xff);
const FIELD_LINE: Color32 = Color32::from_rgb(0x2b, 0x3d, 0x63);
const LABEL: Color32 = Color32::from_rgb(0x9f, 0xb0, 0xd0);

fn cond(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("cond".into()))
}

/// DIN Condensed's capitals sit about a quarter of the font size above the middle of the
/// text box (measured in the rendered screenshots).
const CAPS_OFFSET: f32 = 0.25;

/// Draw condensed `text` with its capitals vertically centred on `pos.y`
/// (`h`: horizontal alignment). Use this whenever a figure or label sits in a box.
fn caps_text(p: &egui::Painter, pos: egui::Pos2, h: egui::Align, text: impl ToString, size: f32, color: Color32) -> Rect {
    p.text(pos + vec2(0.0, size * CAPS_OFFSET), Align2([h, egui::Align::Center]), text, cond(size), color)
}

fn main() -> eframe::Result {
    Settings::migrate_data_dir();
    let settings = Settings::load();
    i18n::set(settings.language);
    openlaunch_connect::notify::ENABLED.store(settings.notifications, std::sync::atomic::Ordering::Relaxed);
    log::info(&format!("{APP_NAME} {} started", env!("CARGO_PKG_VERSION")));
    let tx = engine::start(settings.to_config());
    openlaunch_connect::gspro::DEBUG.store(settings.debug, std::sync::atomic::Ordering::Relaxed);

    if settings.check_updates && std::env::var_os("OPENLAUNCH_DEMO").is_none() {
        update::check_in_background();
    }

    if settings.auto_start_gspro {
        if let Some(path) = settings.launcher_path.clone() {
            let addr = settings.gspro_addr.clone();
            thread::spawn(move || start_gspro(&path, &addr));
        } else {
            log::info("Auto-start is on, but no GSPro launcher is selected (Settings).");
        }
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_NAME)
            // Resizable: the UI zooms to fit or has a fixed size (see `apply_window_zoom`).
            .with_inner_size(start_size(settings.window_size, settings.ui_scale))
            .with_min_inner_size(min_size(settings.ui_scale))
            // Without this, eframe replaces the bundle icon with its default one in the Dock and Cmd-Tab.
            .with_icon(app_icon())
            // Needed for the overlay's see-through corners; the main window paints its own background.
            .with_transparent(true),
        ..Default::default()
    };
    eframe::run_native(
        APP_NAME,
        options,
        Box::new(move |cc| {
            setup_style(&cc.egui_ctx);
            Ok(Box::new(App::new(settings, tx)))
        }),
    )
}

fn app_icon() -> Arc<egui::IconData> {
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../../packaging/icon.png")).unwrap_or_default();
    Arc::new(icon)
}

/// Start GSPro unless it is already running. Blocking; call from a thread.
fn start_gspro(path: &std::path::Path, addr: &str) {
    if launcher::gspro_running(addr) {
        log::info("GSPro is already running – not starting it again.");
        return;
    }
    log::info(&format!("Starting GSPro: {}", path.display()));
    match launcher::launch(path) {
        Ok(()) => log::info("GSPro launcher opened – click »Play« there."),
        Err(e) => log::info(&format!("Could not start GSPro: {e}")),
    }
}

fn load_first(paths: &[&str]) -> Option<Vec<u8>> {
    paths.iter().find_map(|p| std::fs::read(p).ok())
}

fn setup_style(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let proportional = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();

    // Condensed bold for figures and headings (GSPro uses a condensed sans).
    let mut cond_family = Vec::new();
    if let Some(bytes) = load_first(&[
        "/System/Library/Fonts/Supplemental/DIN Condensed Bold.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSansCondensed-Bold.ttf",
    ]) {
        fonts.font_data.insert("cond".into(), Arc::new(egui::FontData::from_owned(bytes)));
        cond_family.push("cond".to_string());
    }
    cond_family.extend(proportional.iter().cloned());
    fonts.families.insert(FontFamily::Name("cond".into()), cond_family);

    // Clean sans for body text.
    if let Some(bytes) = load_first(&["/System/Library/Fonts/Supplemental/Arial.ttf", "/Library/Fonts/Arial.ttf"]) {
        fonts.font_data.insert("body".into(), Arc::new(egui::FontData::from_owned(bytes)));
        if let Some(list) = fonts.families.get_mut(&FontFamily::Proportional) {
            list.insert(0, "body".into());
        }
    }
    ctx.set_fonts(fonts);

    let mut v = egui::Visuals::dark();
    v.panel_fill = PAGE;
    v.window_fill = BAR;
    v.extreme_bg_color = TILE;
    v.override_text_color = Some(SOFT);
    v.selection.bg_fill = ACCENT.gamma_multiply(0.6);
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.widgets.inactive.weak_bg_fill = TILE;
    v.widgets.inactive.bg_fill = TILE;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, LINE);
    v.widgets.hovered.weak_bg_fill = TILE_HOVER;
    v.widgets.hovered.bg_fill = TILE_HOVER;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    v.widgets.active.weak_bg_fill = ACCENT.gamma_multiply(0.5);
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active] {
        w.corner_radius = CornerRadius::same(4);
    }
    ctx.set_visuals(v);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Play,
    Shots,
    Log,
    Overlay,
    Settings,
}

struct App {
    tx: Sender<Cmd>,
    saved: Settings,
    edit: Settings,
    note: Option<String>,
    page: Page,
    capture: Capture,
    /// Open the overlay window (off for offscreen screenshots, where it would be embedded).
    show_overlay: bool,
    /// Remember the window size in the settings (off for offscreen screenshots).
    remember_size: bool,
    /// When the window size started to differ from the saved one (saved once it is stable).
    size_changed: Option<Instant>,
    /// Whether the window's aspect ratio is fixed (not in full screen, where macOS would leave
    /// full screen again; `None` until the window exists).
    aspect_locked: Option<bool>,
    /// Interface size whose minimum window size was last applied (`None` until then).
    applied_scale: Option<Option<f32>>,
    /// Shots page: show all recorded shots instead of this session.
    all_time: bool,
    /// Cached contents of the history file (loaded when "All time" is chosen).
    all_shots: Vec<ShotRecord>,
    /// Session length when `all_shots` was loaded (new shots reload it).
    all_shots_session: usize,
    export_note: Option<String>,
    /// Where and how large the overlay window was first created (native points); later changes
    /// go through AppKit.
    overlay_first: Option<(egui::Pos2, egui::Vec2)>,
    /// The main window's screen size when last known: macOS doesn't tell it while the window is
    /// on no visible Space (e.g. GSPro in full screen on that screen), and the overlays must keep
    /// being drawn then, or egui closes them.
    screen_size: Option<egui::Vec2>,
    /// Every window of the app is covered: the overlays are only kept open (see `App::logic`).
    all_covered: bool,
    /// The overlay's size as measured in its own window (last frame).
    overlay_base: std::cell::Cell<egui::Vec2>,
    /// Where the overlay preview's tiles were drawn (last frame).
    overlay_tile_rects: std::cell::RefCell<Vec<(usize, usize, Rect)>>,
    /// Settings page: asking before the shots view is reset.
    confirm_reset: bool,
    /// Shots page: page shown, filter and selection.
    explorer: explorer::ExplorerState,
    /// Sample shot shown in the overlay preview.
    preview_shot: state::LastShot,
    /// Overlay page: the tile overlay edited (else the status overlay).
    overlay_tab: Option<usize>,
    /// Sample shots for the tile overlay's preview while the session has none.
    sample_shots: std::cell::OnceCell<Vec<ShotRecord>>,
    /// Where the status overlay is on the screen this frame (native points), if shown.
    status_rect: Option<Rect>,
    /// Like `overlay_first`, for the tile overlays.
    tile_overlay_first: HashMap<usize, (egui::Pos2, egui::Vec2)>,
    /// An overlay being moved by hand.
    moving: Option<Moving>,
    /// Whether clicks were last set to go through the status (`None`) and the tile overlays.
    passthrough: HashMap<Option<usize>, bool>,
    /// Asking before arrangement `n` is deleted.
    confirm_delete_arrangement: Option<usize>,
}

/// An overlay being moved by hand: it takes the mouse until it is dropped.
#[derive(Default)]
struct Moving {
    /// The tile overlay (else the status overlay).
    tile: Option<usize>,
    /// Where the mouse and the overlay were when the drag started (native points).
    grab: Option<(egui::Pos2, egui::Pos2)>,
    /// How far it has been dragged where the mouse can't be read from the system.
    dragged: egui::Vec2,
    /// Where it is now (native points, top left).
    at: Option<egui::Pos2>,
}

impl App {
    fn new(mut settings: Settings, tx: Sender<Cmd>) -> Self {
        // The overlays as they are become the first arrangement.
        if settings.arrangements.is_empty() {
            settings.keep_arrangement();
        }
        let mut edit = settings.clone();
        if edit.launcher_path.is_none() {
            edit.launcher_path = launcher::find_gspro_launchers().into_iter().next();
        }
        // OPENLAUNCH_PAGE=settings|log opens another page first (used for README screenshots).
        let page = match std::env::var("OPENLAUNCH_PAGE").as_deref() {
            Ok("settings") => Page::Settings,
            Ok("log") => Page::Log,
            Ok("shots") => Page::Shots,
            Ok("overlay") => Page::Overlay,
            _ => Page::Play,
        };
        Self {
            tx,
            saved: settings,
            edit,
            note: None,
            page,
            capture: Capture::from_env(),
            show_overlay: true,
            remember_size: true,
            size_changed: None,
            aspect_locked: None,
            applied_scale: None,
            all_time: false,
            all_shots: Vec::new(),
            all_shots_session: 0,
            export_note: None,
            overlay_first: None,
            screen_size: None,
            all_covered: false,
            overlay_base: std::cell::Cell::new(egui::Vec2::ZERO),
            overlay_tile_rects: Default::default(),
            confirm_reset: false,
            explorer: explorer::ExplorerState::default(),
            preview_shot: {
                let (ball, club) = engine::test_shot();
                state::LastShot {
                    ball,
                    club: Some(club),
                    tempo: Some(openlaunch_connect::mapping::Tempo { backswing: 0.81, downswing: 0.27 }),
                    at: Instant::now(),
                    delivered: true,
                    shot_number: 0,
                    misread: None,
                }
            },
            overlay_tab: None,
            sample_shots: std::cell::OnceCell::new(),
            status_rect: None,
            tile_overlay_first: HashMap::new(),
            moving: None,
            passthrough: HashMap::new(),
            confirm_delete_arrangement: None,
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::hover());
        let p = ui.painter();
        p.rect_filled(rect, 0.0, BAR);
        p.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, LINE));

        // Wordmark: muted "OPEN" + "LAUNCH" + blue "CONNECT" pill, capitals on the bar's centre line.
        let y = rect.center().y;
        let open = caps_text(p, pos2(rect.left() + 18.0, y), egui::Align::Min, "OPEN", 28.0, MUTED);
        let launch = caps_text(p, pos2(open.right() + 1.0, y), egui::Align::Min, "LAUNCH", 28.0, WHITE);
        let galley = p.layout_no_wrap("CONNECT".into(), cond(22.0), WHITE);
        let pill = Rect::from_min_size(pos2(launch.right() + 6.0, y - 15.0), vec2(galley.size().x + 18.0, 30.0));
        p.rect_filled(pill, 8.0, ACCENT);
        let caps_y = pill.center().y - galley.size().y / 2.0 + 22.0 * CAPS_OFFSET;
        p.galley(pos2(pill.left() + 9.0, caps_y), galley, WHITE);

        // Navigation, right-aligned.
        let mut x = rect.right() - 18.0;
        for (page, label) in
            [(Page::Settings, tr("SETTINGS")), (Page::Overlay, tr("OVERLAY")), (Page::Log, tr("LOG")), (Page::Shots, tr("SHOTS")), (Page::Play, tr("PLAY"))]
        {
            let galley = ui.painter().layout_no_wrap(label.into(), cond(19.0), WHITE);
            let w = galley.size().x;
            let hit = Rect::from_min_max(pos2(x - w - 6.0, rect.top()), pos2(x + 6.0, rect.bottom()));
            let resp = ui.interact(hit, ui.id().with(label), Sense::click());
            let active = self.page == page;
            let color = if active || resp.hovered() { WHITE } else { MUTED };
            let p = ui.painter();
            caps_text(p, pos2(x, y), egui::Align::Max, label, 19.0, color);
            if active {
                p.line_segment([pos2(x - w, y + 16.0), pos2(x, y + 16.0)], Stroke::new(2.0, ACCENT));
            }
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                self.page = page;
            }
            x -= w + 28.0;
        }

        // A newer release: pill left of the navigation, opens the release page.
        if let Some(release) = state::with(|s| s.update.clone()) {
            let label = tr("UPDATE {v}").replace("{v}", &release.version);
            let text_w = ui.painter().layout_no_wrap(label.clone(), cond(16.0), WHITE).size().x;
            let pill = Rect::from_min_max(pos2(x + 8.0 - text_w - 24.0, y - 13.0), pos2(x + 8.0, y + 13.0));
            let resp = ui.interact(pill, ui.id().with("update"), Sense::click());
            let p = ui.painter();
            p.rect_filled(pill, 13.0, if resp.hovered() { ACCENT_HOVER } else { ACCENT });
            caps_text(p, pill.center(), egui::Align::Center, label, 16.0, WHITE);
            if resp
                .on_hover_text(tr("A newer version is available – click to open the release page"))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                ui.ctx().open_url(egui::OpenUrl::new_tab(&release.url));
            }
        }
    }

    fn play(&mut self, ui: &mut egui::Ui) {
        let (last, club, handed, r10, r10_detail, gs_ok, gs_detail, shots, battery) = state::with(|s| {
            (
                s.last_shot.clone(),
                s.gspro_club.clone(),
                s.gspro_handed.clone(),
                s.r10,
                s.r10_detail.clone(),
                s.gspro_connected,
                s.gspro_detail.clone(),
                s.shots_sent,
                s.r10_battery,
            )
        });
        let b = last.as_ref().map(|s| s.ball);
        let c = last.as_ref().and_then(|s| s.club);
        let metric = self.saved.metric;
        let speed = |mph: f64| if metric { mph * MPH_TO_KMH } else { mph };
        let unit = if metric { "km/h" } else { "mph" };

        let head = section(ui, tr("BALL"));
        // With more than one arrangement of the overlays: which one, to switch.
        if self.saved.arrangements.len() > 1 {
            let text = format!("{}: {}", tr("Overlay"), self.arrangement_name(self.saved.arrangement));
            let galley = ui.painter().layout_no_wrap(text, FontId::proportional(13.0), SOFT);
            let r = Rect::from_min_size(pos2(ui.max_rect().right() - galley.size().x - 16.0, head.rect.center().y - galley.size().y / 2.0 - 2.0), galley.size() + vec2(16.0, 0.0));
            let resp = ui.interact(r, ui.id().with("play-arrangement"), Sense::click());
            let color = if resp.hovered() { WHITE } else { SOFT };
            ui.painter().galley_with_override_text_color(r.min, galley, color);
            let c = pos2(r.right() - 5.0, r.center().y);
            ui.painter().add(egui::Shape::convex_polygon(vec![c + vec2(-4.0, -2.0), c + vec2(4.0, -2.0), c + vec2(0.0, 3.0)], color, Stroke::NONE));
            let resp = resp.on_hover_text(tr("Arrangement of the overlays")).on_hover_cursor(egui::CursorIcon::PointingHand);
            self.arrangement_menu(&resp, false);
        }
        tile_row(
            ui,
            [
                (b.map(|b| format!("{:.1}", speed(b.speed))), format!("{} ({unit})", tr("Ball Speed"))),
                (b.map(|b| num(b.vla, 1)), "VLA".into()),
                (b.map(|b| num(b.hla, 1)), "HLA".into()),
                (b.map(|b| format!("{:.0}", b.total_spin)), tr("Total Spin").into()),
                (b.map(|b| num(b.spin_axis, 1)), tr("Spin Axis").into()),
                (b.map(|b| num(b.side_spin, 0)), tr("Side Spin").into()),
            ],
        );
        ui.add_space(10.0);
        section(ui, tr("CLUB"));
        tile_row(
            ui,
            [
                (c.map(|c| format!("{:.1}", speed(c.speed))), format!("{} ({unit})", tr("Club Speed"))),
                (c.map(|c| dir(c.angle_of_attack, "U", "D")), tr("Club AoA").into()),
                (c.map(|c| dir(c.path, "I-O", "O-I")), tr("Club Path").into()),
                (c.map(|c| dir(c.face_to_target, "O", "C")), tr("Face to Target").into()),
                (c.map(|c| dir(c.face_to_target - c.path, "O", "C")), tr("Face to Path").into()),
                (b.and_then(|b| history::smash(&b, c.as_ref())).map(|v| format!("{v:.2}")), tr("Smash Factor").into()),
            ],
        );
        ui.add_space(12.0);

        // Bottom row: club/hand · buttons · last shot · tempo · status, all as tall as the row.
        let h = 108.0;
        let gap = 14.0;
        let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
        let half = h / 2.0 - 1.5;

        // Club + hand (as in GSPro's bottom-left corner).
        let club_r = Rect::from_min_size(row.min, vec2(104.0, half));
        let hand_r = Rect::from_min_size(pos2(row.left(), club_r.bottom() + 3.0), vec2(104.0, half));
        let p = ui.painter();
        p.rect_filled(club_r, 0.0, TILE);
        p.text(
            club_r.center(),
            Align2::CENTER_CENTER,
            club.as_deref().map_or("–".to_string(), club_name),
            FontId::proportional(16.0),
            WHITE,
        );
        p.rect_filled(hand_r, 0.0, TILE);
        let hand = match handed.as_deref() {
            Some("LH") => tr("LEFT"),
            Some(_) => tr("RIGHT"),
            None => "–",
        };
        // "RIGHT" over "Hand", the pair centred in the box.
        caps_text(p, hand_r.center() - vec2(0.0, 8.25), egui::Align::Center, hand, 24.0, WHITE);
        p.text(hand_r.center() + vec2(0.0, 12.75), Align2::CENTER_CENTER, tr("Hand"), FontId::proportional(12.0), SOFT);

        // Buttons, stacked like the club/hand boxes.
        let b1 = Rect::from_min_size(pos2(club_r.right() + gap, row.top()), vec2(200.0, half));
        let b2 = Rect::from_min_size(pos2(b1.left(), b1.bottom() + 3.0), vec2(200.0, half));
        if button(ui, b1, tr("TEST SHOT IN 3 S"), Icon::Clock, false, gs_ok, tr("Connect to GSPro first")) {
            log::info("Test shot in 3 s – switch to the GSPro window now …");
            let tx = self.tx.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_secs(3));
                let (ball, club) = engine::test_shot();
                log::info(&format!(
                    "Test shot: {:.1} mph, VLA {:.1}°, HLA {:.1}°, spin {:.0}",
                    ball.speed, ball.vla, ball.hla, ball.total_spin
                ));
                let _ = tx.send(Cmd::Shot { ball, club: Some(club), tempo: None, test: true });
            });
        }
        let launcher = self.saved.launcher_path.clone().or_else(|| self.edit.launcher_path.clone());
        let why = if gs_ok { tr("GSPro is already connected") } else { tr("No GSPro launcher selected (Settings)") };
        if button(ui, b2, tr("START GSPRO"), Icon::Play, false, launcher.is_some() && !gs_ok, why)
            && let Some(path) = launcher
        {
            let addr = self.saved.gspro_addr.clone();
            thread::spawn(move || start_gspro(&path, &addr));
        }

        // Status panel (like GSPro's tee panel), right.
        let st = Rect::from_min_max(pos2(row.right() - 210.0, row.top()), row.max);
        // Tempo tile left of it, last-shot tile in the remaining space.
        let tempo_r = Rect::from_min_max(pos2(st.left() - gap - 160.0, row.top()), pos2(st.left() - gap, row.bottom()));
        let last_r = Rect::from_min_max(pos2(b1.right() + gap, row.top()), pos2(tempo_r.left() - gap, row.bottom()));
        let p = ui.painter();

        p.rect(last_r, 0.0, TILE, Stroke::new(1.0, LINE), StrokeKind::Inside);
        caps_text(p, pos2(last_r.left() + 14.0, last_r.top() + 18.0), egui::Align::Min, tr("LAST SHOT"), 15.0, MUTED);
        match &last {
            Some(s) => {
                let secs = s.at.elapsed().as_secs();
                let ago = if secs < 60 {
                    tr("{n} s ago").replace("{n}", &secs.to_string())
                } else {
                    tr("{n} min ago").replace("{n}", &(secs / 60).to_string())
                };
                let ago_r = caps_text(p, pos2(last_r.left() + 14.0, last_r.center().y), egui::Align::Min, ago, 30.0, WHITE);
                p.text(
                    pos2(ago_r.right() + 10.0, last_r.center().y + 4.0),
                    Align2::LEFT_CENTER,
                    tr("{n} sent").replace("{n}", &shots.to_string()),
                    FontId::proportional(12.0),
                    MUTED,
                );
                let (dot, what) = match (s.delivered, s.misread) {
                    (false, _) => (AMBER, tr("NOT delivered to GSPro").to_string()),
                    (true, Some(m)) => (AMBER, tr("Misread? ({r})").replace("{r}", misread_name(m))),
                    (true, None) => (GREEN, tr("delivered to GSPro").to_string()),
                };
                let y = last_r.bottom() - 18.0;
                p.circle_filled(pos2(last_r.left() + 18.0, y), 4.0, dot);
                p.text(pos2(last_r.left() + 30.0, y), Align2::LEFT_CENTER, what, FontId::proportional(13.0), SOFT);
            }
            None => {
                caps_text(p, pos2(last_r.left() + 14.0, last_r.center().y), egui::Align::Min, "–", 30.0, MUTED);
                p.text(
                    pos2(last_r.left() + 14.0, last_r.bottom() - 18.0),
                    Align2::LEFT_CENTER,
                    tr("No shot yet"),
                    FontId::proportional(13.0),
                    MUTED,
                );
            }
        }

        let tempo = last.as_ref().and_then(|s| s.tempo);
        p.rect(tempo_r, 0.0, TILE, Stroke::new(1.0, LINE), StrokeKind::Inside);
        let value = tempo.map_or_else(|| "–".to_string(), |t| format!("{:.1} : 1", t.ratio()));
        caps_text(p, pos2(tempo_r.center().x, tempo_r.top() + 40.0), egui::Align::Center, value, 38.0, if tempo.is_some() { WHITE } else { MUTED });
        p.text(pos2(tempo_r.center().x, tempo_r.top() + 72.0), Align2::CENTER_CENTER, tr("Tempo"), FontId::proportional(13.0), SOFT);
        if let Some(t) = tempo {
            p.text(
                pos2(tempo_r.center().x, tempo_r.top() + 90.0),
                Align2::CENTER_CENTER,
                format!("{:.2} s ↑ · {:.2} s ↓", t.backswing, t.downswing),
                FontId::proportional(11.0),
                MUTED,
            );
        }
        ui.interact(tempo_r, ui.id().with("tempo"), Sense::hover())
            .on_hover_text(tr("Backswing : downswing (↑ backswing, ↓ downswing)"));

        let (color, word) = status_word(r10, gs_ok);
        let p = ui.painter();
        p.rect(st, 0.0, TILE, Stroke::new(1.0, LINE), StrokeKind::Inside);
        p.text(st.left_top() + vec2(12.0, 10.0), Align2::LEFT_TOP, tr("STATUS"), cond(15.0), MUTED);
        if let Some(level) = battery {
            draw_battery(p, pos2(st.right() - 12.0, st.top() + 17.0), level);
        }
        let word_size = if word.len() > 8 { 24.0 } else { 38.0 };
        // Capitals centred between the header line and the R10/GSPro dots.
        caps_text(p, pos2(st.center().x, st.top() + 52.75), egui::Align::Center, word, word_size, color);
        let dot_y = st.bottom() - 20.0;
        let r10_c = match r10 {
            R10Status::Ready => GREEN,
            R10Status::Connected => AMBER,
            R10Status::Searching => RED,
        };
        let gs_c = if gs_ok { GREEN } else { AMBER };
        for (i, (label, col)) in [("R10", r10_c), ("GSPro", gs_c)].into_iter().enumerate() {
            let cx = st.left() + st.width() * (0.3 + 0.4 * i as f32);
            p.circle_filled(pos2(cx - 22.0, dot_y), 4.5, col);
            p.text(pos2(cx - 13.0, dot_y), Align2::LEFT_CENTER, label, FontId::proportional(13.0), SOFT);
        }
        p.rect_filled(Rect::from_min_max(pos2(st.left(), st.bottom() - 4.0), st.max), 0.0, color);
        let st_resp = ui.interact(st, ui.id().with("status"), Sense::hover());
        st_resp.on_hover_text(format!("R10: {}\nGSPro: {}", tr_detail(&r10_detail), tr_detail(&gs_detail)));
    }

    fn settings(&mut self, ui: &mut egui::Ui) {
        // 2 × 2 cards; cards side by side share their row's height.
        let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), 184.0 + 10.0 + 140.0), Sense::hover());
        let col_w = (area.width() - 10.0) / 2.0;
        let cell = |col: f32, top: f32, h: f32| {
            Rect::from_min_size(pos2(area.left() + col * (col_w + 10.0), area.top() + top), vec2(col_w, h))
        };
        let (r10_device, r10_battery, r10) = state::with(|s| (s.r10_device.clone(), s.r10_battery, s.r10));

        card(ui, cell(0.0, 0.0, 184.0), tr("GSPRO"), Icon::Monitor, |ui| {
            setting_row(ui, tr("Launcher"), |ui| {
                let text = self.edit.launcher_path.as_ref().map_or_else(
                    || tr("– not selected –").to_string(),
                    |p| short_path(p.as_path()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    #[cfg(target_os = "macos")]
                    if ui.add(secondary_button(tr("Choose …"))).clicked()
                        && let Some(p) = rfd::FileDialog::new()
                            .set_title(tr("Choose GSPro launcher (CrossOver app)"))
                            .add_filter("Applications", &["app"])
                            .pick_file()
                    {
                        self.edit.launcher_path = Some(p);
                    }
                    if ui.add(secondary_button(tr("Find"))).clicked() {
                        match launcher::find_gspro_launchers().into_iter().next() {
                            Some(p) => self.edit.launcher_path = Some(p),
                            None => {
                                self.note = Some(tr("No GSPro launcher found under ~/Applications/CrossOver.").into());
                            }
                        }
                    }
                    read_only_field(ui, ui.available_width(), &text);
                });
            });
            setting_row(ui, tr("Address"), |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.edit.gspro_addr)
                        .desired_width(130.0)
                        .vertical_align(egui::Align::Center)
                        // Height from the margin: 34 px like the other controls.
                        .margin(Margin::symmetric(10, 11)),
                );
                ui.label(RichText::new(tr("Applies after restart")).size(12.0).color(MUTED));
            });
            setting_row(ui, tr("Autostart"), |ui| {
                toggle(ui, &mut self.edit.auto_start_gspro, tr("Open GSPro on start"));
            });
        });

        card(ui, cell(1.0, 0.0, 184.0), tr("DISPLAY"), Icon::Eye, |ui| {
            setting_row(ui, tr("Language"), |ui| {
                egui::ComboBox::from_id_salt("language")
                    .width(128.0)
                    .selected_text(self.edit.language.label())
                    .show_ui(ui, |ui| {
                        for l in Language::ALL {
                            ui.selectable_value(&mut self.edit.language, l, l.label());
                        }
                    });
            });
            setting_row(ui, tr("Units"), |ui| {
                segmented(ui, &mut self.edit.metric, "km/h", "mph");
                ui.add_space(4.0);
                segmented(ui, &mut self.edit.meters, "m", "yd");
            });
            setting_row(ui, tr("Interface"), |ui| {
                let options = [(None, tr("Auto")), (Some(1.0), "100 %"), (Some(1.25), "125 %"), (Some(1.5), "150 %")];
                let mut scale = self.saved.ui_scale;
                // Sizes whose smallest window doesn't fit on the screen can't be chosen.
                let room = screen_room(ui.ctx());
                let fits = |s: Option<f32>| room.is_none_or(|r| min_size(s).x <= r.x + 0.5 && min_size(s).y <= r.y + 0.5);
                if segments_where(ui, &mut scale, &options, fits, tr("Too large for this screen")) {
                    // Applies and saves at once, like the overlay's settings.
                    self.saved.ui_scale = scale;
                    self.edit.ui_scale = scale;
                    if let Err(e) = self.saved.save() {
                        log::info(&format!("Could not save the settings: {e}"));
                    }
                }
            });
        });

        card(ui, cell(0.0, 194.0, 140.0), "R10", Icon::Signal, |ui| {
            setting_row(ui, tr("Ball distance"), |ui| {
                stepper(ui, &mut self.edit.tee_distance, 1.5..=4.0, 0.05, "m")
                    .on_hover_text(tr("Distance from the R10 to the ball; default 2.10 m"));
                ui.label(RichText::new(tr("Applies after restart")).size(12.0).color(MUTED));
            });
            setting_row(ui, tr("Device"), |ui| {
                let (dot, text) = match &r10_device {
                    Some(id) => {
                        let short: String = id.chars().take(8).collect();
                        let mut t = format!("Approach R10 · {short}");
                        if let Some(level) = r10_battery {
                            t.push_str(&format!(" · {} {level} %", tr("Battery")));
                        }
                        (if r10 == R10Status::Ready { GREEN } else { AMBER }, t)
                    }
                    None => (RED, tr("Not connected").to_string()),
                };
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover());
                ui.painter().circle_filled(pos2(r.left() + 4.0, r.center().y), 4.0, dot);
                ui.painter().text(pos2(r.left() + 16.0, r.center().y), Align2::LEFT_CENTER, text, FontId::proportional(14.0), SOFT);
                if let Some(id) = &r10_device {
                    resp.on_hover_text(id.as_str());
                }
            });
        });

        let updates_card = cell(1.0, 194.0, 140.0);
        let check = state::with(|s| s.update_check.clone());
        card(ui, updates_card, tr("NOTIFICATIONS & UPDATES"), Icon::Bell, |ui| {
            setting_row(ui, tr("Notifications"), |ui| {
                if toggle(ui, &mut self.edit.notifications, tr("Notify about problems"))
                    .on_hover_text(tr("Low R10 battery, R10 errors (e.g. tilted), lost connections, shots not delivered"))
                    .changed()
                {
                    // Takes effect immediately; saved with the other settings.
                    openlaunch_connect::notify::ENABLED.store(self.edit.notifications, std::sync::atomic::Ordering::Relaxed);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(secondary_button(tr("Test")))
                        .on_hover_text(tr("Sends a notification now. If none appears, allow notifications for OpenLaunch Connect in System Settings → Notifications."))
                        .clicked()
                    {
                        openlaunch_connect::notify::send_now("Notifications are working.");
                    }
                });
            });
            setting_row(ui, tr("Updates"), |ui| {
                toggle(ui, &mut self.edit.check_updates, tr("Check on start"))
                    .on_hover_text(tr("Asks GitHub for the latest release when the app starts"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(check != Some(update::Check::Running), secondary_button(tr("Update")))
                        .on_hover_text(tr("Asks GitHub for the latest release now"))
                        .clicked()
                    {
                        update::check_in_background();
                    }
                });
            });
        });

        // Installed version and the outcome of the last check, right in the card's header row.
        let installed = tr("Installed: {v}").replace("{v}", env!("CARGO_PKG_VERSION"));
        let (text, hint) = match &check {
            None => (installed, None),
            Some(update::Check::Running) => (format!("{installed} · {}", tr("checking …")), None),
            Some(update::Check::UpToDate) => (format!("{installed} · {}", tr("up to date")), None),
            Some(update::Check::Available) => {
                let v = state::with(|s| s.update.as_ref().map(|r| r.version.clone())).unwrap_or_default();
                (format!("{installed} · {}", tr("update {v} available").replace("{v}", &v)), None)
            }
            Some(update::Check::Failed(e)) => (format!("{installed} · {}", tr("check failed")), Some(e.clone())),
        };
        let r = ui.painter().text(
            pos2(updates_card.right() - 16.0, updates_card.top() + 21.0),
            Align2::RIGHT_CENTER,
            text,
            FontId::proportional(12.0),
            MUTED,
        );
        if let Some(hint) = hint {
            ui.interact(r, ui.id().with("update-check"), Sense::hover()).on_hover_text(hint);
        }

        // Footer: status note and the primary save button, right-aligned.
        ui.add_space(12.0);
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::hover());
        let dirty = self.edit != self.saved;
        let save_r = Rect::from_min_max(pos2(bar.right() - 160.0, bar.top()), bar.max);
        if button(ui, save_r, tr("SAVE"), Icon::None, true, dirty, tr("No changes")) {
            let restart = self.edit.gspro_addr != self.saved.gspro_addr
                || (self.edit.tee_distance - self.saved.tee_distance).abs() > f32::EPSILON;
            match self.edit.save() {
                Ok(()) => {
                    self.saved = self.edit.clone();
                    i18n::set(self.saved.language);
                    self.note = Some(if restart {
                        tr("Saved. Address and distance apply after the next start.").into()
                    } else {
                        tr("Saved.").into()
                    });
                }
                Err(e) => self.note = Some(tr("Saving failed: {e}").replace("{e}", &e)),
            }
        }
        // Left: back to the explorer's default pages (applies and saves at once, like arranging).
        let label = tr("RESET SHOTS VIEW");
        let w = ui.painter().layout_no_wrap(label.to_string(), cond(19.0), WHITE).size().x + 40.0;
        let reset_r = Rect::from_min_size(bar.min, vec2(w, bar.height()));
        let is_default = self.saved.explorer == openlaunch_connect::settings::Explorer::default();
        if button(ui, reset_r, label, Icon::None, false, !is_default, tr("The shots view is as it came")) {
            self.confirm_reset = true;
        }
        if self.confirm_reset {
            let mut reset = false;
            let modal = egui::Modal::new(egui::Id::new("reset-shot-pages"))
                .frame(egui::Frame::new().fill(TILE).stroke(Stroke::new(1.0, BUTTON_LINE)).corner_radius(8).inner_margin(20))
                .show(ui.ctx(), |ui| {
                    ui.set_width(360.0);
                    ui.label(RichText::new(tr("Reset the shots view?")).size(16.0).color(WHITE));
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(tr("All pages and tiles go back to how they came. Your shots are kept."))
                            .size(13.0)
                            .color(SOFT),
                    );
                    ui.add_space(16.0);
                    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover());
                    let ok_r = Rect::from_min_max(pos2(row.right() - 130.0, row.top()), row.max);
                    let cancel_r = Rect::from_min_max(pos2(ok_r.left() - 140.0, row.top()), pos2(ok_r.left() - 10.0, row.bottom()));
                    if button(ui, cancel_r, tr("CANCEL"), Icon::None, false, true, "") {
                        ui.close();
                    }
                    if button(ui, ok_r, tr("RESET"), Icon::None, true, true, "") {
                        reset = true;
                        ui.close();
                    }
                });
            if reset {
                self.saved.explorer = openlaunch_connect::settings::Explorer::default();
                self.edit.explorer = self.saved.explorer.clone();
                self.note = Some(match self.saved.save() {
                    Ok(()) => tr("Shots view reset.").into(),
                    Err(e) => tr("Saving failed: {e}").replace("{e}", &e),
                });
            }
            if modal.should_close() {
                self.confirm_reset = false;
            }
        }
        let note = if dirty { Some(tr("Unsaved changes").to_string()) } else { self.note.clone() };
        if let Some(note) = note {
            ui.painter().text(
                pos2(save_r.left() - 14.0, bar.center().y),
                Align2::RIGHT_CENTER,
                note,
                FontId::proportional(13.0),
                MUTED,
            );
        }
    }

    /// Always-on-top, click-through status panel for playing GSPro in full screen.
    fn overlay(&mut self, ctx: &egui::Context) {
        self.status_rect = None;
        let Some((ax, ay)) = self.saved.overlay.anchor().filter(|_| self.show_overlay) else {
            self.moving = self.moving.take().filter(|m| m.tile.is_some());
            self.passthrough.remove(&None);
            return;
        };
        let Some(screen) = self.screen_size(ctx) else {
            return;
        };
        let style = &self.saved.overlay_style;
        let content = OverlayContent::live(style, self.saved.metric, self.saved.meters);
        // Measured in the overlay's own window last frame, at least the size measured here.
        let base = content.size(ctx).max(self.overlay_base.get());
        // The overlay window's size and place go to the system in native points (without the
        // zoom of egui's points, which the drawing below takes into account), within the part of
        // the screen not covered by the menu bar and the Dock.
        let zoom = ctx.zoom_factor();
        // A transparent border around the panel, so rounding the window to whole pixels never
        // cuts its edge.
        let size = (base + vec2(2.0, 2.0) * OVERLAY_BORDER) * style.scale;
        #[cfg(target_os = "macos")]
        let visible = mac::visible_frame();
        #[cfg(not(target_os = "macos"))]
        let visible: Option<Rect> = None;
        let visible = visible.unwrap_or(Rect::from_min_size(egui::Pos2::ZERO, screen * zoom));
        let moving = self.moving.as_ref().filter(|m| m.tile.is_none());
        let pos = moving.and_then(|m| m.at).unwrap_or_else(|| overlay_origin(visible, size, (ax, ay), self.saved.overlay_spot));
        // On macOS the window is placed through AppKit (below): egui takes the size and place
        // of this window in native points but reports them with the zoom, so the builder only
        // gives the first ones and then stays the same.
        #[cfg(target_os = "macos")]
        let (pos, size_asked) = *self.overlay_first.get_or_insert((pos, size));
        // Elsewhere through egui, in the overlay pass's points (zoomed by the overlay's size).
        #[cfg(not(target_os = "macos"))]
        let size_asked = size / style.scale;
        #[cfg(not(target_os = "macos"))]
        let pos = pos / style.scale;
        let grabbable = moving.is_some();
        let builder = egui::ViewportBuilder::default()
            .with_title(OVERLAY_TITLE)
            .with_position(pos)
            .with_inner_size(size_asked)
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_maximize_button(false)
            .with_window_level(egui::WindowLevel::AlwaysOnTop)
            .with_mouse_passthrough(!grabbable)
            .with_active(false)
            .with_taskbar(false);
        let capture = &mut self.capture;
        let overlay_base = &self.overlay_base;
        let user_scale = style.scale;
        let pos_native = moving.and_then(|m| m.at).unwrap_or_else(|| overlay_origin(visible, size, (ax, ay), self.saved.overlay_spot));
        self.status_rect = Some(Rect::from_min_size(pos_native, size));
        let mut moving = self.moving.as_mut().filter(|m| m.tile.is_none());
        let mut dropped = None;
        // The overlay's pass takes the zoom from the options when it begins: the overlay's size
        // there, the main window's back right after (no repaint is triggered either way).
        ctx.options_mut(|o| o.zoom_factor = user_scale);
        let covered = self.all_covered;
        ctx.show_viewport_immediate(overlay_id(), builder, |ui, _class| {
            // Only kept open (see `App::logic`).
            if covered {
                return;
            }
            #[cfg(target_os = "macos")]
            mac::float_over_fullscreen(ui.ctx(), OVERLAY_TITLE, grabbable);
            // Measured here, in the overlay's own window (the next frame sizes the window by it).
            let own = content.size(ui.ctx());
            overlay_base.set(own);
            #[cfg(target_os = "macos")]
            mac::place_window(OVERLAY_TITLE, pos_native, size);

            // This window's pass runs with the overlay's size as its zoom (set above), so the
            // overlay is drawn at its base size: no scaling of shapes or layers needed.
            let rect = Rect::from_min_size(pos2(OVERLAY_BORDER, OVERLAY_BORDER), own);
            overlay_ui(&ui.ctx().layer_painter(overlay_layer()).with_clip_rect(rect.expand(OVERLAY_BORDER)), rect, &content);
            if let Some(m) = moving.as_deref_mut() {
                dropped = drag_overlay(ui, rect, m, pos_native, user_scale);
            }
            capture.save_from(ui.ctx());
        });
        ctx.options_mut(|o| o.zoom_factor = zoom);
        self.set_passthrough(ctx, None, !grabbable);
        if let Some(at) = dropped {
            self.saved.overlay_spot = Some(overlay_spot(visible, size, at));
            self.moving = None;
            self.save_overlays();
        }
    }

    /// The main window's screen size, or the last one known (see `screen_size`).
    fn screen_size(&mut self, ctx: &egui::Context) -> Option<egui::Vec2> {
        if let Some(size) = ctx.input(|i| i.viewport().monitor_size) {
            self.screen_size = Some(size);
        }
        self.screen_size
    }

    /// Lets clicks go through the status (`None`) or a tile overlay, or not while it is moved;
    /// told to the window when it changes.
    fn set_passthrough(&mut self, ctx: &egui::Context, which: Option<usize>, on: bool) {
        if self.passthrough.get(&which) != Some(&on) {
            let id = which.map_or_else(overlay_id, tile_overlay_id);
            ctx.send_viewport_cmd_to(id, egui::ViewportCommand::MousePassthrough(on));
            self.passthrough.insert(which, on);
        }
    }

    /// Saves the overlays (applied at once), into the arrangement in use, too.
    fn save_overlays(&mut self) {
        let s = &mut self.saved;
        s.keep_arrangement();
        // The settings page saves its own copy; it keeps these as they are.
        let e = &mut self.edit;
        e.overlay = s.overlay;
        e.overlay_spot = s.overlay_spot;
        e.overlay_style = s.overlay_style.clone();
        e.tile_overlays = s.tile_overlays.clone();
        e.arrangements = s.arrangements.clone();
        e.arrangement = s.arrangement;
        if let Err(e) = s.save() {
            log::info(&format!("Saving the overlay settings failed: {e}"));
        }
    }

    /// Shows the overlays of arrangement `i` (and saves that it is in use).
    fn use_arrangement(&mut self, i: usize) {
        self.saved.use_arrangement(i);
        self.moving = None;
        self.overlay_tab = self.overlay_tab.filter(|&k| k < self.saved.tile_overlays.len());
        self.save_overlays();
    }

    /// The arrangement's name as shown (the first one's translated while unchanged).
    fn arrangement_name(&self, i: usize) -> String {
        let name = self.saved.arrangements.get(i).map_or(DEFAULT_ARRANGEMENT, |a| a.name.as_str());
        if name == DEFAULT_ARRANGEMENT { tr(DEFAULT_ARRANGEMENT).to_string() } else { name.to_string() }
    }

    /// Choosing, renaming, adding and deleting arrangements: the menu of `resp`. `full` adds
    /// renaming and the rest (the Overlay page), else only choosing (the Play page).
    fn arrangement_menu(&mut self, resp: &egui::Response, full: bool) {
        let mut chosen = None;
        let mut renamed = None;
        let mut added = false;
        egui::Popup::menu(resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            for i in 0..self.saved.arrangements.len() {
                if ui.selectable_label(i == self.saved.arrangement, self.arrangement_name(i)).clicked() {
                    chosen = Some(i);
                    ui.close();
                }
            }
            if !full {
                return;
            }
            ui.separator();
            let mut name = self.arrangement_name(self.saved.arrangement);
            if ui.add(egui::TextEdit::singleline(&mut name).hint_text(tr("Name")).desired_width(170.0)).on_hover_text(tr("Name of the arrangement")).changed() {
                renamed = Some(name);
            }
            if ui.button(tr("Save as a new arrangement")).on_hover_text(tr("A copy of the overlays as they are, to change from there")).clicked() {
                added = true;
                ui.close();
            }
            if self.saved.arrangements.len() > 1 && ui.button(RichText::new(tr("Delete arrangement")).color(RED)).clicked() {
                self.confirm_delete_arrangement = Some(self.saved.arrangement);
                ui.close();
            }
        });
        if let Some(name) = renamed {
            let i = self.saved.arrangement;
            self.saved.arrangements[i].name = name;
            self.save_overlays();
        }
        if added {
            let n = self.saved.arrangements.len() + 1;
            let copy = Arrangement { name: tr("Arrangement {n}").replace("{n}", &n.to_string()), ..self.saved.arrangements[self.saved.arrangement].clone() };
            self.saved.arrangements.push(copy);
            self.use_arrangement(self.saved.arrangements.len() - 1);
        }
        if let Some(i) = chosen {
            self.use_arrangement(i);
        }
    }

    /// "Delete the arrangement?" while asked.
    fn confirm_delete_arrangement(&mut self, ctx: &egui::Context) {
        let Some(i) = self.confirm_delete_arrangement else { return };
        let name = self.arrangement_name(i);
        let mut delete = false;
        let modal = egui::Modal::new(egui::Id::new("delete-arrangement"))
            .frame(egui::Frame::new().fill(TILE).stroke(Stroke::new(1.0, BUTTON_LINE)).corner_radius(8).inner_margin(20))
            .show(ctx, |ui| {
                ui.set_width(360.0);
                ui.label(RichText::new(tr("Delete the arrangement “{name}”?").replace("{name}", &name)).size(16.0).color(WHITE));
                ui.add_space(6.0);
                ui.label(RichText::new(tr("Its overlays go with it.")).size(13.0).color(SOFT));
                ui.add_space(16.0);
                let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover());
                let ok_r = Rect::from_min_max(pos2(row.right() - 130.0, row.top()), row.max);
                let cancel_r = Rect::from_min_max(pos2(ok_r.left() - 140.0, row.top()), pos2(ok_r.left() - 10.0, row.bottom()));
                if button(ui, cancel_r, tr("CANCEL"), Icon::None, false, true, "") {
                    ui.close();
                }
                if button(ui, ok_r, tr("DELETE"), Icon::None, true, true, "") {
                    delete = true;
                    ui.close();
                }
            });
        if delete && i < self.saved.arrangements.len() && self.saved.arrangements.len() > 1 {
            self.saved.arrangements.remove(i);
            let next = self.saved.arrangement.min(self.saved.arrangements.len() - 1);
            // Shown without writing the deleted one's overlays into another.
            self.saved.arrangement = next;
            self.use_arrangement(next);
        }
        if modal.should_close() {
            self.confirm_delete_arrangement = None;
        }
    }

    /// Always-on-top, click-through windows with one tile of the shots explorer each. Overlays at
    /// the same place (not placed by hand) stack: each under the one before (above it at the bottom
    /// of the screen), the status first.
    fn tile_overlays(&mut self, ctx: &egui::Context) {
        let tiles = self.saved.tile_overlays.clone();
        self.moving = self.moving.take().filter(|m| m.tile.is_none_or(|k| tiles.get(k).is_some_and(|t| t.pos != OverlayPos::Off)));
        if !self.show_overlay {
            return;
        }
        let Some(screen) = self.screen_size(ctx) else {
            return;
        };
        let zoom = ctx.zoom_factor();
        #[cfg(target_os = "macos")]
        let visible = mac::visible_frame();
        #[cfg(not(target_os = "macos"))]
        let visible: Option<Rect> = None;
        let visible = visible.unwrap_or(Rect::from_min_size(egui::Pos2::ZERO, screen * zoom));
        // The last overlay at each of the nine places.
        let mut taken: HashMap<OverlayPos, Rect> = HashMap::new();
        if let Some(status) = self.status_rect.filter(|_| self.saved.overlay_spot.is_none()) {
            taken.insert(self.saved.overlay, status);
        }
        let (shots, club) = state::with(|s| (s.session.clone(), s.gspro_club.clone()));
        for (k, cfg) in tiles.iter().enumerate() {
            let Some((ax, ay)) = cfg.pos.anchor() else {
                self.passthrough.remove(&Some(k));
                continue;
            };
            let size = vec2(cfg.width, cfg.height()) + vec2(2.0, 2.0) * OVERLAY_BORDER;
            let mut pos = overlay_origin(visible, size, (ax, ay), cfg.spot);
            if cfg.spot.is_none() {
                if let Some(before) = taken.get(&cfg.pos) {
                    pos.y = if ay < 1.0 { before.bottom() + 8.0 } else { before.top() - 8.0 - size.y };
                }
                taken.insert(cfg.pos, Rect::from_min_size(pos, size));
            }
            let mut moving = self.moving.as_mut().filter(|m| m.tile == Some(k));
            if let Some(at) = moving.as_ref().and_then(|m| m.at) {
                pos = at;
            }
            let grabbable = moving.is_some();
            #[cfg(target_os = "macos")]
            let (first_pos, size_asked) = *self.tile_overlay_first.entry(k).or_insert((pos, size));
            #[cfg(not(target_os = "macos"))]
            let (first_pos, size_asked) = (pos, size);
            let title = tile_overlay_title(k);
            let builder = egui::ViewportBuilder::default()
                .with_title(title.clone())
                .with_position(first_pos)
                .with_inner_size(size_asked)
                .with_decorations(false)
                .with_transparent(true)
                .with_resizable(false)
                .with_maximize_button(false)
                .with_window_level(egui::WindowLevel::AlwaysOnTop)
                .with_mouse_passthrough(!grabbable)
                .with_active(false)
                .with_taskbar(false);
            let units = (self.saved.metric, self.saved.meters);
            let gspro = self.saved.gspro_results;
            let explorer = &mut self.explorer;
            let mut dropped = None;
            // Drawn at its own size, whatever the main window's zoom (see `overlay`).
            ctx.options_mut(|o| o.zoom_factor = 1.0);
            let covered = self.all_covered;
            ctx.show_viewport_immediate(tile_overlay_id(k), builder, |ui, _class| {
                // Only kept open (see `App::logic`).
                if covered {
                    return;
                }
                #[cfg(target_os = "macos")]
                {
                    mac::float_over_fullscreen(ui.ctx(), &title, grabbable);
                    mac::place_window(&title, pos, size);
                }
                let rect = Rect::from_min_size(pos2(OVERLAY_BORDER, OVERLAY_BORDER), vec2(cfg.width, cfg.height()));
                explorer.overlay_tile(ui, rect, k, cfg, &shots, club.as_deref(), units, gspro);
                if let Some(m) = moving.as_deref_mut() {
                    dropped = drag_overlay(ui, rect, m, pos, 1.0);
                }
            });
            ctx.options_mut(|o| o.zoom_factor = zoom);
            self.set_passthrough(ctx, Some(k), !grabbable);
            if let Some(at) = dropped {
                self.saved.tile_overlays[k].spot = Some(overlay_spot(visible, size, at));
                self.moving = None;
                self.save_overlays();
            }
        }
    }

    /// Overlay position, size and contents with a preview; changes apply and are saved at once.
    fn overlay_page(&mut self, ui: &mut egui::Ui) {
        self.confirm_delete_arrangement(ui.ctx());
        if let Some(k) = self.overlay_tab.filter(|&k| k < self.saved.tile_overlays.len()) {
            return self.tile_overlay_page(ui, k);
        }
        self.overlay_tab = None;
        let mut pos = self.saved.overlay;
        let mut spot = self.saved.overlay_spot;
        let mut style = self.saved.overlay_style.clone();
        let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), 254.0), Sense::hover());
        let col_w = (area.width() - 10.0) / 2.0;
        let left = Rect::from_min_size(area.min, vec2(col_w, area.height()));
        let right = Rect::from_min_size(pos2(area.left() + col_w + 10.0, area.top()), vec2(col_w, area.height()));

        let title = self.arrangement_name(self.saved.arrangement).to_uppercase();
        card(ui, left, &title, Icon::Layout, |ui| {
            ui.spacing_mut().item_spacing.y = 7.0;
            ui.horizontal(|ui| {
                setting_label(ui, tr("Position"), 72.0);
                if overlay_picker(ui, &mut pos, spot.is_some()) {
                    spot = None;
                }
                ui.add_space(12.0);
                place_text(ui, pos, spot.is_some(), tr("On top of GSPro, also in full screen."), &mut self.moving, None);
            });
            setting_row(ui, tr("Size"), |ui| {
                segments(ui, &mut style.scale, &[(0.75, "75 %"), (1.0, "100 %"), (1.25, "125 %"), (1.5, "150 %")]);
            });
            setting_row(ui, tr("Lines"), |ui| {
                segments(ui, &mut style.one_line, &[(false, tr("Two lines")), (true, tr("One line"))]);
            });
            setting_row(ui, tr("Background"), |ui| {
                segments(ui, &mut style.opacity, &[(0.7, "70 %"), (0.85, "85 %"), (1.0, tr("Solid"))]);
            });
        });
        self.overlay_card_head(ui, left, &title);

        card(ui, right, tr("CONTENTS"), Icon::List, |ui| {
            ui.label(RichText::new(tr("Click a tile in the preview to choose what it shows, drag it to move it.")).size(13.0).color(SOFT));
            ui.label(RichText::new(tr("+ at the end of the tiles adds a column. The status always comes first.")).size(12.0).color(MUTED));
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(style.layout != OverlayItem::default_layout(), secondary_button(tr("Reset")))
                    .on_hover_text(tr("Back to the tiles as they came"))
                    .clicked()
                {
                    style.layout = OverlayItem::default_layout();
                }
            });

        });

        // Below the cards, down to the page's bottom margin (the cards move the layout cursor).
        let preview = Rect::from_min_max(pos2(area.left(), area.bottom() + 10.0), pos2(area.right(), ui.max_rect().bottom()));
        self.overlay_preview(ui, preview, pos, &mut style);

        if pos != self.saved.overlay || spot != self.saved.overlay_spot || style != self.saved.overlay_style {
            self.saved.overlay = pos;
            self.saved.overlay_spot = spot;
            self.saved.overlay_style = style;
            self.save_overlays();
        }
    }

    /// The tile overlay's position, size and tile with a preview; changes apply and are saved at once.
    fn tile_overlay_page(&mut self, ui: &mut egui::Ui, k: usize) {
        let mut cfg = self.saved.tile_overlays[k].clone();
        let mut remove = false;
        // One card with everything (its tabs where the status overlay's are), the preview next to it.
        let (area, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
        let col_w = (area.width() - 10.0) / 2.0;
        let left = Rect::from_min_size(area.min, vec2(col_w, area.height()));
        let right = Rect::from_min_size(pos2(area.left() + col_w + 10.0, area.top()), vec2(col_w, area.height()));

        let title = self.arrangement_name(self.saved.arrangement).to_uppercase();
        card(ui, left, &title, Icon::Layout, |ui| {
            ui.spacing_mut().item_spacing.y = 7.0;
            ui.horizontal(|ui| {
                setting_label(ui, tr("Position"), 72.0);
                if overlay_picker(ui, &mut cfg.pos, cfg.spot.is_some()) {
                    cfg.spot = None;
                }
                ui.add_space(12.0);
                place_text(ui, cfg.pos, cfg.spot.is_some(), tr("Under the overlays at its place."), &mut self.moving, Some(k));
            });
            setting_row(ui, tr("Size"), |ui| {
                let [s, m, l] = TileOverlay::WIDTHS;
                segments(ui, &mut cfg.width, &[(s, tr("Small")), (m, tr("Medium")), (l, tr("Large"))]);
            });
            setting_row(ui, tr("Background"), |ui| {
                segments(ui, &mut cfg.opacity, &[(0.7, "70 %"), (0.85, "85 %"), (1.0, tr("Solid"))]);
            });
            explorer::tile_options(ui, &mut cfg.tile, self.saved.metric, self.saved.meters, |ui| {
                ui.add_space(4.0);
                let resp = ui.add(egui::Button::new(RichText::new(tr("Remove tile")).size(13.0).color(RED)).frame(false));
                if resp.on_hover_text(tr("Takes this tile off the screen and out of the arrangement")).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    remove = true;
                }
            });
            setting_row(ui, tr("Shots"), |ui| {
                segments(ui, &mut cfg.club_only, &[(false, tr("This session")), (true, tr("Club in GSPro"))]);
            });
        });
        self.overlay_card_head(ui, left, &title);
        self.tile_overlay_preview(ui, right, &cfg);

        if remove {
            self.saved.tile_overlays.remove(k);
            self.moving = None;
            self.overlay_tab = k.checked_sub(1).or((!self.saved.tile_overlays.is_empty()).then_some(0));
            self.save_overlays();
        } else if cfg != self.saved.tile_overlays[k] {
            self.saved.tile_overlays[k] = cfg;
            self.save_overlays();
        }
    }

    /// The left card's head on the Overlay page: the arrangement's name opens its menu, the tabs
    /// pick the overlay edited, *+* adds a tile.
    fn overlay_card_head(&mut self, ui: &mut egui::Ui, card: Rect, title: &str) {
        let w = ui.painter().layout_no_wrap(title.to_string(), cond(15.0), LABEL).size().x + title.chars().count() as f32 * 1.5;
        let head = Rect::from_min_max(pos2(card.left() + 14.0, card.top() + 6.0), pos2(card.left() + 58.0 + w, card.top() + 36.0));
        let resp = ui.interact(head, ui.id().with("arrangement"), Sense::click());
        let c = pos2(head.right() - 4.0, card.top() + 21.0);
        let color = if resp.hovered() { WHITE } else { LABEL };
        ui.painter().add(egui::Shape::convex_polygon(vec![c + vec2(-4.0, -2.0), c + vec2(4.0, -2.0), c + vec2(0.0, 3.0)], color, Stroke::NONE));
        let resp = resp.on_hover_text(tr("Arrangement: click to switch, rename or save as a new one")).on_hover_cursor(egui::CursorIcon::PointingHand);
        self.arrangement_menu(&resp, true);
        let count = self.saved.tile_overlays.len();
        if overlay_tabs(ui, card, &mut self.overlay_tab, count) {
            self.saved.tile_overlays.push(TileOverlay::default());
            self.overlay_tab = Some(count);
            self.save_overlays();
        }
    }

    /// The tile overlay on the GSPro-like backdrop, scaled to fit, with the session's shots (or
    /// samples while there are none).
    fn tile_overlay_preview(&mut self, ui: &mut egui::Ui, area: Rect, cfg: &TileOverlay) {
        let p = ui.painter();
        backdrop(p, area);
        let anchor = cfg.pos.anchor();
        let label_right = anchor.is_some_and(|(ax, ay)| ay > 0.5 && ax < 1.0);
        let (label_pos, label_align) = if label_right {
            (pos2(area.right() - 12.0, area.bottom() - 14.0), egui::Align::Max)
        } else {
            (pos2(area.left() + 12.0, area.bottom() - 14.0), egui::Align::Min)
        };
        caps_text(p, label_pos, label_align, tr("PREVIEW"), 14.0, SOFT);
        let Some((ax, ay)) = anchor else {
            p.text(area.center(), Align2::CENTER_CENTER, tr("Overlay off"), FontId::proportional(14.0), SOFT);
            return;
        };
        // Placed by hand: there in the preview, too.
        let [ax, ay] = cfg.spot.unwrap_or([ax, ay]);
        let (mut shots, club) = state::with(|s| (s.session.clone(), s.gspro_club.clone()));
        if shots.is_empty() {
            shots = self.sample_shots.get_or_init(engine::demo_session).clone();
        }
        let club = club.or_else(|| shots.last().and_then(|s| s.club_code.clone()));
        let size = vec2(cfg.width, cfg.height());
        let pad = 9.0;
        let room = area.size() - vec2(2.0 * pad, 2.0 * pad);
        let scale = (room.x / size.x).min(room.y / size.y).min(1.0);
        let free = (room - size * scale).max(egui::Vec2::ZERO);
        let origin = area.min + vec2(pad + free.x * ax, pad + free.y * ay);
        // Scaled on its own layer, like the status overlay's preview.
        let layer = egui::LayerId::new(egui::Order::Middle, egui::Id::new("tile-overlay-preview"));
        ui.ctx().set_transform_layer(layer, egui::emath::TSTransform::new(origin.to_vec2(), scale));
        let rect = Rect::from_min_size(egui::Pos2::ZERO, size);
        let clip = Rect::from_min_max(((area.min - origin) / scale).to_pos2(), ((area.max - origin) / scale).to_pos2());
        let mut tile_ui = ui.new_child(egui::UiBuilder::new().layer_id(layer).max_rect(rect));
        tile_ui.set_clip_rect(clip);
        let units = (self.saved.metric, self.saved.meters);
        self.explorer.overlay_tile(&mut tile_ui, rect, 500, cfg, &shots, club.as_deref(), units, self.saved.gspro_results);
    }

    /// The overlay with a sample shot on a GSPro-like backdrop, at its size and position.
    fn overlay_preview(&self, ui: &egui::Ui, area: Rect, pos: OverlayPos, style: &mut OverlayStyle) {
        let p = ui.painter();
        backdrop(p, area);
        let anchor = pos.anchor();
        // The label goes to the bottom corner the overlay leaves free.
        let label_right = anchor.is_some_and(|(ax, ay)| ay > 0.5 && ax < 1.0);
        let (label_pos, label_align) = if label_right {
            (pos2(area.right() - 12.0, area.bottom() - 14.0), egui::Align::Max)
        } else {
            (pos2(area.left() + 12.0, area.bottom() - 14.0), egui::Align::Min)
        };
        caps_text(p, label_pos, label_align, tr("PREVIEW"), 14.0, SOFT);
        let Some((ax, ay)) = anchor else {
            p.text(area.center(), Align2::CENTER_CENTER, tr("Overlay off"), FontId::proportional(14.0), SOFT);
            return;
        };
        let [ax, ay] = self.saved.overlay_spot.unwrap_or([ax, ay]);
        let ready = (R10Status::Ready, "", true);
        let pin = Some(pin_text(139.0, self.saved.meters));
        let mut content = OverlayContent::new(style, self.saved.metric, ready, Some("I7"), Some(82), Some(&self.preview_shot), pin);
        content.edit = true;
        let size = content.size(ui.ctx());
        let pad = 9.0;
        let room = area.size() - vec2(2.0 * pad, 2.0 * pad);
        // At its size, or smaller when it doesn't fit (very long single lines).
        let scale = style.scale.min(room.x / size.x).min(room.y / size.y);
        let free = (room - size * scale).max(egui::Vec2::ZERO);
        let origin = area.min + vec2(pad + free.x * ax, pad + free.y * ay);
        // Scaled on its own layer, like the overlay window; clipped to the preview.
        let layer = egui::LayerId::new(egui::Order::Middle, egui::Id::new("overlay-preview"));
        ui.ctx().set_transform_layer(layer, egui::emath::TSTransform::new(origin.to_vec2(), scale));
        let clip = Rect::from_min_max(((area.min - origin) / scale).to_pos2(), ((area.max - origin) / scale).to_pos2());
        let tiles = overlay_ui(&ui.ctx().layer_painter(layer).with_clip_rect(clip), Rect::from_min_size(egui::Pos2::ZERO, size), &content);

        // A click on a tile picks what it shows; dragging it onto another tile swaps the two (the
        // status stays where it is).
        let to_page = |r: Rect| Rect::from_min_max(origin + r.min.to_vec2() * scale, origin + r.max.to_vec2() * scale);
        let tiles: Vec<(usize, usize, Rect)> =
            tiles.into_iter().filter(|(k, row, _)| (*k, *row) != (0, 0)).map(|(k, row, r)| (k, row, to_page(r).intersect(area))).collect();
        *self.overlay_tile_rects.borrow_mut() = tiles.clone();
        let pointer = ui.ctx().pointer_interact_pos();
        let mut swap: Option<((usize, usize), (usize, usize))> = None;
        let mut insert: Option<((usize, usize), usize)> = None;
        // The lines between the columns (and after the last one): (place in the layout, x, top, bottom).
        let (top, bottom) = tiles.iter().fold((f32::MAX, f32::MIN), |(a, b), t| (a.min(t.2.top()), b.max(t.2.bottom())));
        let mut lines: Vec<(usize, f32)> = Vec::new();
        for &(k, _, r) in &tiles {
            if k > 0 && !lines.iter().any(|l| l.0 == k) {
                lines.push((k, r.left()));
            }
        }
        if let Some(&(k, _, _)) = tiles.iter().max_by_key(|t| t.0) {
            let right = tiles.iter().filter(|t| t.0 == k).map(|t| t.2.right()).fold(f32::MIN, f32::max);
            lines.push((k + 1, right));
        }
        let line_at = |at: egui::Pos2| {
            lines.iter().copied().filter(|l| (l.1 - at.x).abs() <= 7.0 && at.y >= top && at.y <= bottom).min_by(|a, b| (a.1 - at.x).abs().total_cmp(&(b.1 - at.x).abs()))
        };
        for &(k, row, r) in &tiles {
            let resp = ui.interact(r, ui.id().with(("overlay-tile", k, row)), Sense::click_and_drag());
            // Onto a line between two columns: a new column there; else onto a tile: swap.
            let on_line = pointer.and_then(line_at);
            let target = pointer
                .filter(|_| on_line.is_none())
                .and_then(|at| tiles.iter().find(|t| t.2.contains(at)))
                .filter(|t| (t.0, t.1) != (k, row));
            if resp.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                let fg = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, ui.id().with("overlay-drag")));
                fg.rect_filled(r, 4.0, Color32::from_black_alpha(90));
                if let Some(t) = target {
                    fg.rect_stroke(t.2.shrink(1.0), 4.0, Stroke::new(2.0, ACCENT), StrokeKind::Inside);
                }
                if let Some((_, x)) = on_line {
                    fg.line_segment([pos2(x, top + 2.0), pos2(x, bottom - 2.0)], Stroke::new(3.0, ACCENT));
                }
                if let Some(at) = pointer {
                    let name = style.layout.get(k).and_then(|c| c[row]).map_or(tr("Empty"), overlay_item_name);
                    let galley = fg.layout_no_wrap(name.to_string(), FontId::proportional(12.0), WHITE);
                    let label = Rect::from_min_size(at + vec2(12.0, 8.0), galley.size() + vec2(14.0, 6.0));
                    fg.rect(label, 5.0, BUTTON, Stroke::new(1.0, ACCENT), StrokeKind::Inside);
                    fg.galley(label.min + vec2(7.0, 3.0), galley, WHITE);
                }
            } else if resp.hovered() {
                ui.painter().rect_stroke(r.shrink(1.0), 4.0, Stroke::new(1.0, ICON), StrokeKind::Inside);
            }
            if resp.drag_stopped() {
                if let Some((at, _)) = on_line {
                    insert = Some(((k, row), at));
                } else if let Some(t) = target {
                    swap = Some(((k, row), (t.0, t.1)));
                }
            }
            let resp = resp.on_hover_cursor(egui::CursorIcon::Grab).on_hover_text(tr("Click to choose what this tile shows, drag to move it"));
            egui::Popup::menu(&resp).show(|ui| {
                let current = style.layout.get(k).and_then(|c| c[row]);
                for item in OverlayItem::CHOICES {
                    if item.is_graphic() && k == 0 {
                        continue;
                    }
                    if item == OverlayItem::ImpactSide {
                        ui.separator();
                    }
                    if ui.selectable_label(current == Some(item), overlay_item_name(item)).clicked() {
                        set_tile(&mut style.layout, k, row, Some(item));
                    }
                }
                ui.separator();
                if ui.selectable_label(current.is_none(), tr("Empty")).clicked() {
                    set_tile(&mut style.layout, k, row, None);
                }
                // A value may fill its column (not next to the status, not a graphic).
                if k > 0
                    && let Some(item) = current.filter(|i| !i.is_graphic())
                {
                    let tall = style.layout[k] == [Some(item), Some(item)];
                    if ui.selectable_label(tall, tr("Double height")).clicked() {
                        style.layout[k] = if tall { [Some(item), None] } else { [Some(item), Some(item)] };
                    }
                }
                if k > 0 && ui.button(RichText::new(tr("Remove column")).color(RED)).clicked() {
                    style.layout.remove(k);
                }
            });
        }
        // + right after the last column adds one.
        if let Some(right) = tiles.iter().map(|t| t.2.right()).reduce(f32::max) {
            let top = tiles.iter().map(|t| t.2.top()).fold(f32::MAX, f32::min);
            let bottom = tiles.iter().map(|t| t.2.bottom()).fold(f32::MIN, f32::max);
            let r = Rect::from_center_size(pos2(right, (top + bottom) / 2.0), vec2(20.0, 20.0));
            let add = ui.interact(r, ui.id().with("overlay-add-column"), Sense::click());
            let fg = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, ui.id().with("overlay-add")));
            fg.circle(r.center(), 9.5, if add.hovered() { ACCENT } else { BUTTON }, Stroke::new(1.0, if add.hovered() { ACCENT_HOVER } else { BUTTON_LINE }));
            fg.line_segment([r.center() - vec2(4.5, 0.0), r.center() + vec2(4.5, 0.0)], Stroke::new(1.6, WHITE));
            fg.line_segment([r.center() - vec2(0.0, 4.5), r.center() + vec2(0.0, 4.5)], Stroke::new(1.6, WHITE));
            if add.on_hover_text(tr("Add an empty column at the end")).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                style.layout.push([None, None]);
            }
        }
        if let Some((a, b)) = swap {
            swap_tiles(&mut style.layout, a, b);
        }
        if let Some((from, at)) = insert {
            insert_tile(&mut style.layout, from, at);
        }
    }

    fn log(&mut self, ui: &mut egui::Ui) {
        let lines: Vec<String> = state::with(|s| s.log.iter().cloned().collect());
        ui.horizontal(|ui| {
            // Row as tall as the switch, so the heading centres on it.
            ui.set_min_height(34.0);
            // Heading drawn with caps_text so its capitals line up with the switch label.
            let galley_w = ui.painter().layout_no_wrap(tr("LOG").into(), cond(15.0), MUTED).size().x;
            let (r, _) = ui.allocate_exact_size(vec2(galley_w + 4.0, 34.0), Sense::hover());
            caps_text(ui.painter(), r.left_center(), egui::Align::Min, tr("LOG"), 15.0, MUTED);
            ui.add_space(12.0);
            // Troubleshooting switch next to the log it fills; saved right away.
            if toggle(ui, &mut self.saved.debug, tr("Log GSPro messages")).changed() {
                self.edit.debug = self.saved.debug;
                openlaunch_connect::gspro::DEBUG.store(self.saved.debug, std::sync::atomic::Ordering::Relaxed);
                if let Err(e) = self.saved.save() {
                    log::info(&format!("Could not save settings: {e}"));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(120.0, 26.0), Sense::hover());
                // For bug reports: the whole log with version, ready to paste.
                if button(ui, rect, tr("COPY"), Icon::None, false, !lines.is_empty(), tr("The log is empty")) {
                    let header = format!("{APP_NAME} {} ({})\n", env!("CARGO_PKG_VERSION"), std::env::consts::OS);
                    ui.ctx().copy_text(header + &lines.join("\n"));
                }
            });
        });
        egui::Frame::new().fill(TILE).inner_margin(Margin::same(10)).show(ui, |ui| {
            egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for line in lines {
                        ui.label(RichText::new(line).monospace().size(12.0).color(SOFT));
                    }
                });
        });
    }
}

impl eframe::App for App {
    /// While every window of the app is covered (the main window by GSPro and, for a moment, the
    /// overlays), eframe skips `ui` – and egui would close the overlays left out of that pass. So
    /// they are kept open here then, without drawing: a covered window isn't painted anyway, and
    /// text drawn in such a pass would add letters to the font texture that eframe never uploads.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.all_covered = !ctx.input(|i| i.raw.viewports.values().any(|v| v.visible().unwrap_or(true)));
        if self.all_covered {
            self.overlay(ctx);
            self.tile_overlays(ctx);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if !self.all_covered {
            self.overlay(ui.ctx());
            self.tile_overlays(ui.ctx());
        }
        egui::CentralPanel::default().frame(egui::Frame::new().fill(PAGE)).show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            self.top_bar(ui);
            egui::Frame::new().inner_margin(Margin { left: 18, right: 18, top: 14, bottom: 14 }).show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
                match self.page {
                    Page::Play => self.play(ui),
                    Page::Shots => self.shots(ui),
                    Page::Settings => self.settings(ui),
                    Page::Overlay => self.overlay_page(ui),
                    Page::Log => self.log(ui),
                }
            });
        });
        self.capture.tick(ui.ctx(), self.saved.overlay != OverlayPos::Off);
        let scale = self.saved.ui_scale;
        let old_zoom = ui.ctx().zoom_factor();
        let logical = apply_window_zoom(ui.ctx(), scale);
        if self.applied_scale != Some(scale) {
            if self.applied_scale.is_some() {
                let min = min_size(scale);
                let room = screen_room(ui.ctx());
                let size = resized_window(logical, old_zoom, scale, room);
                log::info(&format!(
                    "Interface size {}: window {:.0}×{:.0} → {:.0}×{:.0} (screen room {})",
                    scale.map_or("auto".to_string(), |s| format!("{:.0} %", s * 100.0)),
                    logical.x,
                    logical.y,
                    size.x,
                    size.y,
                    room.map_or("unknown".to_string(), |r| format!("{:.0}×{:.0}", r.x, r.y)),
                ));
                set_window_size(ui.ctx(), size, min, scale.unwrap_or(old_zoom));
            }
            self.applied_scale = Some(scale);
        }
        #[cfg(target_os = "macos")]
        {
            // A fixed interface size leaves the window free; fitting keeps the base shape.
            let lock = scale.is_none() && !ui.ctx().input(|i| i.viewport().fullscreen).unwrap_or(false);
            if self.aspect_locked != Some(lock) && mac::lock_aspect_ratio(APP_NAME, lock.then_some(BASE_SIZE)) {
                self.aspect_locked = Some(lock);
            }
        }
        if self.remember_size {
            self.remember_window_size(logical);
        }
        ui.ctx().request_repaint_after(Duration::from_millis(250));
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // Fully transparent so the overlay's rounded corners show what's behind it.
        [0.0; 4]
    }
}

/// Zoom that fits the base layout into a window of `logical` size (points at zoom 1):
/// the smaller of the two ratios, never below 1, rounded down to 1 %.
fn fit_zoom(logical: egui::Vec2) -> f32 {
    let z = (logical.x / BASE_SIZE.x).min(logical.y / BASE_SIZE.y).max(1.0);
    (z * 100.0).floor() / 100.0
}

/// Zoom the whole UI to the window size, or to the fixed interface size `scale`; returns the
/// window size at zoom 1.
fn apply_window_zoom(ctx: &egui::Context, scale: Option<f32>) -> egui::Vec2 {
    let zoom = ctx.zoom_factor();
    // Content size is in points (already zoomed); times the zoom gives a zoom-independent size.
    let logical = ctx.content_rect().size() * zoom;
    let fit = scale.unwrap_or_else(|| fit_zoom(logical));
    if (fit - zoom).abs() >= 0.01 {
        ctx.set_zoom_factor(fit);
    }
    logical
}

/// Smallest window (at zoom 1) that holds the base layout at the interface size `scale`.
fn min_size(scale: Option<f32>) -> egui::Vec2 {
    BASE_SIZE * scale.unwrap_or(1.0)
}

/// Window size on start: the saved one, at the base aspect ratio when the interface fits the
/// window (saved sizes from a fixed interface size or from before the ratio was fixed may differ).
fn start_size(saved: Option<[f32; 2]>, scale: Option<f32>) -> egui::Vec2 {
    let min = min_size(scale);
    match (saved, scale) {
        (None, _) => min,
        (Some([w, _]), None) => vec2(w, window_height(w)),
        (Some([w, h]), Some(_)) => vec2(w, h).max(min),
    }
}

/// Window size after switching the interface size to `scale`: the window grows or shrinks with
/// the interface (from `zoom`), within the minimum for `scale` and the room on the screen. Back to
/// fitting, it takes the base shape at the current zoom.
fn resized_window(logical: egui::Vec2, zoom: f32, scale: Option<f32>, room: Option<egui::Vec2>) -> egui::Vec2 {
    let room = room.unwrap_or(egui::Vec2::INFINITY);
    match scale {
        Some(s) => (logical * (s / zoom)).min(room).max(min_size(scale)),
        None => {
            let w = (BASE_SIZE.x * zoom).min(room.x).min(room.y * BASE_SIZE.x / BASE_SIZE.y).max(BASE_SIZE.x).round();
            vec2(w, window_height(w))
        }
    }
}

/// Resize the main window to `size` with the minimum `min` (both at zoom 1). egui's viewport
/// commands take points at the zoom `zoom` the window will have; on macOS AppKit is told
/// directly, keeping the window's top edge in place.
fn set_window_size(ctx: &egui::Context, size: egui::Vec2, min: egui::Vec2, zoom: f32) {
    #[cfg(target_os = "macos")]
    if mac::set_content_size(APP_NAME, size, min) {
        return;
    }
    ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(min / zoom));
    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size / zoom));
}

/// Largest window content on the current screen (at zoom 1): without the menu bar, the Dock and
/// the title bar on macOS.
fn screen_room(ctx: &egui::Context) -> Option<egui::Vec2> {
    #[cfg(target_os = "macos")]
    if let Some(room) = mac::max_content_size(APP_NAME) {
        return Some(room);
    }
    ctx.input(|i| i.viewport().monitor_size).map(|m| m * ctx.zoom_factor())
}

/// Window height for a width at the base aspect ratio, which the window keeps: the zoom
/// follows the smaller ratio, so any other shape would only add empty space.
fn window_height(width: f32) -> f32 {
    (width * BASE_SIZE.y / BASE_SIZE.x).round()
}

impl App {
    /// Save the window size once it has been stable for a second (not on every resize step).
    fn remember_window_size(&mut self, logical: egui::Vec2) {
        let size = [logical.x.round(), logical.y.round()];
        if self.saved.window_size == Some(size) {
            self.size_changed = None;
            return;
        }
        let since = *self.size_changed.get_or_insert_with(Instant::now);
        if since.elapsed() >= Duration::from_secs(1) {
            self.size_changed = None;
            self.saved.window_size = Some(size);
            // Keep unsaved edits on the settings page apart from this.
            self.edit.window_size = Some(size);
            if let Err(e) = self.saved.save() {
                log::info(&format!("Could not save the window size: {e}"));
            }
        }
    }
}

fn overlay_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("overlay")
}

fn tile_overlay_id(k: usize) -> egui::ViewportId {
    egui::ViewportId::from_hash_of(("tile-overlay", k))
}

/// Window title of tile overlay `k` (AppKit finds the window by it).
fn tile_overlay_title(k: usize) -> String {
    format!("{TILE_OVERLAY_TITLE} {}", k + 1)
}

/// Layer the overlay draws on, scaled down by the main window's zoom.
fn overlay_layer() -> egui::LayerId {
    egui::LayerId::new(egui::Order::Middle, egui::Id::new("overlay-content"))
}

/// Screenshot mode for the README: `OPENLAUNCH_SCREENSHOT=<file.png>` saves the main window,
/// `OPENLAUNCH_SCREENSHOT_OVERLAY=<file.png>` the overlay; the app quits once all are written.
/// Uses egui's own renderer, so no macOS screen recording permission is needed.
struct Capture {
    main: Option<PathBuf>,
    overlay: Option<PathBuf>,
    requested: bool,
    started: Instant,
}

impl Capture {
    /// Time for the demo shot to arrive before capturing.
    const DELAY: Duration = Duration::from_millis(3500);

    fn from_env() -> Self {
        let path = |key| std::env::var_os(key).map(PathBuf::from);
        Self {
            main: path("OPENLAUNCH_SCREENSHOT"),
            overlay: path("OPENLAUNCH_SCREENSHOT_OVERLAY"),
            requested: false,
            started: Instant::now(),
        }
    }

    fn pending(&self) -> bool {
        self.main.is_some() || self.overlay.is_some()
    }

    /// Called once per frame from the main window.
    fn tick(&mut self, ctx: &egui::Context, overlay_shown: bool) {
        if !self.pending() {
            return;
        }
        if !overlay_shown && self.overlay.take().is_some() {
            log::info("Screenshot: overlay is off (Settings) – skipping it.");
        }
        if !self.requested && self.started.elapsed() < Self::DELAY {
            // eframe only captures visible windows; a window started from a terminal may be covered.
            ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Focus);
        }
        if !self.requested && self.started.elapsed() >= Self::DELAY {
            self.requested = true;
            let cmd = || egui::ViewportCommand::Screenshot(egui::UserData::default());
            if self.main.is_some() {
                ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, cmd());
            }
            if self.overlay.is_some() {
                ctx.send_viewport_cmd_to(overlay_id(), cmd());
            }
        }
        self.save_from(ctx);
        if self.requested && !self.pending() {
            ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
        }
    }

    /// Write screenshots that arrived in this viewport's input.
    fn save_from(&mut self, ctx: &egui::Context) {
        let shots: Vec<_> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Screenshot { viewport_id, image, .. } => Some((*viewport_id, image.clone())),
                    _ => None,
                })
                .collect()
        });
        for (id, image) in shots {
            let target = if id == egui::ViewportId::ROOT { self.main.take() } else { self.overlay.take() };
            let Some(path) = target else { continue };
            let [w, h] = image.size;
            let rgba: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
            let result = image::RgbaImage::from_raw(w as u32, h as u32, rgba)
                .ok_or_else(|| "bad image size".to_string())
                .and_then(|img| img.save(&path).map_err(|e| e.to_string()));
            match result {
                Ok(()) => log::info(&format!("Screenshot saved: {}", path.display())),
                Err(e) => log::info(&format!("Screenshot failed ({}): {e}", path.display())),
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2::MainThreadMarker;
    use objc2::runtime::AnyObject;
    use objc2_app_kit::{
        NSApplication, NSNormalWindowLevel, NSScreenSaverWindowLevel, NSWindowCollectionBehavior,
        NSWindowOrderingMode, NSWorkspace, NSWorkspaceDidActivateApplicationNotification,
    };
    use objc2_core_foundation::CFArray;
    use objc2_core_graphics::{CGWindowListCopyWindowInfo, CGWindowListOption};
    use std::ptr::NonNull;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};

    use block2::RcBlock;
    use objc2_app_kit::{NSWindow, NSWindowDidEndLiveResizeNotification, NSWindowWillStartLiveResizeNotification};
    use objc2_foundation::{NSArray, NSDictionary, NSNotification, NSNotificationCenter, NSNumber, NSSize, NSString, NSTimer};

    /// Aspect ratio the main window keeps while resizing (`None`: none, e.g. in full screen).
    static ASPECT: Mutex<Option<(f64, f64)>> = Mutex::new(None);

    /// Middle of GSPro's window when last seen on screen (window-server coordinates: points from
    /// the top left of the first screen).
    static GSPRO_AT: Mutex<Option<(f64, f64)>> = Mutex::new(None);

    /// The screen the overlays are on: GSPro's (e.g. a projector), else the main window's.
    fn overlay_screen(mtm: MainThreadMarker) -> Option<objc2::rc::Retained<objc2_app_kit::NSScreen>> {
        use objc2_app_kit::NSScreen;
        let screens = NSScreen::screens(mtm);
        let gspro_at = *GSPRO_AT.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(i) = gspro_at.and_then(|at| screen_of(mtm, at)) {
            return Some(screens.objectAtIndex(i));
        }
        let main_title = NSString::from_str(super::APP_NAME);
        let windows = NSApplication::sharedApplication(mtm).windows();
        windows.iter().find(|w| w.title().isEqualToString(&main_title)).and_then(|w| w.screen()).or_else(|| NSScreen::mainScreen(mtm))
    }

    /// Put the content of the window titled `title` at `top_left` (points from the main screen's
    /// top left corner) with `size` (points), if it isn't there already.
    pub fn place_window(title: &str, top_left: eframe::egui::Pos2, size: eframe::egui::Vec2) {
        use objc2_foundation::{NSPoint, NSRect};
        let Some(mtm) = MainThreadMarker::new() else { return };
        let Some(screen) = overlay_screen(mtm) else { return };
        let full = screen.frame();
        let title = NSString::from_str(title);
        let windows = NSApplication::sharedApplication(mtm).windows();
        let Some(window) = windows.iter().find(|w| w.title().isEqualToString(&title)) else { return };
        // AppKit counts from the bottom left.
        let content = NSRect::new(
            NSPoint::new(full.origin.x + f64::from(top_left.x), full.origin.y + full.size.height - f64::from(top_left.y) - f64::from(size.y)),
            NSSize::new(size.x.into(), size.y.into()),
        );
        let frame = window.frameRectForContentRect(content);
        let now = window.frame();
        let off = (now.origin.x - frame.origin.x).abs()
            + (now.origin.y - frame.origin.y).abs()
            + (now.size.width - frame.size.width).abs()
            + (now.size.height - frame.size.height).abs();
        if off > 0.5 {
            window.setFrame_display(frame, true);
        }
    }

    /// The mouse on the overlays' screen, in points from its top left corner.
    pub fn mouse_location() -> Option<eframe::egui::Pos2> {
        use objc2_app_kit::NSEvent;
        let mtm = MainThreadMarker::new()?;
        let full = overlay_screen(mtm)?.frame();
        let at = NSEvent::mouseLocation();
        // AppKit counts from the bottom left.
        Some(eframe::egui::pos2((at.x - full.origin.x) as f32, (full.origin.y + full.size.height - at.y) as f32))
    }

    /// The overlays' screen without the menu bar and the Dock, in points from its top left corner.
    pub fn visible_frame() -> Option<eframe::egui::Rect> {
        let mtm = MainThreadMarker::new()?;
        let screen = overlay_screen(mtm)?;
        let (full, visible) = (screen.frame(), screen.visibleFrame());
        // AppKit counts from the bottom left.
        let top = (full.origin.y + full.size.height) - (visible.origin.y + visible.size.height);
        let left = visible.origin.x - full.origin.x;
        Some(eframe::egui::Rect::from_min_size(
            eframe::egui::pos2(left as f32, top as f32),
            eframe::egui::vec2(visible.size.width as f32, visible.size.height as f32),
        ))
    }

    /// Largest content size the window titled `title` can have on its screen: the visible frame
    /// (without menu bar and Dock) minus the title bar, in points. `None` before the window exists.
    pub fn max_content_size(title: &str) -> Option<eframe::egui::Vec2> {
        let mtm = MainThreadMarker::new()?;
        let title = NSString::from_str(title);
        let windows = NSApplication::sharedApplication(mtm).windows();
        let window = windows.iter().find(|w| w.title().isEqualToString(&title))?;
        let visible = window.screen()?.visibleFrame();
        let frame = window.frame();
        let title_bar = frame.size.height - window.contentRectForFrameRect(frame).size.height;
        Some(eframe::egui::vec2(visible.size.width as f32, (visible.size.height - title_bar) as f32))
    }

    /// Set the content size and minimum content size (points) of the window titled `title`,
    /// keeping its top edge and the window on the screen. False before the window exists.
    pub fn set_content_size(title: &str, size: eframe::egui::Vec2, min: eframe::egui::Vec2) -> bool {
        let Some(mtm) = MainThreadMarker::new() else {
            return false;
        };
        let title = NSString::from_str(title);
        let windows = NSApplication::sharedApplication(mtm).windows();
        let Some(window) = windows.iter().find(|w| w.title().isEqualToString(&title)) else {
            return false;
        };
        window.setContentMinSize(NSSize::new(min.x.into(), min.y.into()));
        let frame = window.frame();
        let mut content = window.contentRectForFrameRect(frame);
        content.size = NSSize::new(size.x.into(), size.y.into());
        let mut new = window.frameRectForContentRect(content);
        // AppKit's origin is bottom-left: keep the top edge where it was.
        new.origin.y = frame.origin.y + frame.size.height - new.size.height;
        if let Some(screen) = window.screen() {
            let v = screen.visibleFrame();
            new.origin.x = new.origin.x.min(v.origin.x + v.size.width - new.size.width).max(v.origin.x);
            new.origin.y = new.origin.y.max(v.origin.y).min(v.origin.y + v.size.height - new.size.height);
        }
        window.setFrame_display(new, true);
        true
    }

    /// Let the window be resized only in the given aspect ratio (`None`: freely). macOS applies
    /// it during the drag itself, which is smooth; correcting the size afterwards makes the
    /// window jump. Returns false while the window doesn't exist yet.
    pub fn lock_aspect_ratio(title: &str, ratio: Option<eframe::egui::Vec2>) -> bool {
        static OBSERVING: AtomicBool = AtomicBool::new(false);
        let Some(mtm) = MainThreadMarker::new() else {
            return false;
        };
        let title = NSString::from_str(title);
        let windows = NSApplication::sharedApplication(mtm).windows();
        let Some(window) = windows.iter().find(|w| w.title().isEqualToString(&title)) else {
            return false;
        };
        *ASPECT.lock().unwrap() = ratio.map(|r| (r.x.into(), r.y.into()));
        apply_aspect(&window);
        // winit resets the content resize increments when a live resize starts and ends, which
        // clears the aspect ratio; set it again right after (observers run in the order added).
        if !OBSERVING.swap(true, Ordering::Relaxed) {
            let center = NSNotificationCenter::defaultCenter();
            for name in unsafe { [NSWindowWillStartLiveResizeNotification, NSWindowDidEndLiveResizeNotification] } {
                let w = window.clone();
                let block = RcBlock::new(move |_: NonNull<NSNotification>| apply_aspect(&w));
                let token = unsafe { center.addObserverForName_object_queue_usingBlock(Some(name), Some(&window), None, &block) };
                // Observes for the lifetime of the app.
                std::mem::forget(token);
            }
        }
        true
    }

    fn apply_aspect(window: &NSWindow) {
        match *ASPECT.lock().unwrap() {
            Some((w, h)) => window.setContentAspectRatio(NSSize::new(w, h)),
            // Setting resize increments is how AppKit clears an aspect ratio.
            None => window.setContentResizeIncrements(NSSize::new(1.0, 1.0)),
        }
    }

    /// Keep the window above full-screen apps: show it on every Space, including the
    /// separate Space of a full-screen app, and place it for the app in front (see
    /// [`arrange_overlays`]). `moving`: an overlay is being moved, so they all stay on top.
    pub fn float_over_fullscreen(ctx: &eframe::egui::Context, title: &str, moving: bool) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        CONTEXT.get_or_init(|| ctx.clone());
        let title = NSString::from_str(title);
        let behavior = NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle;
        for window in NSApplication::sharedApplication(mtm).windows() {
            if window.title().isEqualToString(&title) && window.collectionBehavior() != behavior {
                window.setCollectionBehavior(behavior);
            }
        }
        MOVING.store(moving, Ordering::Relaxed);
        if !WATCHING_APPS.swap(true, Ordering::Relaxed) {
            // Re-arranged when another app comes to the front, also while this one doesn't repaint.
            let workspace = NSWorkspace::sharedWorkspace();
            let block = RcBlock::new(|_: NonNull<NSNotification>| arrange_overlays(true));
            let token = unsafe {
                workspace.notificationCenter().addObserverForName_object_queue_usingBlock(
                    Some(NSWorkspaceDidActivateApplicationNotification),
                    None,
                    None,
                    &block,
                )
            };
            // Observes for the lifetime of the app.
            std::mem::forget(token);
            // And checked four times a second: right after a switch the window list may not show
            // the new order yet. The tick also keeps the overlays painted: egui stops painting
            // while all its windows are covered (the main window by GSPro, the overlays by the
            // main window) and doesn't start again when one of them shows.
            let tick = RcBlock::new(|_: NonNull<NSTimer>| {
                arrange_overlays(false);
                if let Some(ctx) = CONTEXT.get() {
                    ctx.request_repaint();
                }
            });
            let timer = unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(0.25, true, &tick) };
            std::mem::forget(timer);
        }
        arrange_overlays(false);
    }

    /// An overlay is being moved.
    static MOVING: AtomicBool = AtomicBool::new(false);
    /// To repaint the overlays when they show again: a window that was hidden (or covered) while
    /// the app didn't repaint comes back empty.
    static CONTEXT: std::sync::OnceLock<eframe::egui::Context> = std::sync::OnceLock::new();
    static WATCHING_APPS: AtomicBool = AtomicBool::new(false);
    /// The app in front when the overlays were last arranged.
    static ARRANGED_FOR: Mutex<Option<Front>> = Mutex::new(None);

    /// The app in front, as far as the overlays care.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Front {
        Ours,
        Gspro,
        Other,
    }

    /// GSPro's processes under CrossOver: "GSPro.exe", and "GSPconnect.exe" for its Connect window.
    fn is_gspro(name: &str) -> bool {
        name.to_lowercase().starts_with("gsp")
    }

    /// A normal window of another app on screen.
    struct OtherWindow {
        owner: String,
        number: isize,
        /// Its middle in window-server coordinates.
        middle: Option<(f64, f64)>,
    }

    /// The normal windows on screen of other apps, front to back.
    fn other_windows() -> Vec<OtherWindow> {
        let options = CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements;
        let Some(list) = CGWindowListCopyWindowInfo(options, 0) else { return Vec::new() };
        // A CFArray of CFDictionaries; toll-free bridged to Foundation.
        let list: &NSArray<NSDictionary<NSString, AnyObject>> = unsafe { &*std::ptr::from_ref::<CFArray>(&list).cast() };
        let get = |info: &NSDictionary<NSString, AnyObject>, key: &str| info.objectForKey(&NSString::from_str(key));
        let number = |info: &NSDictionary<NSString, AnyObject>, key: &str| {
            get(info, key).and_then(|o| o.downcast::<NSNumber>().ok()).map(|n| n.integerValue())
        };
        let own = isize::try_from(std::process::id()).unwrap_or(-1);
        list.iter()
            .filter(|info| number(info, "kCGWindowLayer") == Some(0) && number(info, "kCGWindowOwnerPID") != Some(own))
            .filter_map(|info| {
                let owner = get(&info, "kCGWindowOwnerName").and_then(|o| o.downcast::<NSString>().ok())?.to_string();
                let bounds = get(&info, "kCGWindowBounds").and_then(|o| o.downcast::<NSDictionary>().ok());
                let middle = bounds.and_then(|b| {
                    let v = |k: &str| b.objectForKey(&NSString::from_str(k)).and_then(|o| o.downcast::<NSNumber>().ok()).map(|n| n.doubleValue());
                    Some((v("X")? + v("Width")? / 2.0, v("Y")? + v("Height")? / 2.0))
                });
                Some(OtherWindow { owner, number: number(&info, "kCGWindowNumber")?, middle })
            })
            .collect()
    }

    /// The screen (its index) a point in window-server coordinates is on.
    fn screen_of(mtm: MainThreadMarker, (x, y): (f64, f64)) -> Option<usize> {
        let screens = objc2_app_kit::NSScreen::screens(mtm);
        // AppKit counts from the bottom left of the first screen.
        let y = screens.firstObject()?.frame().size.height - y;
        screens.iter().position(|s| {
            let f = s.frame();
            x >= f.origin.x && x < f.origin.x + f.size.width && y >= f.origin.y && y < f.origin.y + f.size.height
        })
    }

    /// Who is in front on GSPro's screen: GSPro if it is the active app (the name macOS gives it
    /// under CrossOver isn't reliable, so also if its window is the top one there), else this app
    /// if it is active and its window is on that screen, else another app.
    fn front(mtm: MainThreadMarker) -> Front {
        let app = NSWorkspace::sharedWorkspace().frontmostApplication();
        let ours = app.as_ref().is_some_and(|a| i64::from(a.processIdentifier()) == i64::from(std::process::id()));
        let named_gspro = app.and_then(|a| a.localizedName()).is_some_and(|n| is_gspro(&n.to_string()));
        let windows = other_windows();
        // Remember where GSPro is: the overlays go to its screen.
        if let Some(at) = windows.iter().find(|w| is_gspro(&w.owner)).and_then(|w| w.middle) {
            *GSPRO_AT.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(at);
        }
        let gspro_at = *GSPRO_AT.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let gspro_screen = gspro_at.and_then(|at| screen_of(mtm, at));
        if ours {
            // This app's window covers GSPro only on GSPro's screen.
            let main_title = NSString::from_str(super::APP_NAME);
            let windows = NSApplication::sharedApplication(mtm).windows();
            let main = windows.iter().find(|w| w.title().isEqualToString(&main_title)).map(|w| w.frame());
            let main_screen = main.and_then(|f| {
                let first = objc2_app_kit::NSScreen::screens(mtm).firstObject()?.frame();
                // Its middle in window-server coordinates (from the top left of the first screen).
                let middle = (f.origin.x + f.size.width / 2.0, first.size.height - (f.origin.y + f.size.height / 2.0));
                screen_of(mtm, middle)
            });
            if gspro_screen.is_none() || main_screen == gspro_screen {
                return Front::Ours;
            }
        } else if named_gspro {
            return Front::Gspro;
        }
        // The top window on GSPro's screen (of another app) decides.
        let top_there = windows.iter().find(|w| gspro_screen.is_none() || w.middle.and_then(|m| screen_of(mtm, m)) == gspro_screen);
        match top_there {
            Some(w) if is_gspro(&w.owner) => Front::Gspro,
            _ if ours => Front::Ours,
            _ => Front::Other,
        }
    }

    /// GSPro's frontmost window on screen (its window-server number), if GSPro runs.
    fn gspro_window() -> Option<isize> {
        other_windows().into_iter().find(|w| is_gspro(&w.owner)).map(|w| w.number)
    }

    /// The overlays sit right above GSPro: on top of everything while GSPro is in front (also in
    /// full screen), else right above GSPro's window, under any window in front of it, this app's
    /// too (hidden when GSPro doesn't run). `changed`: another app came to the front.
    fn arrange_overlays(changed: bool) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let front = if MOVING.load(Ordering::Relaxed) { Front::Gspro } else { front(mtm) };
        let mut last = ARRANGED_FOR.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let reorder = changed || *last != Some(front);
        *last = Some(front);
        drop(last);
        let app = NSApplication::sharedApplication(mtm);
        let windows = app.windows();
        let overlays: Vec<_> = windows
            .iter()
            .filter(|w| {
                let t = w.title().to_string();
                t.starts_with(super::OVERLAY_TITLE) || t.starts_with(super::TILE_OVERLAY_TITLE)
            })
            .collect();
        if overlays.is_empty() {
            return;
        }
        // Where they go: their level, whether they show, and the window they go behind or above.
        let (level, shown, place) = match front {
            Front::Gspro => (NSScreenSaverWindowLevel, true, None),
            Front::Ours | Front::Other => match gspro_window().filter(|_| reorder) {
                Some(n) => (NSNormalWindowLevel, true, Some((NSWindowOrderingMode::Above, n))),
                None if reorder => (NSNormalWindowLevel, false, None),
                // Placed when that app came to the front; only new overlays still need it.
                None => (NSNormalWindowLevel, overlays.iter().any(|w| w.alphaValue() > 0.0), None),
            },
        };
        let alpha = if shown { 1.0 } else { 0.0 };
        let mut moved = false;
        for window in overlays {
            let new = window.level() != level || (window.alphaValue() - alpha).abs() > f64::EPSILON;
            if !new && !reorder {
                continue;
            }
            window.setLevel(level);
            window.setAlphaValue(alpha);
            if let Some((mode, other)) = place {
                window.orderWindow_relativeTo(mode, other);
            } else if front == Front::Gspro {
                // A higher level alone doesn't bring it back into GSPro's full-screen Space.
                window.orderFrontRegardless();
            }
            moved = true;
        }
        if moved && let Some(ctx) = CONTEXT.get() {
            ctx.request_repaint();
        }
    }
}

// ── widgets ──

fn section(ui: &mut egui::Ui, title: &str) -> egui::Response {
    ui.label(RichText::new(title).font(cond(15.0)).color(MUTED).extra_letter_spacing(1.5))
}

fn tile_row(ui: &mut egui::Ui, cells: [(Option<String>, String); 6]) {
    let gap = 3.0;
    let w = (ui.available_width() - gap * 5.0) / 6.0;
    let h = 80.0;
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    let p = ui.painter();
    for (i, (value, label)) in cells.into_iter().enumerate() {
        let r = Rect::from_min_size(pos2(row.left() + i as f32 * (w + gap), row.top()), vec2(w, h));
        p.rect_filled(r, 0.0, TILE);
        let (text, color) = match value {
            Some(v) => (v, WHITE),
            None => ("–".to_string(), MUTED),
        };
        // Capitals centred between the tile's top edge and the label.
        caps_text(p, pos2(r.center().x, r.top() + 29.75), egui::Align::Center, text, 40.0, color);
        p.text(pos2(r.center().x, r.bottom() - 15.0), Align2::CENTER_CENTER, label, FontId::proportional(13.5), SOFT);
    }
}

/// Width of the label column on the settings page.
const SETTING_LABEL_W: f32 = 92.0;

/// A settings card: tile with border, icon + condensed heading, divider; `add` fills the body.
fn card(ui: &mut egui::Ui, rect: Rect, title: &str, icon: Icon, add: impl FnOnce(&mut egui::Ui)) {
    let p = ui.painter();
    p.rect(rect, 6.0, TILE, Stroke::new(1.0, LINE), StrokeKind::Inside);
    let head_y = rect.top() + 21.0;
    draw_icon(p, pos2(rect.left() + 24.0, head_y), icon, ACCENT);
    let mut job = egui::text::LayoutJob::single_section(
        title.to_string(),
        egui::TextFormat { font_id: cond(15.0), color: LABEL, extra_letter_spacing: 1.5, ..Default::default() },
    );
    job.wrap.max_rows = 1;
    let galley = p.layout_job(job);
    let y = head_y - galley.size().y / 2.0 + 15.0 * CAPS_OFFSET;
    p.galley(pos2(rect.left() + 38.0, y), galley, LABEL);
    p.line_segment([pos2(rect.left() + 16.0, rect.top() + 38.0), pos2(rect.right() - 16.0, rect.top() + 38.0)], Stroke::new(1.0, LINE));
    let body = Rect::from_min_max(pos2(rect.left() + 16.0, rect.top() + 48.0), pos2(rect.right() - 16.0, rect.bottom() - 10.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(body).layout(egui::Layout::top_down(egui::Align::Min)), |ui| {
        let sp = ui.spacing_mut();
        sp.item_spacing = vec2(12.0, 10.0);
        sp.interact_size.y = 34.0;
        sp.button_padding = vec2(14.0, 8.0);
        // Inputs darker than the card, raised controls like the buttons.
        let v = ui.visuals_mut();
        v.extreme_bg_color = PAGE;
        v.widgets.inactive.bg_fill = BUTTON;
        v.widgets.inactive.weak_bg_fill = BUTTON;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, FIELD_LINE);
        v.widgets.hovered.bg_fill = BUTTON_HOVER;
        v.widgets.hovered.weak_bg_fill = BUTTON_HOVER;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
        v.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
        v.selection.stroke = Stroke::new(1.0, ACCENT);
        for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
            w.corner_radius = CornerRadius::same(5);
        }
        add(ui);
    });
}

/// Fixed-width row label, vertically centred on a row of height `h`.
fn setting_label(ui: &mut egui::Ui, label: &str, h: f32) {
    let (r, _) = ui.allocate_exact_size(vec2(SETTING_LABEL_W, h), Sense::hover());
    ui.painter().text(r.left_center(), Align2::LEFT_CENTER, label, FontId::proportional(13.0), LABEL);
}

fn setting_row(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    // Its own id, so equal options in two rows (e.g. "Mittel") don't clash.
    ui.push_id(label, |ui| {
        ui.horizontal(|ui| {
            ui.set_min_height(34.0);
            setting_label(ui, label, 34.0);
            add(ui);
        });
    });
}

/// Secondary button inside a settings card (34 px, raised like the main buttons).
fn secondary_button(label: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(label).size(13.0).strong().color(SOFT)).min_size(vec2(0.0, 34.0))
}

/// Read-only value in an input-like field (e.g. the launcher path), truncated with a tooltip.
fn read_only_field(ui: &mut egui::Ui, width: f32, text: &str) {
    let (r, resp) = ui.allocate_exact_size(vec2(width, 34.0), Sense::hover());
    ui.painter().rect(r, 5.0, PAGE, Stroke::new(1.0, FIELD_LINE), StrokeKind::Inside);
    let inner = r.shrink2(vec2(10.0, 0.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
        ui.add(egui::Label::new(RichText::new(text).size(13.0).color(SOFT)).truncate());
    });
    resp.on_hover_text(text);
}

/// Switch with a label to its right; clicking either toggles. Returns the response (`changed()`).
fn toggle(ui: &mut egui::Ui, on: &mut bool, label: &str) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(14.0), SOFT);
    let size = vec2(38.0 + 10.0 + galley.size().x, 34.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool(resp.id, *on);
    let track = Rect::from_min_size(pos2(rect.left(), rect.center().y - 11.0), vec2(38.0, 22.0));
    let p = ui.painter();
    let fill = if *on { ACCENT } else { FIELD_LINE };
    p.rect(track, 11.0, fill, Stroke::new(1.0, if resp.hovered() { ACCENT_HOVER } else { fill }), StrokeKind::Inside);
    let x = egui::lerp(track.left() + 11.0..=track.right() - 11.0, t);
    p.circle_filled(pos2(x, track.center().y), 8.0, if *on { WHITE } else { SOFT });
    p.galley(pos2(track.right() + 10.0, rect.center().y - galley.size().y / 2.0), galley, SOFT);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Two equal segments (left = `true`), e.g. km/h | mph.
fn segmented(ui: &mut egui::Ui, left_on: &mut bool, left: &str, right: &str) {
    segments(ui, left_on, &[(true, left), (false, right)]);
}

/// Equal segments, one per option; the chosen one is filled. Returns true when it changed.
fn segments<T: PartialEq + Copy>(ui: &mut egui::Ui, value: &mut T, options: &[(T, &str)]) -> bool {
    segments_where(ui, value, options, |_| true, "")
}

/// Like `segments`; options that aren't `enabled` are greyed out and explain why on hover.
fn segments_where<T: PartialEq + Copy>(
    ui: &mut egui::Ui,
    value: &mut T,
    options: &[(T, &str)],
    enabled: impl Fn(T) -> bool,
    why_disabled: &str,
) -> bool {
    let text_w = options
        .iter()
        .map(|(_, label)| ui.painter().layout_no_wrap((*label).to_string(), FontId::proportional(13.0), WHITE).size().x)
        .fold(0.0, f32::max);
    let seg = (text_w + 24.0).max(56.0).ceil();
    let (rect, _) = ui.allocate_exact_size(vec2(options.len() as f32 * seg, 34.0), Sense::hover());
    ui.painter().rect(rect, 5.0, PAGE, Stroke::new(1.0, FIELD_LINE), StrokeKind::Inside);
    let mut changed = false;
    for (i, &(option, label)) in options.iter().enumerate() {
        let r = Rect::from_min_size(rect.min + vec2(i as f32 * seg, 0.0), vec2(seg, 34.0));
        let on = enabled(option);
        let resp = ui.interact(r, ui.id().with(("seg", label)), if on { Sense::click() } else { Sense::hover() });
        if resp.clicked() && *value != option {
            *value = option;
            changed = true;
        }
        let selected = *value == option;
        let p = ui.painter();
        if selected {
            let radius = CornerRadius {
                nw: if i == 0 { 5 } else { 0 },
                sw: if i == 0 { 5 } else { 0 },
                ne: if i + 1 == options.len() { 5 } else { 0 },
                se: if i + 1 == options.len() { 5 } else { 0 },
            };
            p.rect_filled(r.shrink(1.0), radius, ACCENT);
        } else if i > 0 && options[i - 1].0 != *value {
            p.line_segment([r.left_top() + vec2(0.0, 1.0), r.left_bottom() - vec2(0.0, 1.0)], Stroke::new(1.0, FIELD_LINE));
        }
        let color = if selected { WHITE } else if !on { FIELD_LINE } else if resp.hovered() { SOFT } else { MUTED };
        p.text(r.center(), Align2::CENTER_CENTER, label, FontId::proportional(13.0), color);
        if on {
            resp.on_hover_cursor(egui::CursorIcon::PointingHand);
        } else {
            resp.on_hover_text(why_disabled);
        }
    }
    changed
}

/// [−] value unit [+] in one field.
fn stepper(ui: &mut egui::Ui, value: &mut f32, range: std::ops::RangeInclusive<f32>, step: f32, unit: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(140.0, 34.0), Sense::hover());
    ui.painter().rect(rect, 5.0, PAGE, Stroke::new(1.0, FIELD_LINE), StrokeKind::Inside);
    for (i, sign) in [-1.0_f32, 1.0].into_iter().enumerate() {
        let r = if i == 0 {
            Rect::from_min_size(rect.min, vec2(32.0, 34.0))
        } else {
            Rect::from_min_size(pos2(rect.right() - 32.0, rect.top()), vec2(32.0, 34.0))
        };
        let b = ui.interact(r, ui.id().with(("step", i)), Sense::click());
        if b.clicked() {
            *value = ((*value + sign * step) * 100.0).round() / 100.0;
            *value = value.clamp(*range.start(), *range.end());
        }
        let radius = if i == 0 {
            CornerRadius { nw: 5, sw: 5, ne: 0, se: 0 }
        } else {
            CornerRadius { nw: 0, sw: 0, ne: 5, se: 5 }
        };
        let p = ui.painter();
        p.rect_filled(r.shrink(1.0), radius, if b.hovered() { BUTTON_HOVER } else { BUTTON });
        p.text(r.center(), Align2::CENTER_CENTER, if i == 0 { "−" } else { "+" }, FontId::proportional(16.0), WHITE);
        b.on_hover_cursor(egui::CursorIcon::PointingHand);
    }
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, format!("{value:.2} {unit}"), FontId::proportional(14.0), WHITE);
    resp
}

/// Small line icons drawn with the painter (no icon font needed).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Icon {
    None,
    Clock,
    Play,
    Monitor,
    Eye,
    Signal,
    Bell,
    Layout,
    List,
}

fn draw_icon(p: &egui::Painter, c: egui::Pos2, icon: Icon, color: Color32) {
    let s = Stroke::new(1.7, color);
    let arc = |center: egui::Pos2, r: f32, from: f32, to: f32| -> Vec<egui::Pos2> {
        (0..=16).map(|i| {
            let a = (from + (to - from) * i as f32 / 16.0).to_radians();
            center + vec2(a.cos(), a.sin()) * r
        }).collect()
    };
    match icon {
        Icon::None => {}
        Icon::Clock => {
            p.circle_stroke(c, 7.5, s);
            p.line_segment([c, c + vec2(0.0, -4.5)], s);
            p.line_segment([c, c + vec2(3.2, 2.2)], s);
        }
        Icon::Play => {
            p.add(egui::Shape::convex_polygon(vec![c + vec2(-4.0, -6.0), c + vec2(6.5, 0.0), c + vec2(-4.0, 6.0)], color, Stroke::NONE));
        }
        Icon::Monitor => {
            p.rect_stroke(Rect::from_center_size(c - vec2(0.0, 1.5), vec2(16.0, 10.5)), 2.0, s, StrokeKind::Middle);
            p.line_segment([c + vec2(0.0, 3.75), c + vec2(0.0, 6.5)], s);
            p.line_segment([c + vec2(-4.0, 6.5), c + vec2(4.0, 6.5)], s);
        }
        Icon::Eye => {
            let mut top = arc(c + vec2(0.0, 6.0), 10.0, 217.0, 323.0);
            let bottom = arc(c - vec2(0.0, 6.0), 10.0, 37.0, 143.0);
            top.extend(bottom);
            p.add(egui::Shape::closed_line(top, s));
            p.circle_stroke(c, 2.5, s);
        }
        Icon::Signal => {
            let base = c + vec2(0.0, 4.0);
            p.add(egui::Shape::line(arc(base, 8.0, 205.0, 335.0), s));
            p.add(egui::Shape::line(arc(base, 4.5, 215.0, 325.0), s));
            p.circle_filled(base, 1.8, color);
        }
        Icon::Bell => {
            let pts = [(-5.5, 3.0), (-5.0, -1.5), (-3.0, -5.0), (0.0, -6.0), (3.0, -5.0), (5.0, -1.5), (5.5, 3.0), (7.0, 4.5), (-7.0, 4.5)];
            p.add(egui::Shape::closed_line(pts.iter().map(|&(x, y)| c + vec2(x, y)).collect(), s));
            p.circle_filled(c + vec2(0.0, 6.8), 1.5, color);
        }
        Icon::Layout => {
            p.rect_stroke(Rect::from_center_size(c, vec2(17.0, 12.0)), 2.0, s, StrokeKind::Middle);
            p.rect_filled(Rect::from_min_size(c + vec2(1.5, -3.8), vec2(5.0, 3.5)), 1.0, color);
        }
        Icon::List => {
            for (dy, w) in [(-4.5, 14.0), (0.0, 14.0), (4.5, 9.0)] {
                p.line_segment([c + vec2(-7.0, dy), c + vec2(w - 7.0, dy)], s);
            }
        }
    }
}

/// Raised, rounded button with condensed caps and an optional icon. A disabled button keeps
/// its shape, dimmed, and explains itself in a tooltip. Returns true when clicked.
fn button(ui: &mut egui::Ui, rect: Rect, label: &str, icon: Icon, primary: bool, enabled: bool, disabled_hint: &str) -> bool {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let resp = ui.interact(rect, ui.id().with(("button", label)), sense);
    let hovered = enabled && resp.hovered();
    let pressed = enabled && resp.is_pointer_button_down_on();
    let r = if pressed { rect.translate(vec2(0.0, 1.0)) } else { rect };
    let dim = |c: Color32| if enabled { c } else { c.gamma_multiply(0.45) };
    let (fill, border) = match (primary, hovered) {
        (true, false) => (ACCENT, ACCENT),
        (true, true) => (ACCENT_HOVER, ACCENT_HOVER),
        (false, false) => (BUTTON, BUTTON_LINE),
        (false, true) => (BUTTON_HOVER, ACCENT),
    };
    let p = ui.painter();
    if !pressed {
        // Bottom edge: makes it read as raised.
        p.rect_filled(rect.translate(vec2(0.0, 2.0)), 6.0, Color32::from_rgb(0x06, 0x0b, 0x16));
    }
    p.rect(r, 6.0, dim(fill), Stroke::new(1.0, dim(border)), StrokeKind::Inside);
    p.line_segment(
        [r.left_top() + vec2(6.0, 1.5), r.right_top() + vec2(-6.0, 1.5)],
        Stroke::new(1.0, Color32::from_white_alpha(if enabled { 24 } else { 10 })),
    );
    let text = dim(WHITE);
    let text_w = p.layout_no_wrap(label.to_string(), cond(19.0), text).size().x;
    let icon_w = if icon == Icon::None { 0.0 } else { 26.0 };
    let x0 = r.center().x - (icon_w + text_w) / 2.0;
    if icon != Icon::None {
        draw_icon(p, pos2(x0 + 8.0, r.center().y), icon, dim(if primary { WHITE } else { ICON }));
    }
    caps_text(p, pos2(x0 + icon_w, r.center().y), egui::Align::Min, label, 19.0, text);
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
    } else {
        resp.on_hover_text(disabled_hint);
        false
    }
}

/// Display name of an overlay position.
fn overlay_name(pos: OverlayPos) -> &'static str {
    match pos {
        OverlayPos::Off => tr("No overlay"),
        OverlayPos::TopLeft => tr("Top left"),
        OverlayPos::Top => tr("Top"),
        OverlayPos::TopRight => tr("Top right"),
        OverlayPos::Left => tr("Left"),
        OverlayPos::Right => tr("Right"),
        OverlayPos::BottomLeft => tr("Bottom left"),
        OverlayPos::Bottom => tr("Bottom"),
        OverlayPos::BottomRight => tr("Bottom right"),
    }
}

/// Headline status and its colour, shared by the status panel and the overlay.
fn status_word(r10: R10Status, gs_ok: bool) -> (Color32, &'static str) {
    match (r10, gs_ok) {
        (R10Status::Ready, true) => (GREEN, tr("READY")),
        (R10Status::Searching, _) => (RED, tr("NO R10")),
        (_, false) => (AMBER, tr("NO GSPRO")),
        (R10Status::Connected, true) => (AMBER, tr("NOT READY")),
    }
}

/// Every word `status_word` can give (English source texts).
const STATUS_WORDS: [&str; 4] = ["READY", "NO R10", "NO GSPRO", "NOT READY"];

/// Overlay content: status, club and battery on top; last shot (or what's wrong) below.
/// Height of the two-line overlay with the impact graphics.
const OVERLAY_IMPACT_H: f32 = 96.0;
/// Room in front of the first tile (the status colour bar) and after the last one.
const TILE_PAD_L: f32 = 18.0;
const TILE_PAD_R: f32 = 16.0;
/// Between two columns of tiles (a thin line in the middle).
const TILE_GAP: f32 = 24.0;
/// Room for a value's symbol in front of it.
const GLYPH_W: f32 = 22.0;
/// Size of the figures in a tile of double height.
const TALL_SIZE: f32 = 30.0;
/// Width of an empty place in the preview.
const EMPTY_TILE_W: f32 = 44.0;

/// What a tile shows.
#[derive(Clone, PartialEq, Debug)]
enum Cell {
    /// Dot and status word in the status colour.
    Status,
    /// A value, with its symbol in front; as wide as `wide`, the widest text it can take, so the
    /// overlay keeps its size from shot to shot.
    Value { glyph: Option<Glyph>, text: String, color: Color32, wide: String },
    /// Battery level of the R10 (`None`: not known, e.g. no R10 connected).
    Battery(Option<u8>),
    /// An impact graphic, filling its column.
    Graphic(OverlayItem),
    /// A value filling its column, in large figures.
    Tall(Box<Cell>),
}

/// What the overlay shows: the tiles (columns of two), or the status and what's wrong.
struct OverlayContent {
    color: Color32,
    word: &'static str,
    /// The tiles in columns (top, bottom); the first one holds the status.
    columns: Vec<[Option<Cell>; 2]>,
    /// Not ready or not delivered: shown on a band over the other columns, whose last values stay
    /// dimmed underneath (the overlay keeps its size).
    message: Option<(String, Color32)>,
    one_line: bool,
    opacity: f32,
    /// Angles for the impact graphics (when a shot is shown).
    impact: Option<Impact>,
    /// Shown in the overlay page's preview: empty places stay visible to put something there.
    edit: bool,
}

/// What the impact graphics show, in degrees: angle of attack and club path need club data.
#[derive(Clone, Copy)]
struct Impact {
    spin_axis: f64,
    aoa: Option<f64>,
    vla: f64,
    path: Option<f64>,
    hla: f64,
}

impl OverlayContent {
    /// From the shared status.
    fn live(style: &OverlayStyle, metric: bool, meters: bool) -> Self {
        let (r10, r10_detail, gs_ok, gs_detail, club, battery, last, to_pin) = state::with(|s| {
            let to_pin = s.gspro_result.and_then(|r| r.to_pin);
            let gspro = (s.gspro_connected, s.gspro_detail.clone());
            (s.r10, s.r10_detail.clone(), gspro.0, gspro.1, s.gspro_club.clone(), s.r10_battery, s.last_shot.clone(), to_pin)
        });
        let pin = to_pin.map(|m| pin_text(m, meters));
        let mut content = Self::new(style, metric, (r10, &r10_detail, gs_ok), club.as_deref(), battery, last.as_ref(), pin);
        // What is wrong with GSPro, e.g. that its Connect needs a reset.
        if r10 == R10Status::Ready && !gs_ok {
            content.message = Some((tr_detail(&gs_detail), AMBER));
        }
        content
    }

    fn new(
        style: &OverlayStyle,
        metric: bool,
        (r10, r10_detail, gs_ok): (R10Status, &str, bool),
        club: Option<&str>,
        battery: Option<u8>,
        last: Option<&state::LastShot>,
        pin: Option<String>,
    ) -> Self {
        let (color, word) = status_word(r10, gs_ok);
        let ready = r10 == R10Status::Ready && gs_ok;
        let message = match (last, ready) {
            (_, false) if r10 != R10Status::Ready => Some((tr_detail(r10_detail), AMBER)),
            (_, false) => Some((tr("GSPro not connected").to_string(), AMBER)),
            (Some(s), true) if !s.delivered => Some((tr("Last shot NOT delivered to GSPro").to_string(), AMBER)),
            _ => None,
        };
        // The last shot's values stay, also under a message; a dash where there is none yet.
        let shot = last;
        let (factor, unit) = if metric { (MPH_TO_KMH, "km/h") } else { (1.0, "mph") };
        let value = |glyph, text: Option<String>, wide: String| {
            Some(Cell::Value { glyph, text: text.unwrap_or_else(|| "–".into()), color: WHITE, wide })
        };
        let speed = |mph: f64| format!("{:.1} {unit}", mph * factor);
        let value_cell = |item: OverlayItem| -> Option<Cell> {
            let b = shot.map(|s| &s.ball);
            match item {
                OverlayItem::Status => Some(Cell::Status),
                OverlayItem::Club => Some(Cell::Value {
                    glyph: None,
                    text: club.map_or_else(|| "–".into(), club_name),
                    color: SOFT,
                    wide: club_name("PW"),
                }),
                OverlayItem::Battery => Some(Cell::Battery(battery)),
                OverlayItem::Pin => value(Some(Glyph::Pin), pin.clone().filter(|_| shot.is_some()), pin_text(888.0, true)),
                OverlayItem::ImpactSide | OverlayItem::ImpactTop => Some(Cell::Graphic(item)),
                OverlayItem::BallSpeed => value(Some(Glyph::BallSpeed), b.map(|b| speed(b.speed)), speed(288.8 / factor)),
                OverlayItem::ClubSpeed => {
                    value(Some(Glyph::ClubSpeed), shot.and_then(|s| s.club).map(|c| speed(c.speed)), speed(288.8 / factor))
                }
                OverlayItem::Smash => value(
                    Some(Glyph::Smash),
                    shot.and_then(|s| history::smash(&s.ball, s.club.as_ref())).map(|v| format!("{v:.2}")),
                    "8.88".into(),
                ),
                OverlayItem::Spin => value(Some(Glyph::Spin), b.map(|b| format!("{:.0}", b.total_spin)), "18888".into()),
                OverlayItem::Vla => value(None, b.map(|b| format!("VLA {}°", num(b.vla, 1))), "VLA -88.8°".into()),
                OverlayItem::Hla => value(None, b.map(|b| format!("HLA {}°", num(b.hla, 1))), "HLA -88.8°".into()),
                OverlayItem::SpinAxis => {
                    value(None, b.map(|b| format!("{} {}°", tr("Axis"), num(b.spin_axis, 1))), format!("{} -88.8°", tr("Axis")))
                }
                OverlayItem::Tempo => {
                    value(Some(Glyph::Tempo), shot.and_then(|s| s.tempo).map(|t| format!("{:.1} : 1", t.ratio())), "8.8 : 1".into())
                }
            }
        };
        // The value that looks misread stands out.
        let suspect = |item: OverlayItem| {
            shot.and_then(|s| s.misread).is_some_and(|m| match m {
                Misread::Spin => item == OverlayItem::Spin,
                Misread::Launch => item == OverlayItem::Vla,
                Misread::Direction => item == OverlayItem::Hla,
                Misread::SpinAxis => item == OverlayItem::SpinAxis,
                Misread::Smash => matches!(item, OverlayItem::Smash | OverlayItem::ClubSpeed),
            })
        };
        let cell = |item: OverlayItem| -> Option<Cell> {
            let mut cell = value_cell(item)?;
            if suspect(item)
                && let Cell::Value { color, .. } = &mut cell
            {
                *color = AMBER;
            }
            Some(cell)
        };
        // A graphic fills its column.
        let mut columns: Vec<[Option<Cell>; 2]> = style
            .layout
            .iter()
            .map(|[a, b]| match (a.filter(|i| i.is_graphic()), b.filter(|i| i.is_graphic())) {
                (Some(g), _) | (None, Some(g)) => [cell(g), None],
                // The same value in both places: one tile of double height.
                _ if a.is_some() && a == b && *a != Some(OverlayItem::Status) => [a.and_then(cell).map(|c| Cell::Tall(Box::new(c))), None],
                _ => [a.and_then(cell), b.and_then(cell)],
            })
            .collect();
        // The status always comes first.
        match columns.first_mut() {
            Some(first) if first[0] != Some(Cell::Status) => first[0] = Some(Cell::Status),
            None => columns.push([Some(Cell::Status), None]),
            _ => {}
        }
        let impact = last.map(|s| Impact {
            spin_axis: s.ball.spin_axis,
            aoa: s.club.map(|c| c.angle_of_attack),
            vla: s.ball.vla,
            path: s.club.map(|c| c.path),
            hla: s.ball.hla,
        });
        Self { color, word, columns, message, one_line: style.one_line, opacity: style.opacity, impact, edit: false }
    }

    /// The columns drawn with their place in the layout: all of them with a shot, else the first
    /// one and the battery. While editing (the preview), empty columns and tiles stay as places.
    fn shown_columns(&self) -> Vec<(usize, [Option<&Cell>; 2])> {
        self.columns
            .iter()
            .enumerate()
            .map(|(k, [a, b])| {
                (k, [a.as_ref(), b.as_ref()])
            })
            .filter(|(_, [a, b])| a.is_some() || b.is_some() || self.edit)
            .collect()
    }

    /// The tiles one after the other (one line): place in the layout and what it shows (`None`: an
    /// empty place while editing).
    fn shown_tiles(&self) -> Vec<(usize, usize, Option<&Cell>)> {
        let mut out = Vec::new();
        for (k, col) in self.shown_columns() {
            let whole = matches!(col[0], Some(Cell::Graphic(_) | Cell::Tall(_)));
            for (r, cell) in col.into_iter().enumerate() {
                if cell.is_some() || (self.edit && !(whole && r == 1)) {
                    out.push((k, r, cell));
                }
            }
        }
        out
    }

    /// Panel size at scale 1.
    fn size(&self, ctx: &egui::Context) -> egui::Vec2 {
        let measure = |text: &str, font: FontId| ctx.fonts_mut(|f| f.layout_no_wrap(text.to_string(), font, WHITE).size().x);
        let (tiles, height) = self.tiles(&measure);
        // A little room to spare: text laid out at another scale may come out a bit wider.
        let width = tiles.last().map_or(TILE_PAD_L, |t| t.0 + t.1) + TILE_PAD_R + 4.0;
        vec2(width.ceil(), height)
    }

    /// Where the columns go (left edge and width, from the panel's left) and the panel's height.
    fn tiles(&self, measure: &dyn Fn(&str, FontId) -> f32) -> (Vec<(f32, f32)>, f32) {
        let columns = self.shown_columns();
        let mut x = TILE_PAD_L;
        let mut out = Vec::new();
        let width = |cell: Option<&Cell>| cell.map_or(EMPTY_TILE_W, |c| cell_width(c, self.word, self.one_line, measure));
        if self.one_line {
            // One row: every tile after the other.
            for (_, _, cell) in self.shown_tiles() {
                let w = width(cell);
                out.push((x, w));
                x += w + 16.0;
            }
            return (out, 40.0);
        }
        for (_, col) in &columns {
            let whole = matches!(col[0], Some(Cell::Graphic(_) | Cell::Tall(_)));
            let w = if whole { width(col[0]) } else { col.iter().map(|c| width(*c)).fold(0.0, f32::max) };
            out.push((x, w));
            x += w + TILE_GAP;
        }
        // Taller with an impact graphic, which has its values underneath.
        let graphic = columns.iter().any(|(_, c)| matches!(c[0], Some(Cell::Graphic(_))));
        (out, if graphic { OVERLAY_IMPACT_H } else { OVERLAY_SIZE.y })
    }
}

/// Width of a tile's contents.
fn cell_width(cell: &Cell, word: &str, one_line: bool, measure: &dyn Fn(&str, FontId) -> f32) -> f32 {
    match cell {
        // As wide as the longest status word, so the overlay doesn't change with the status.
        Cell::Status => {
            let font = cond(if one_line { 20.0 } else { 24.0 });
            16.0 + STATUS_WORDS.iter().map(|w| measure(tr(w), font.clone())).fold(measure(word, font.clone()), f32::max)
        }
        Cell::Value { glyph, text, wide, .. } => {
            let font = FontId::proportional(14.0);
            (if glyph.is_some() { GLYPH_W } else { 0.0 }) + measure(text, font.clone()).max(measure(wide, font))
        }
        Cell::Battery(_) => 32.0 + measure("100 %", FontId::proportional(13.0)),
        // The panel spans the gaps on both sides.
        Cell::Graphic(_) => impact_panel_width(one_line) - if one_line { 16.0 } else { TILE_GAP },
        Cell::Tall(inner) if one_line => cell_width(inner, word, true, measure),
        Cell::Tall(inner) => match inner.as_ref() {
            Cell::Value { glyph, text, wide, .. } => {
                (if glyph.is_some() { GLYPH_W + 4.0 } else { 0.0 }) + measure(text, cond(TALL_SIZE)).max(measure(wide, cond(TALL_SIZE)))
            }
            other => cell_width(other, word, false, measure),
        },
    }
}

/// Put `item` into a tile; an impact graphic takes the whole column, and a value put where a
/// graphic was leaves the other place empty.
fn set_tile(layout: &mut [[Option<OverlayItem>; 2]], k: usize, row: usize, item: Option<OverlayItem>) {
    let Some(col) = layout.get_mut(k) else { return };
    let tall = col[0].is_some() && col[0] == col[1];
    if item.is_some_and(OverlayItem::is_graphic) || col.iter().flatten().any(|i| i.is_graphic()) {
        *col = [item, None];
    } else if tall {
        // A tile of double height keeps its height with the new value.
        *col = [item, item];
    } else {
        col[row] = item;
    }
}

/// Swap two tiles; when one of them fills its column, the two columns swap (never the first,
/// which holds the status).
fn swap_tiles(layout: &mut [[Option<OverlayItem>; 2]], (k1, r1): (usize, usize), (k2, r2): (usize, usize)) {
    if k1 >= layout.len() || k2 >= layout.len() {
        return;
    }
    // Graphics and tiles of double height fill their column: they move with it.
    let whole = |k: usize| layout[k].iter().flatten().any(|i| i.is_graphic()) || (layout[k][0].is_some() && layout[k][0] == layout[k][1]);
    if whole(k1) || whole(k2) {
        if k1 != 0 && k2 != 0 {
            layout.swap(k1, k2);
        }
        return;
    }
    let a = layout[k1][r1];
    layout[k1][r1] = layout[k2][r2];
    layout[k2][r2] = a;
}

/// Move a tile into a new column at `at` (dropped on the line between two columns). A tile
/// filling its column moves with it; a column left empty goes. Never before the status.
fn insert_tile(layout: &mut Vec<[Option<OverlayItem>; 2]>, (k, row): (usize, usize), at: usize) {
    if k == 0 && row == 0 || k >= layout.len() || at == 0 || at > layout.len() {
        return;
    }
    let col = layout[k];
    let whole = col.iter().flatten().any(|i| i.is_graphic()) || (col[0].is_some() && col[0] == col[1]);
    if whole {
        if k == 0 {
            return;
        }
        let moved = layout.remove(k);
        layout.insert(if at > k { at - 1 } else { at }, moved);
        return;
    }
    let Some(item) = col[row] else { return };
    layout[k][row] = None;
    layout.insert(at, [Some(item), None]);
    let old = if at <= k { k + 1 } else { k };
    if old != 0 && layout[old] == [None, None] {
        layout.remove(old);
    }
}

/// Name of a tile's contents on the overlay page.
fn overlay_item_name(item: OverlayItem) -> &'static str {
    match item {
        OverlayItem::Status => tr("Status"),
        OverlayItem::Club => tr("Club"),
        OverlayItem::BallSpeed => tr("Ball Speed"),
        OverlayItem::ClubSpeed => tr("Club Speed"),
        OverlayItem::Smash => tr("Smash Factor"),
        OverlayItem::Spin => tr("Total Spin"),
        OverlayItem::Vla => "VLA",
        OverlayItem::Hla => "HLA",
        OverlayItem::SpinAxis => tr("Spin Axis"),
        OverlayItem::Tempo => tr("Tempo"),
        OverlayItem::Pin => tr("Distance to the pin"),
        OverlayItem::Battery => tr("Battery"),
        OverlayItem::ImpactSide => tr("Impact from the side"),
        OverlayItem::ImpactTop => tr("Impact from above"),
    }
}

/// "139 m": the distance left to the pin, in metres or yards (shown after a flag).
fn pin_text(m: f64, meters: bool) -> String {
    if meters { format!("{m:.0} m") } else { format!("{:.0} yd", m * M_TO_YD) }
}

/// Draw the overlay into `rect`; returns where each tile of the layout was drawn (column, row,
/// rectangle), for picking what a tile shows in the preview.
fn overlay_ui(p: &egui::Painter, rect: Rect, c: &OverlayContent) -> Vec<(usize, usize, Rect)> {
    p.rect(rect, 8.0, PAGE.gamma_multiply(c.opacity), Stroke::new(1.0, LINE), StrokeKind::Inside);
    p.rect_filled(
        Rect::from_min_size(rect.min + vec2(1.0, 1.0), vec2(5.0, rect.height() - 2.0)),
        CornerRadius { nw: 7, sw: 7, ne: 0, se: 0 },
        c.color,
    );

    let measure = |text: &str, font: FontId| p.layout_no_wrap(text.to_string(), font, WHITE).size().x;
    let (tiles, _) = c.tiles(&measure);
    let draw_cell = |cell: &Cell, at: egui::Pos2| match cell {
        Cell::Status => {
            p.circle_filled(pos2(at.x + 4.0, at.y), 5.0, c.color);
            caps_text(p, pos2(at.x + 16.0, at.y), egui::Align::Min, c.word, if c.one_line { 20.0 } else { 24.0 }, c.color);
        }
        Cell::Value { glyph, text, color, .. } => {
            let mut x = at.x;
            if let Some(g) = glyph {
                draw_glyph(p, pos2(x + 8.0, at.y), *g, LABEL);
                x += GLYPH_W;
            }
            p.text(pos2(x, at.y), Align2::LEFT_CENTER, text, FontId::proportional(14.0), *color);
        }
        Cell::Battery(level) => {
            let w = cell_width(cell, c.word, c.one_line, &measure);
            match level {
                Some(level) => draw_battery(p, pos2(at.x + w, at.y), *level),
                None => {
                    p.text(pos2(at.x + w, at.y), Align2::RIGHT_CENTER, "–", FontId::proportional(14.0), MUTED);
                }
            }
        }
        Cell::Tall(inner) => match inner.as_ref() {
            // In one line just the value; two lines draw it large (see below).
            Cell::Value { glyph, text, color, .. } if !c.one_line => {
                let mut x = at.x;
                if let Some(g) = glyph {
                    draw_glyph(p, pos2(x + 8.0, at.y), *g, LABEL);
                    x += GLYPH_W + 4.0;
                }
                caps_text(p, pos2(x, at.y), egui::Align::Min, text, TALL_SIZE, *color);
            }
            Cell::Value { glyph, text, color, .. } => {
                let mut x = at.x;
                if let Some(g) = glyph {
                    draw_glyph(p, pos2(x + 8.0, at.y), *g, LABEL);
                    x += GLYPH_W;
                }
                p.text(pos2(x, at.y), Align2::LEFT_CENTER, text, FontId::proportional(14.0), *color);
            }
            Cell::Battery(Some(level)) => draw_battery(p, pos2(at.x + cell_width(inner, c.word, c.one_line, &measure), at.y), *level),
            _ => {}
        },
        Cell::Graphic(item) => {
            let Some(impact) = c.impact else { return };
            let gap = if c.one_line { 16.0 } else { TILE_GAP };
            let r = Rect::from_min_size(pos2(at.x - gap / 2.0, rect.top()), vec2(impact_panel_width(c.one_line), rect.height()));
            let draw: ImpactGraphic = if *item == OverlayItem::ImpactSide { impact_side } else { impact_top };
            draw(p, r, impact, !c.one_line);
        }
    };
    // An empty place while editing: a dashed outline.
    let placeholder = |r: Rect| {
        let r = r.shrink2(vec2(2.0, 4.0));
        let outline = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
        p.extend(egui::Shape::dashed_line(&outline, Stroke::new(1.0, BUTTON_LINE), 4.0, 3.0));
    };
    let mut hit = Vec::new();
    if c.one_line {
        let y = rect.center().y;
        let mut tile = tiles.iter();
        for (k, r, cell) in c.shown_tiles() {
            let Some(&(x, w)) = tile.next() else { break };
            let area = Rect::from_min_size(pos2(rect.left() + x, rect.top()), vec2(w, rect.height()));
            match cell {
                Some(cell) => draw_cell(cell, pos2(rect.left() + x, y)),
                None => placeholder(area),
            }
            hit.push((k, r, area));
        }
        message_band(p, rect, &tiles, c);
        return hit;
    }
    // Two rows, a thin line between the columns.
    let inset = 20.0 + ((rect.height() - OVERLAY_SIZE.y) / 2.0).max(0.0);
    let rows = [rect.top() + inset, rect.bottom() - inset];
    let half = (rows[1] - rows[0]) / 2.0;
    let mut tile = tiles.iter();
    for (n, (k, col)) in c.shown_columns().into_iter().enumerate() {
        let Some(&(x, w)) = tile.next() else { break };
        if n > 0 {
            let lx = rect.left() + x - TILE_GAP / 2.0;
            p.line_segment([pos2(lx, rect.top() + 12.0), pos2(lx, rect.bottom() - 12.0)], Stroke::new(1.0, LINE));
        }
        if let Some(cell @ (Cell::Graphic(_) | Cell::Tall(_))) = col[0] {
            draw_cell(cell, pos2(rect.left() + x, rect.center().y));
            hit.push((k, 0, Rect::from_min_size(pos2(rect.left() + x - TILE_GAP / 2.0, rect.top()), vec2(w + TILE_GAP, rect.height()))));
        } else {
            for (r, cell) in col.iter().enumerate() {
                let y = rows[r];
                let area = Rect::from_center_size(pos2(rect.left() + x + w / 2.0, y), vec2(w + TILE_GAP - 2.0, half * 2.0));
                match cell {
                    Some(cell) => draw_cell(cell, pos2(rect.left() + x, y)),
                    None if c.edit => placeholder(Rect::from_center_size(area.center(), vec2(w + 8.0, half * 2.0 - 4.0))),
                    None => {}
                }
                hit.push((k, r, area));
            }
        }
    }
    message_band(p, rect, &tiles, c);
    hit
}

/// The message on a band over every column but the status, whose last values stay dimmed
/// underneath.
fn message_band(p: &egui::Painter, rect: Rect, tiles: &[(f32, f32)], c: &OverlayContent) {
    let (Some((text, color)), Some(&(x, _))) = (&c.message, tiles.get(1)) else { return };
    let gap = if c.one_line { 16.0 } else { TILE_GAP };
    let band = Rect::from_min_max(pos2(rect.left() + x - gap / 2.0 + 1.0, rect.top() + 1.0), rect.max - vec2(1.0, 1.0));
    p.rect(band, CornerRadius { nw: 0, sw: 0, ne: 7, se: 7 }, PAGE.gamma_multiply(0.94), Stroke::NONE, StrokeKind::Inside);
    p.line_segment([band.left_top(), band.left_bottom()], Stroke::new(2.0, *color));
    let inner = band.shrink2(vec2(14.0, 6.0));
    let mut job = egui::text::LayoutJob::single_section(
        format!("▲  {text}"),
        egui::TextFormat { font_id: FontId::proportional(14.0), color: *color, ..Default::default() },
    );
    job.wrap.max_width = inner.width();
    job.wrap.max_rows = if c.one_line { 1 } else { 3 };
    let galley = p.layout_job(job);
    let at = pos2(inner.left(), inner.center().y - galley.size().y / 2.0);
    // A solid ground right under the text, so no value shows through it.
    p.rect_filled(Rect::from_min_size(at, galley.size()).expand2(vec2(8.0, 3.0)), 4.0, PAGE);
    p.galley(at, galley, *color);
}

/// Symbols in front of the overlay's values.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Glyph {
    /// A golf ball with speed lines: ball speed.
    BallSpeed,
    /// A driver's head on its shaft: club speed.
    ClubSpeed,
    /// Club face meeting the ball: smash factor.
    Smash,
    /// A golf ball with an arrow around it: spin.
    Spin,
    /// A flag: distance to the pin.
    Pin,
    /// A stopwatch: swing tempo.
    Tempo,
}

/// A golf ball: white with a few dimples.
fn golf_ball(p: &egui::Painter, c: egui::Pos2, r: f32) {
    p.circle_filled(c, r, Color32::from_rgb(0xf2, 0xf5, 0xfa));
    let dimple = Color32::from_rgb(0xa6, 0xb2, 0xc6);
    for (dx, dy) in [(-0.35, -0.3), (0.25, -0.4), (0.4, 0.15), (-0.1, 0.1), (-0.4, 0.35), (0.15, 0.5)] {
        p.circle_filled(c + vec2(dx, dy) * r, r * 0.13, dimple);
    }
}

/// A 16 px symbol centred on `c`.
fn draw_glyph(p: &egui::Painter, c: egui::Pos2, glyph: Glyph, color: Color32) {
    match glyph {
        Glyph::BallSpeed => {
            golf_ball(p, c + vec2(3.0, 0.0), 5.2);
            for (dy, len) in [(-3.2, 3.5), (0.0, 5.5), (3.2, 3.5)] {
                p.line_segment([c + vec2(-3.5 - len, dy), c + vec2(-3.5, dy)], Stroke::new(1.3, color));
            }
        }
        Glyph::ClubSpeed => {
            // A driver: shaft down to a big round head, the face on the left.
            p.line_segment([c + vec2(6.5, -8.5), c + vec2(2.0, 1.5)], Stroke::new(1.4, color));
            let head: Vec<egui::Pos2> = [
                (2.8, 1.0),
                (1.0, 0.2),
                (-2.0, 0.0),
                (-5.0, 0.8),
                (-7.2, 2.6),
                (-7.8, 5.0),
                (-6.6, 7.0),
                (-3.0, 7.6),
                (1.5, 7.2),
                (3.6, 5.6),
                (3.8, 3.2),
            ]
            .iter()
            .map(|&(x, y)| c + vec2(x, y))
            .collect();
            p.add(egui::Shape::convex_polygon(head, color, Stroke::NONE));
            // The face, a little lighter.
            p.line_segment([c + vec2(-7.0, 2.8), c + vec2(-7.3, 5.6)], Stroke::new(1.2, Color32::from_rgb(0xe6, 0xec, 0xf6)));
        }
        Glyph::Smash => {
            // The face of a club meeting the ball, with a burst between them.
            p.add(egui::Shape::convex_polygon(
                vec![c + vec2(-8.0, -6.5), c + vec2(-5.0, -6.5), c + vec2(-5.0, 6.5), c + vec2(-8.0, 6.5)],
                color,
                Stroke::NONE,
            ));
            golf_ball(p, c + vec2(3.5, 0.0), 4.6);
            for a in [-0.7_f32, 0.0, 0.7] {
                let d = vec2(a.cos(), a.sin());
                p.line_segment([c + vec2(-3.6, 0.0) + d * 0.5, c + vec2(-3.6, 0.0) + d * 2.8], Stroke::new(1.1, AMBER));
            }
        }
        Glyph::Spin => {
            golf_ball(p, c, 4.4);
            // Most of a circle around the ball, ending in an arrow head.
            let r = 7.3;
            let (from, sweep) = (-2.6_f32, 4.4_f32);
            let pts: Vec<egui::Pos2> = (0..=20)
                .map(|k| {
                    let a = from + k as f32 / 20.0 * sweep;
                    c + vec2(a.cos(), a.sin()) * r
                })
                .collect();
            let end = pts[pts.len() - 1];
            p.add(egui::Shape::line(pts, Stroke::new(1.3, color)));
            let a = from + sweep;
            let (tangent, normal) = (vec2(-a.sin(), a.cos()), vec2(a.cos(), a.sin()));
            p.add(egui::Shape::convex_polygon(
                vec![end + tangent * 3.4, end - tangent * 1.0 + normal * 2.6, end - tangent * 1.0 - normal * 2.6],
                color,
                Stroke::NONE,
            ));
        }
        Glyph::Tempo => {
            // A stopwatch: round face, crown and button on top, a hand.
            let face = c + vec2(0.0, 1.2);
            p.circle_stroke(face, 6.0, Stroke::new(1.4, color));
            p.rect_filled(Rect::from_center_size(c + vec2(0.0, -6.6), vec2(3.6, 1.8)), 0.6, color);
            p.line_segment([c + vec2(0.0, -5.7), c + vec2(0.0, -4.6)], Stroke::new(1.4, color));
            p.line_segment([c + vec2(4.6, -3.6), c + vec2(5.8, -4.8)], Stroke::new(1.4, color));
            p.line_segment([face, face + vec2(2.6, -2.8)], Stroke::new(1.4, AMBER));
            p.circle_filled(face, 1.1, color);
        }
        Glyph::Pin => {
            p.line_segment([c + vec2(-3.0, 7.5), c + vec2(-3.0, -7.5)], Stroke::new(1.4, color));
            p.add(egui::Shape::convex_polygon(vec![c + vec2(-2.5, -7.5), c + vec2(6.5, -4.6), c + vec2(-2.5, -1.7)], RED, Stroke::NONE));
            p.add(egui::Shape::ellipse_filled(c + vec2(-3.0, 7.5), vec2(4.0, 1.3), color.gamma_multiply(0.6)));
        }
    }
}

/// A shot shape's name in the language shown ("Straight" is the only one golfers translate).
/// The reading that looks misread, for the UI.
fn misread_name(m: Misread) -> &'static str {
    match m {
        Misread::Spin => tr("spin"),
        Misread::Launch => tr("launch angle"),
        Misread::Direction => tr("start direction"),
        Misread::SpinAxis => tr("spin axis"),
        Misread::Smash => tr("smash factor"),
    }
}

fn shape_name(shape: Shape) -> String {
    let name = shape.name();
    if name == "Straight" { tr("Straight").to_string() } else { name }
}

/// Draws one impact graphic into a panel (with or without its caption).
type ImpactGraphic = fn(&egui::Painter, Rect, Impact, bool);

/// Width of one impact graphic: room for its caption with two lines, small in one line.
fn impact_panel_width(one_line: bool) -> f32 {
    if one_line { 50.0 } else { 150.0 }
}

/// Room for a graphic's caption underneath (two lines only).
const IMPACT_CAPTION_H: f32 = 18.0;

/// Two values under a graphic, centred, each in its colour: "AoA 3.5 D · VLA 17.4°".
fn impact_caption(p: &egui::Painter, r: Rect, parts: &[(String, Color32)]) {
    let font = FontId::proportional(11.0);
    let sep = "  ·  ";
    let galleys: Vec<_> = parts.iter().map(|(t, c)| p.layout_no_wrap(t.clone(), font.clone(), *c)).collect();
    let sep_w = p.layout_no_wrap(sep.to_string(), font.clone(), MUTED).size().x;
    let total = galleys.iter().map(|g| g.size().x).sum::<f32>() + sep_w * (galleys.len().saturating_sub(1)) as f32;
    let y = r.bottom() - IMPACT_CAPTION_H / 2.0 - 2.0;
    let mut x = r.center().x - total / 2.0;
    for (k, g) in galleys.into_iter().enumerate() {
        if k > 0 {
            p.text(pos2(x, y), Align2::LEFT_CENTER, sep, font.clone(), MUTED);
            x += sep_w;
        }
        let w = g.size().x;
        p.galley(pos2(x, y - g.size().y / 2.0), g, WHITE);
        x += w;
    }
}

/// Map the part `view` of the mock's 118 × 94 drawing space into `r`, centred, same scale both ways.
fn impact_space(r: Rect, view: Rect) -> impl Fn(f32, f32) -> egui::Pos2 {
    let s = (r.width() / view.width()).min(r.height() / view.height());
    let origin = r.center() - view.center().to_vec2() * s;
    move |x, y| origin + vec2(x, y) * s
}

/// Filled arrow head at `tip`, pointing away from `from`.
fn arrow_head(p: &egui::Painter, from: egui::Pos2, tip: egui::Pos2, size: f32, color: Color32) {
    let d = (tip - from).normalized();
    let n = vec2(-d.y, d.x);
    let base = tip - d * size;
    p.add(egui::Shape::convex_polygon(vec![tip, base + n * size * 0.55, base - n * size * 0.55], color, Stroke::NONE));
}

/// Side view of the impact: the club head arriving at the angle of attack (blue, negative is
/// down), the ball leaving at the launch angle (white, dashed).
fn impact_side(p: &egui::Painter, r: Rect, i: Impact, labels: bool) {
    let draw = if labels { Rect::from_min_max(r.min + vec2(0.0, 2.0), r.max - vec2(0.0, IMPACT_CAPTION_H)) } else { r };
    // Club and ball stay between y 20 and 84: zoom into that band.
    let at = impact_space(draw, Rect::from_min_max(pos2(4.0, 20.0), pos2(114.0, 76.0)));
    let s = at(1.0, 0.0).x - at(0.0, 0.0).x;
    p.line_segment([at(6.0, 66.0), at(112.0, 66.0)], Stroke::new(2.0 * s, Color32::from_rgb(0x4b, 0x7a, 0x57)));
    if let Some(aoa) = i.aoa {
        let a = aoa.to_radians() as f32;
        let start = at(56.0 - 46.0 * a.cos(), 62.0 + 46.0 * a.sin());
        let end = at(56.0, 62.0);
        p.line_segment([start, end - (end - start).normalized() * 6.0 * s], Stroke::new(3.0 * s, ICON));
        arrow_head(p, start, end, 9.0 * s, ICON);
    }
    let v = i.vla.to_radians() as f32;
    let ball = at(60.0, 62.0);
    let out = at(60.0 + 50.0 * v.cos(), 62.0 - 50.0 * v.sin());
    p.extend(egui::Shape::dashed_line(&[ball, out - (out - ball).normalized() * 6.0 * s], Stroke::new(2.0 * s, WHITE), 4.0 * s, 3.0 * s));
    arrow_head(p, ball, out, 9.0 * s, WHITE);
    p.circle_filled(ball, 4.0 * s, WHITE);
    if labels {
        let mut parts: Vec<(String, Color32)> = i.aoa.map(|aoa| (format!("AoA {}", dir(aoa, "U", "D")), ICON)).into_iter().collect();
        parts.push((format!("VLA {}°", num(i.vla, 1)), WHITE));
        impact_caption(p, r, &parts);
    }
}

/// Top view of the impact over the target line (dashed): a broad arrow along the club path
/// (blue, no outline) with a narrow arrow for the start direction on top (white). The angles are
/// drawn three times larger; the numbers are the real ones.
fn impact_top(p: &egui::Painter, r: Rect, i: Impact, labels: bool) {
    let draw = if labels { Rect::from_min_max(r.min + vec2(0.0, 2.0), r.max - vec2(0.0, IMPACT_CAPTION_H)) } else { r };
    let at = impact_space(draw, Rect::from_min_max(pos2(0.0, 2.0), pos2(118.0, 92.0)));
    let s = at(1.0, 0.0).x - at(0.0, 0.0).x;
    p.extend(egui::Shape::dashed_line(&[at(59.0, 4.0), at(59.0, 90.0)], Stroke::new(1.0, Color32::from_rgb(0x4b, 0x6a, 0x8f)), 3.0 * s, 3.0 * s));
    // Outline of an arrow from the base (59, 88) at `deg`: shaft, neck, head, in drawing units.
    let arrow = |deg: f64, len: f32, shaft: f32, head_w: f32, head_l: f32| {
        let a = (deg * 3.0).to_radians() as f32;
        let (ux, uy) = (a.sin(), -a.cos());
        let (px, py) = (-uy, ux);
        let pt = |along: f32, across: f32| at(59.0 + ux * along + px * across, 88.0 + uy * along + py * across);
        let neck = len - head_l;
        let shaft_pts = vec![pt(0.0, -shaft / 2.0), pt(neck, -shaft / 2.0), pt(neck, shaft / 2.0), pt(0.0, shaft / 2.0)];
        let head_pts = vec![pt(neck, -head_w / 2.0), pt(len, 0.0), pt(neck, head_w / 2.0)];
        let outline = vec![
            pt(0.0, -shaft / 2.0),
            pt(neck, -shaft / 2.0),
            pt(neck, -head_w / 2.0),
            pt(len, 0.0),
            pt(neck, head_w / 2.0),
            pt(neck, shaft / 2.0),
            pt(0.0, shaft / 2.0),
        ];
        (shaft_pts, head_pts, outline)
    };
    if let Some(path) = i.path {
        let (shaft, head, _) = arrow(path, 78.0, 22.0, 40.0, 26.0);
        let blue = ICON.gamma_multiply(0.6);
        p.add(egui::Shape::convex_polygon(shaft, blue, Stroke::NONE));
        p.add(egui::Shape::convex_polygon(head, blue, Stroke::NONE));
    }
    // The start arrow, then the ball's flight curving by the spin axis (dashed, also exaggerated):
    // together they show the shot shape.
    let start_len = 46.0;
    let (shaft, head, outline) = arrow(i.hla, start_len, 6.0, 16.0, 14.0);
    let a = (i.hla * 3.0).to_radians() as f32;
    let heading = vec2(a.sin(), -a.cos());
    let tip = at(59.0 + heading.x * start_len, 88.0 + heading.y * start_len);
    // Curved only where the shape's name says so, like the announcement.
    let shape = Shape::of(i.hla, i.spin_axis, false);
    let bend = if shape.curve == Curve::None { 0.0 } else { (i.spin_axis as f32 * 2.2).clamp(-40.0, 40.0) };
    let control = at(59.0 + heading.x * (start_len + 22.0), 88.0 + heading.y * (start_len + 22.0));
    let end = at(59.0 + heading.x * 84.0 + bend, 88.0 + heading.y * 84.0);
    let curve: Vec<egui::Pos2> = (0..=16)
        .map(|k| {
            let t = k as f32 / 16.0;
            (tip.to_vec2() * (1.0 - t) * (1.0 - t) + control.to_vec2() * 2.0 * t * (1.0 - t) + end.to_vec2() * t * t).to_pos2()
        })
        .collect();
    p.extend(egui::Shape::dashed_line(&curve, Stroke::new(2.0 * s, WHITE), 4.0 * s, 3.0 * s));
    p.circle_filled(end, 2.5 * s, WHITE);
    p.add(egui::Shape::convex_polygon(shaft, WHITE, Stroke::NONE));
    p.add(egui::Shape::convex_polygon(head, WHITE, Stroke::NONE));
    p.add(egui::Shape::closed_line(outline, Stroke::new(1.0, PAGE)));
    if labels {
        p.text(r.left_top() + vec2(6.0, 4.0), Align2::LEFT_TOP, shape_name(shape), FontId::proportional(11.5), WHITE);
    }
    if labels {
        let mut parts: Vec<(String, Color32)> = i.path.map(|path| (format!("{} {}", tr("Path"), dir(path, "I-O", "O-I")), ICON)).into_iter().collect();
        parts.push((format!("Start {}", dir(i.hla, "R", "L")), WHITE));
        impact_caption(p, r, &parts);
    }
}

/// The overlay position as a small screen with eight spots; the middle turns the overlay off.
/// Sky and fairway behind the overlay previews, like GSPro.
fn backdrop(p: &egui::Painter, area: Rect) {
    const SKY: Color32 = Color32::from_rgb(0x1f, 0x3a, 0x5c);
    const FAIRWAY: Color32 = Color32::from_rgb(0x23, 0x50, 0x2f);
    p.rect_filled(area, 6.0, SKY);
    let ground = Rect::from_min_max(pos2(area.left(), area.top() + area.height() * 0.58), area.max);
    p.rect_filled(ground, CornerRadius { nw: 0, ne: 0, sw: 6, se: 6 }, FAIRWAY);
    p.rect_stroke(area, 6.0, Stroke::new(1.0, LINE), StrokeKind::Inside);
}

/// Which overlay the page edits: tabs in the header of the left card (`card`), the status and
/// the `count` tiles, then *+*. Returns whether *+* was clicked.
fn overlay_tabs(ui: &mut egui::Ui, card: Rect, tab: &mut Option<usize>, count: usize) -> bool {
    let mut right = card.right() - 16.0;
    let mut add = false;
    // Right to left: +, the tiles from the last, the status.
    let mut tabs: Vec<(Option<Option<usize>>, String)> = vec![(None, "+".into())];
    for k in (0..count).rev() {
        let label = if count <= 2 { tr("TILE {n}").replace("{n}", &(k + 1).to_string()) } else { (k + 1).to_string() };
        tabs.push((Some(Some(k)), label));
    }
    tabs.push((Some(None), tr("STATUS").to_string()));
    for (which, label) in tabs {
        let w = ui.painter().layout_no_wrap(label.clone(), cond(15.0), WHITE).size().x;
        let r = Rect::from_min_max(pos2(right - w, card.top() + 6.0), pos2(right, card.top() + 38.0));
        let resp = ui.interact(r.expand2(vec2(4.0, 0.0)), ui.id().with(("overlay-tab", which)), Sense::click());
        let active = which == Some(*tab);
        let color = if active { WHITE } else if resp.hovered() { SOFT } else { MUTED };
        caps_text(ui.painter(), pos2(r.left(), card.top() + 21.0), egui::Align::Min, &label, 15.0, color);
        if active {
            ui.painter().line_segment([pos2(r.left(), card.top() + 37.0), pos2(r.right(), card.top() + 37.0)], Stroke::new(2.0, ACCENT));
        }
        let tip = if which.is_none() { tr("Add a tile overlay") } else { "" };
        let resp = if tip.is_empty() { resp } else { resp.on_hover_text(tip) };
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            match which {
                Some(t) => *tab = t,
                None => add = true,
            }
        }
        right -= w + 16.0;
    }
    add
}

/// Where an overlay goes, next to its picker: the place, a hint, and *Move on screen* (or *Done*
/// while it is being moved).
fn place_text(ui: &mut egui::Ui, pos: OverlayPos, by_hand: bool, hint: &str, moving: &mut Option<Moving>, tile: Option<usize>) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 72.0), Sense::hover());
    let name = if by_hand && pos != OverlayPos::Off { tr("Placed by hand") } else { overlay_name(pos) };
    let p = ui.painter();
    p.text(r.left_center() - vec2(0.0, 18.0), Align2::LEFT_CENTER, name, FontId::proportional(14.0), WHITE);
    p.text(r.left_center() + vec2(0.0, 1.0), Align2::LEFT_CENTER, hint, FontId::proportional(12.0), MUTED);
    let here = moving.as_ref().is_some_and(|m| m.tile == tile);
    let (label, color) = if here { (tr("Done – drag it in GSPro"), ACCENT_HOVER) } else { (tr("Move on screen"), ICON) };
    let galley = p.layout_no_wrap(label.to_string(), FontId::proportional(12.5), color);
    let link = Rect::from_min_size(r.left_center() + vec2(0.0, 13.0), galley.size());
    let on = pos != OverlayPos::Off;
    let resp = ui.interact(link, ui.id().with(("move-overlay", tile)), if on { Sense::click() } else { Sense::hover() });
    let color = if !on { MUTED } else if resp.hovered() { WHITE } else { color };
    p.galley_with_override_text_color(link.min, galley, color);
    if !on {
        return;
    }
    let tip = if here { tr("Ends moving; the overlay stays where it is") } else { tr("Makes the overlay grabbable: drag it to its place in GSPro and let go") };
    if resp.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        *moving = if here { None } else { Some(Moving { tile, ..Moving::default() }) };
    }
}

/// Top left corner of an overlay of `size` (native points): placed by hand at `spot`, else at the
/// anchor, `OVERLAY_MARGIN` from the edges of the visible screen and never off its left or top.
fn overlay_origin(visible: Rect, size: egui::Vec2, (ax, ay): (f32, f32), spot: Option<[f32; 2]>) -> egui::Pos2 {
    if let Some([fx, fy]) = spot {
        let room = (visible.size() - size).max(egui::Vec2::ZERO);
        return visible.min + vec2(room.x * fx.clamp(0.0, 1.0), room.y * fy.clamp(0.0, 1.0));
    }
    let free = (visible.size() - size - vec2(2.0, 2.0) * OVERLAY_MARGIN).max(egui::Vec2::ZERO);
    visible.min + vec2(OVERLAY_MARGIN + free.x * ax, OVERLAY_MARGIN + free.y * ay)
}

/// Where an overlay dropped at `at` is kept: fractions of the room the visible screen leaves, so
/// it keeps its place on another screen size.
fn overlay_spot(visible: Rect, size: egui::Vec2, at: egui::Pos2) -> [f32; 2] {
    let room = visible.size() - size;
    let frac = |off: f32, room: f32| if room > 0.0 { (off / room).clamp(0.0, 1.0) } else { 0.0 };
    [frac(at.x - visible.left(), room.x), frac(at.y - visible.top(), room.y)]
}

/// An overlay being moved: grabbable all over, with a frame and a hint. `origin` is where it is
/// (native points), `zoom` the points of its pass in native points. Returns where it was dropped.
fn drag_overlay(ui: &egui::Ui, rect: Rect, moving: &mut Moving, origin: egui::Pos2, zoom: f32) -> Option<egui::Pos2> {
    let resp = ui.interact(rect, egui::Id::new("move-overlay"), Sense::drag());
    #[cfg(target_os = "macos")]
    let mouse = mac::mouse_location();
    #[cfg(not(target_os = "macos"))]
    let mouse: Option<egui::Pos2> = None;
    if resp.drag_started() {
        moving.grab = Some((mouse.unwrap_or(egui::Pos2::ZERO), origin));
        moving.dragged = egui::Vec2::ZERO;
    }
    if resp.dragged()
        && let Some((m0, o0)) = moving.grab
    {
        // The window moves under the mouse, so the system's mouse position is the steady one.
        moving.dragged += resp.drag_delta() * zoom;
        let by = mouse.map_or(moving.dragged, |m| m - m0);
        moving.at = Some(o0 + by);
    }
    ui.ctx().set_cursor_icon(if resp.dragged() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
    let fg = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("move-overlay")));
    fg.rect_stroke(rect, 8.0, Stroke::new(2.0, ACCENT), StrokeKind::Inside);
    let galley = fg.layout_no_wrap(tr("Drag to its place").to_string(), FontId::proportional(12.0), WHITE);
    let badge = Rect::from_center_size(pos2(rect.center().x, rect.top() + 14.0), galley.size() + vec2(14.0, 6.0));
    fg.rect_filled(badge, 5.0, ACCENT);
    fg.galley(badge.min + vec2(7.0, 3.0), galley, WHITE);
    ui.ctx().request_repaint();
    if resp.drag_stopped() { moving.at.or(Some(origin)) } else { None }
}

/// The nine places; returns whether one was clicked. Placed `by_hand`, none is marked.
fn overlay_picker(ui: &mut egui::Ui, value: &mut OverlayPos, by_hand: bool) -> bool {
    let mut clicked = false;
    let (screen, _) = ui.allocate_exact_size(vec2(128.0, 72.0), Sense::hover());
    ui.painter().rect(screen, 6.0, PAGE, Stroke::new(2.0, FIELD_LINE), StrokeKind::Inside);
    let inner = screen.shrink2(vec2(10.0, 7.0));
    let cell = vec2(inner.width() / 3.0, inner.height() / 3.0);
    for (row, cells) in OverlayPos::GRID.iter().enumerate() {
        for (col, &pos) in cells.iter().enumerate() {
            let c = inner.min + vec2((col as f32 + 0.5) * cell.x, (row as f32 + 0.5) * cell.y);
            let hit = Rect::from_center_size(c, cell);
            let resp = ui.interact(hit, ui.id().with(("overlay_pos", row, col)), Sense::click());
            let selected = *value == pos && (!by_hand || pos == OverlayPos::Off);
            let p = ui.painter();
            if pos == OverlayPos::Off {
                let color = if selected { WHITE } else if resp.hovered() { SOFT } else { MUTED };
                p.text(c, Align2::CENTER_CENTER, tr("OFF"), FontId::proportional(10.0).clone(), color);
            } else {
                let spot = Rect::from_center_size(c, vec2(16.0, 10.0));
                if selected {
                    p.rect_filled(spot.expand(2.0), 3.0, ACCENT.gamma_multiply(0.35));
                }
                let fill = if selected { ACCENT } else if resp.hovered() { BUTTON_LINE } else { FIELD_LINE };
                p.rect_filled(spot, 2.0, fill);
            }
            if resp.on_hover_text(overlay_name(pos)).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                *value = pos;
                clicked = true;
            }
        }
    }
    clicked
}

/// Battery glyph with percentage, right-aligned at `anchor` (vertical centre).
fn draw_battery(p: &egui::Painter, anchor: egui::Pos2, level: u8) {
    let color = match level {
        0..=15 => RED,
        16..=35 => AMBER,
        _ => GREEN,
    };
    let text = format!("{level} %");
    let galley = p.layout_no_wrap(text, FontId::proportional(13.0), SOFT);
    let text_w = galley.size().x;
    let body = Rect::from_min_size(pos2(anchor.x - text_w - 34.0, anchor.y - 6.0), vec2(24.0, 12.0));
    let nub = Rect::from_min_size(pos2(body.right(), anchor.y - 3.0), vec2(2.5, 6.0));
    p.rect_stroke(body, 2.0, Stroke::new(1.2, SOFT), StrokeKind::Inside);
    p.rect_filled(nub, 1.0, SOFT);
    let fill_w = (body.width() - 4.0) * f32::from(level.min(100)) / 100.0;
    p.rect_filled(
        Rect::from_min_size(body.min + vec2(2.0, 2.0), vec2(fill_w.max(1.0), body.height() - 4.0)),
        1.0,
        color,
    );
    p.galley(pos2(anchor.x - text_w, anchor.y - galley.size().y / 2.0), galley, SOFT);
}

/// GSPro-style signed value: magnitude plus direction letter(s), e.g. "4.0 D", "1.0 O-I".
fn dir(v: f64, positive: &str, negative: &str) -> String {
    let rounded = (v * 10.0).round() / 10.0;
    if rounded == 0.0 {
        "0.0".to_string()
    } else if rounded > 0.0 {
        format!("{rounded:.1} {positive}")
    } else {
        format!("{:.1} {negative}", -rounded)
    }
}

/// GSPro club code → display name.
fn club_name(code: &str) -> String {
    let num = |s: &str| s[1..].to_string();
    match code {
        "DR" => "Driver".into(),
        "PW" => "Pitching Wedge".into(),
        "GW" => "Gap Wedge".into(),
        "SW" => "Sand Wedge".into(),
        "LW" => "Lob Wedge".into(),
        "PT" => "Putter".into(),
        c if c.len() >= 2 && c.starts_with('W') && i18n::german() => format!("Holz {}", num(c)),
        c if c.len() >= 2 && c.starts_with('H') && i18n::german() => format!("Hybrid {}", num(c)),
        c if c.len() >= 2 && c.starts_with('I') && i18n::german() => format!("Eisen {}", num(c)),
        c if c.len() >= 2 && c.starts_with('W') => format!("{}-Wood", num(c)),
        c if c.len() >= 2 && c.starts_with('H') => format!("{}-Hybrid", num(c)),
        c if c.len() >= 2 && c.starts_with('I') => format!("{}-Iron", num(c)),
        c => c.to_string(),
    }
}

/// Export the shown shots as CSV; returns a status note for the UI.
fn export_csv(shots: &[ShotRecord], format: ExportFormat) -> String {
    let name = format.file_name(chrono::Local::now());
    #[cfg(target_os = "macos")]
    let target = rfd::FileDialog::new().set_title(tr("Export shots as CSV")).set_file_name(&name).save_file();
    #[cfg(not(target_os = "macos"))]
    let target = openlaunch_connect::settings::Settings::data_dir().map(|d| d.join(&name));
    let Some(path) = target else {
        return String::new();
    };
    match std::fs::write(&path, format.export(shots)) {
        Ok(()) => tr("Exported {n} shots to {path}")
            .replace("{n}", &shots.len().to_string())
            .replace("{path}", &short_path(&path)),
        Err(e) => tr("Export failed: {e}").replace("{e}", &e.to_string()),
    }
}

fn short_path(p: &std::path::Path) -> String {
    let s = p.display().to_string();
    match std::env::var("HOME") {
        Ok(home) if s.starts_with(&home) => format!("~{}", &s[home.len()..]),
        _ => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Status as during a real session, for the README screenshots.
    pub(super) fn demo_state() {
        // Screenshot tests drive the app; they must not touch the real settings file.
        openlaunch_connect::settings::SAVE.store(false, std::sync::atomic::Ordering::Relaxed);
        let (ball, club) = engine::test_shot();
        state::set_r10(R10Status::Ready, "Ready – hit away");
        state::set_gspro(true, "Connected (127.0.0.1:921)");
        state::with(|s| {
            s.r10_battery = Some(82);
            s.r10_device = Some("E4:51:2A:7C:19:0B".into());
            s.gspro_club = Some("I7".into());
            s.gspro_handed = Some("RH".into());
            s.shots_sent = 9;
            s.session = engine::demo_session();
            s.gspro_result = Some(openlaunch_connect::gspro_db::GsproResult {
                carry: 150.0,
                total: 156.0,
                side: 4.0,
                apex: 28.0,
                descent: 47.0,
                to_pin: Some(139.0),
            });
            s.last_shot = Some(state::LastShot {
                ball,
                club: Some(club),
                tempo: Some(openlaunch_connect::mapping::Tempo { backswing: 0.81, downswing: 0.27 }),
                at: Instant::now().checked_sub(Duration::from_secs(4)).unwrap_or_else(Instant::now),
                delivered: true,
                shot_number: 56_728_296,
                misread: None,
            });
            s.log.clear();
        });
        state::push_log(format!("18:09:58 OpenLaunch Connect {} started", env!("CARGO_PKG_VERSION")));
        for line in [
            "18:09:58 [R10]   Searching for R10 … (switch it on; turn off iPhone Bluetooth so it doesn't grab the R10)",
            "18:10:01 [R10]   Bluetooth connected (E4:51:2A:7C:19:0B)",
            "18:10:01 [R10]   Registered, handshake in progress …",
            "18:10:02 [R10]   Handshake complete",
            "18:10:02 [R10]   Battery: 82 %",
            "18:10:02 [GSPro] ✅ Connected to GSPro OpenConnect (127.0.0.1:921)",
            "18:10:03 [R10]   Tee distance set (2.10 m)",
            "18:10:03 [R10]   🟢 Ready – hit away",
            "18:10:41 [R10]   ⛳ Shot #1: ball 150.3 mph | VLA 11.2° | HLA 1.4° | spin 2610 rpm, axis 2.9° | club 104.1 mph, face 0.9°, path -0.6°, AoA -1.2°",
            "18:10:41 [GSPro] Sending shot to GSPro (ShotNumber 56728296) …",
            "18:10:41 [GSPro] GSPro acknowledged the shot (Ball Data received)",
            "18:10:41 [GSPro] Player: RH, club: DR",
        ] {
            state::push_log(line.into());
        }
    }

    /// `OPENLAUNCH_SCREENSHOT_DIR` overrides the target (default `docs/images`).
    pub(super) fn save(img: &image::RgbaImage, name: &str) {
        let dir = std::env::var_os("OPENLAUNCH_SCREENSHOT_DIR")
            .map_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/images"), PathBuf::from);
        let path = dir.join(name);
        img.save(&path).unwrap();
        println!("wrote {}", path.display());
    }

    /// Regenerates the README screenshots offscreen (no window, no screen recording permission):
    /// `cargo test --bin openlaunch-connect readme_screenshots -- --ignored`
    /// (`OPENLAUNCH_SCREENSHOT_LANG=de` renders the German UI, e.g. to check that texts fit).
    #[test]
    #[ignore = "writes docs/images; run on demand"]
    fn readme_screenshots() {
        use egui_kittest::Harness;

        let language = if std::env::var("OPENLAUNCH_SCREENSHOT_LANG").as_deref() == Ok("de") {
            Language::German
        } else {
            Language::English
        };
        i18n::set(language);
        demo_state();
        for (page, name) in [
            (Page::Play, "screenshot-play.png"),
            (Page::Shots, "screenshot-shots.png"),
            (Page::Settings, "screenshot-settings.png"),
            (Page::Overlay, "screenshot-overlay-settings.png"),
            (Page::Log, "screenshot-log.png"),
        ] {
            let home = std::env::var("HOME").unwrap_or_default();
            let settings = Settings {
                overlay: OverlayPos::TopRight,
                launcher_path: Some(PathBuf::from(home).join("Applications/CrossOver/GSPro/GSPro.app")),
                language,
                ..Settings::default()
            };
            let (tx, _rx) = std::sync::mpsc::channel();
            let mut harness = Harness::builder()
                .with_size(vec2(1000.0, 470.0))
                .with_pixels_per_point(2.0)
                .wgpu()
                .build_eframe(|cc| {
                    setup_style(&cc.egui_ctx);
                    App::new(settings, tx)
                });
            harness.state_mut().page = page;
            harness.state_mut().show_overlay = false; // rendered separately below
            harness.state_mut().remember_size = false;
            // Grids size their rows from the previous frame.
            for _ in 0..6 {
                harness.step();
            }
            save(&harness.render().unwrap(), name);
        }

        // The shots explorer with a range session (seven of the fourteen clubs): its default pages,
        // a page with the correlation charts and averages, and the configure mode.
        state::with(|s| s.session = engine::demo_range());
        let first = state::with(|s| s.session.iter().rev().nth(10).map(|r| (r.time.clone(), r.shot_number)));
        let hidden: std::collections::BTreeSet<Option<String>> =
            ["W5", "H3", "H4", "I6", "I8", "GW", "LW"].into_iter().map(|c| Some(c.to_string())).collect();
        let analysis = {
            use openlaunch_connect::settings::{ExplorerPage, Metric, Tile, TileKind, TileSize};
            let scatter = |x, y| Tile { kind: TileKind::Scatter, size: TileSize::W2H2, x, y, ..Tile::default() };
            ExplorerPage {
                name: "Analysis".into(),
                tiles: vec![
                    scatter(Metric::FaceToPath, Metric::SpinAxis),
                    scatter(Metric::BallSpeed, Metric::Carry),
                    Tile { kind: TileKind::Table, size: TileSize::W2H2, averages: true, ..Tile::default() },
                ],
            }
        };
        for (page, configure, name) in [
            (0, false, "screenshot-shots.png"),
            (1, false, "screenshot-ball-flight.png"),
            (2, false, "screenshot-data.png"),
            (3, false, "screenshot-analysis.png"),
            (2, true, "screenshot-configure.png"),
        ] {
            let mut settings = Settings { overlay: OverlayPos::Off, language, ..Settings::default() };
            settings.explorer.pages.push(analysis.clone());
            if configure {
                // Room on the page for the empty tile that adds tiles.
                settings.explorer.pages[2].tiles[0].size = openlaunch_connect::settings::TileSize::W3H2;
            }
            let (tx, _rx) = std::sync::mpsc::channel();
            let mut harness = Harness::builder()
                .with_size(vec2(1000.0, 470.0))
                .with_pixels_per_point(2.0)
                .wgpu()
                .build_eframe(|cc| {
                    setup_style(&cc.egui_ctx);
                    App::new(settings, tx)
                });
            let app = harness.state_mut();
            app.page = Page::Shots;
            app.show_overlay = false;
            app.remember_size = false;
            app.explorer.set_for_screenshot(page, configure, hidden.clone(), first.clone());
            for _ in 0..6 {
                harness.step();
            }
            save(&harness.render().unwrap(), name);
        }

        // Wide enough for the impact graphics; cropped to the overlay below.
        let drawn = std::rc::Rc::new(std::cell::Cell::new(Rect::ZERO));
        let size_out = drawn.clone();
        let mut harness = Harness::builder()
            .with_size(vec2(OVERLAY_SIZE.x + 2.0 * (impact_panel_width(false) + 1.0) + 120.0, OVERLAY_IMPACT_H + 20.0))
            .with_pixels_per_point(2.0)
            .wgpu()
            .build_ui(|ui| {
                // Fonts set in one frame apply from the next; draw only once they are there.
                if ui.ctx().fonts(|f| f.families().contains(&FontFamily::Name("cond".into()))) {
                    let content = OverlayContent::live(&OverlayStyle::default(), true, true);
                    let rect = Rect::from_min_size(ui.max_rect().min, content.size(ui.ctx()));
                    size_out.set(rect);
                    overlay_ui(ui.painter(), rect, &content);
                } else {
                    setup_style(ui.ctx());
                }
            });
        for _ in 0..3 {
            harness.step();
        }
        let r = drawn.get();
        let (min, size) = (r.min * 2.0, r.size() * 2.0);
        let image = image::imageops::crop_imm(&harness.render().unwrap(), min.x as u32, min.y as u32, size.x as u32, size.y as u32).to_image();
        save(&image, "screenshot-overlay.png");
    }

    #[test]
    fn direction_letters_match_gspro() {
        assert_eq!(dir(-4.0, "U", "D"), "4.0 D");
        assert_eq!(dir(-1.0, "I-O", "O-I"), "1.0 O-I");
        assert_eq!(dir(0.5, "O", "C"), "0.5 O");
        assert_eq!(dir(0.5 - -1.0, "O", "C"), "1.5 O");
        assert_eq!(dir(0.04, "O", "C"), "0.0");
    }

    /// Renders the play and shots pages in a 1.5× window (wider than 1000:470) to check the zoom:
    /// `OPENLAUNCH_SCREENSHOT_DIR=<dir> cargo test --bin openlaunch-connect zoomed_window -- --ignored`
    #[test]
    #[ignore = "writes screenshots; run on demand"]
    fn zoomed_window() {
        use egui_kittest::Harness;

        i18n::set(Language::English);
        demo_state();
        for (page, name) in [(Page::Play, "zoomed-play.png"), (Page::Shots, "zoomed-shots.png")] {
            let settings = Settings { overlay: OverlayPos::Off, language: Language::English, ..Settings::default() };
            let (tx, _rx) = std::sync::mpsc::channel();
            let mut harness = Harness::builder()
                .with_size(vec2(1700.0, 780.0))
                .with_pixels_per_point(1.0)
                .wgpu()
                .build_eframe(|cc| {
                    setup_style(&cc.egui_ctx);
                    App::new(settings, tx)
                });
            harness.state_mut().page = page;
            harness.state_mut().remember_size = false;
            for _ in 0..8 {
                harness.step();
            }
            assert!((harness.ctx.zoom_factor() - fit_zoom(vec2(1700.0, 780.0))).abs() < 0.01);
            save(&harness.render().unwrap(), name);
        }
    }

    /// Renders every page in a 1600×900 window at a fixed interface size of 100 %, where the pages
    /// get the extra room: `OPENLAUNCH_SCREENSHOT_DIR=<dir> cargo test --bin openlaunch-connect large_window -- --ignored`
    #[test]
    #[ignore = "writes screenshots; run on demand"]
    fn large_window() {
        use egui_kittest::Harness;

        i18n::set(Language::English);
        demo_state();
        for (page, name) in [
            (Page::Play, "large-play.png"),
            (Page::Shots, "large-shots.png"),
            (Page::Settings, "large-settings.png"),
            (Page::Overlay, "large-overlay.png"),
            (Page::Log, "large-log.png"),
        ] {
            let settings = Settings { overlay: OverlayPos::Off, ui_scale: Some(1.0), ..Settings::default() };
            let (tx, _rx) = std::sync::mpsc::channel();
            let mut harness = Harness::builder()
                .with_size(vec2(1600.0, 900.0))
                .with_pixels_per_point(1.0)
                .wgpu()
                .build_eframe(|cc| {
                    setup_style(&cc.egui_ctx);
                    App::new(settings, tx)
                });
            harness.state_mut().page = page;
            harness.state_mut().remember_size = false;
            for _ in 0..8 {
                harness.step();
            }
            assert_eq!(harness.ctx.zoom_factor(), 1.0);
            save(&harness.render().unwrap(), name);
        }
    }

    /// Renders the top-view impact graphic for a few shot shapes side by side:
    /// `OPENLAUNCH_SCREENSHOT_DIR=<dir> cargo test --bin openlaunch-connect impact_shapes -- --ignored`
    #[test]
    #[ignore = "writes screenshots; run on demand"]
    fn impact_shapes() {
        use egui_kittest::Harness;

        i18n::set(Language::English);
        // (HLA, spin axis, path): straight, push-draw, pull-fade, hook, slice.
        let shots = [(0.3, 1.0, 0.5), (4.0, -6.0, 5.0), (-3.5, 7.0, -4.0), (-1.0, -14.0, 3.0), (1.0, 16.0, -6.0)];
        let w = impact_panel_width(false);
        let mut harness = Harness::builder()
            .with_size(vec2(w * shots.len() as f32, OVERLAY_IMPACT_H))
            .with_pixels_per_point(2.0)
            .wgpu()
            .build_ui(move |ui| {
                if ui.ctx().fonts(|f| f.families().contains(&FontFamily::Name("cond".into()))) {
                    for (k, &(hla, spin_axis, path)) in shots.iter().enumerate() {
                        let r = Rect::from_min_size(pos2(k as f32 * w, 0.0), vec2(w, OVERLAY_IMPACT_H));
                        ui.painter().rect_filled(r, 0.0, PAGE);
                        let impact = Impact { spin_axis, aoa: None, vla: 15.0, path: Some(path), hla };
                        impact_top(ui.painter(), r, impact, true);
                    }
                } else {
                    setup_style(ui.ctx());
                }
            });
        for _ in 0..3 {
            harness.step();
        }
        save(&harness.render().unwrap(), "impact-shapes.png");
    }

    /// The Overlay page's tile tab with a few tiles (range session).
    #[test]
    #[ignore = "writes screenshots; run on demand"]
    fn tile_overlay_states() {
        use egui_kittest::Harness;
        use openlaunch_connect::settings::{Camera, Tile, TileKind};

        let language = if std::env::var("OPENLAUNCH_SCREENSHOT_LANG").as_deref() == Ok("de") { Language::German } else { Language::English };
        i18n::set(language);
        demo_state();
        state::with(|s| s.session = engine::demo_range());
        let tile = |kind| Tile { kind, ..Tile::default() };
        for (name, cfg) in [
            ("tile-overlay-dispersion.png", TileOverlay { pos: OverlayPos::TopRight, ..TileOverlay::default() }),
            (
                "tile-overlay-flight.png",
                TileOverlay { pos: OverlayPos::BottomLeft, tile: Tile { camera: Camera::Above, ..tile(TileKind::Flight) }, club_only: true, ..TileOverlay::default() },
            ),
            ("tile-overlay-moving.png", TileOverlay { pos: OverlayPos::Left, spot: Some([0.0, 0.45]), ..TileOverlay::default() }),
            ("tile-overlay-status.png", TileOverlay::default()),
            ("tile-overlay-play.png", TileOverlay::default()),
            ("tile-overlay-table.png", TileOverlay { pos: OverlayPos::Right, tile: Tile { averages: true, ..tile(TileKind::Table) }, width: 460.0, ..TileOverlay::default() }),
        ] {
            let mut settings = Settings { tile_overlays: vec![cfg, TileOverlay::default()], ..Settings::default() };
            settings.keep_arrangement();
            settings.arrangements.insert(0, Arrangement { name: "Course".into(), ..Arrangement::default() });
            settings.arrangement = 1;
            settings.arrangements[1].name = "Range".into();
            let (tx, _rx) = std::sync::mpsc::channel();
            let mut harness = Harness::builder().with_size(BASE_SIZE).with_pixels_per_point(2.0).wgpu().build_eframe(|cc| {
                setup_style(&cc.egui_ctx);
                App::new(settings, tx)
            });
            harness.state_mut().page = if name.contains("play") { Page::Play } else { Page::Overlay };
            harness.state_mut().overlay_tab = (!name.contains("status")).then_some(0);
            if name.contains("moving") {
                harness.state_mut().moving = Some(Moving { tile: Some(0), ..Moving::default() });
            }
            harness.state_mut().show_overlay = false;
            harness.state_mut().remember_size = false;
            for _ in 0..8 {
                harness.step();
            }
            save(&harness.render().unwrap(), name);
        }
    }

    #[test]
    #[ignore = "writes screenshots; run on demand"]
    fn overlay_symbols() {
        use egui_kittest::Harness;
        let glyphs = [Glyph::BallSpeed, Glyph::ClubSpeed, Glyph::Smash, Glyph::Spin, Glyph::Tempo, Glyph::Pin];
        let mut harness = Harness::builder().with_size(vec2(32.0 * glyphs.len() as f32, 32.0)).with_pixels_per_point(6.0).wgpu().build_ui(move |ui| {
            ui.painter().rect_filled(ui.max_rect(), 0.0, PAGE);
            for (k, g) in glyphs.iter().enumerate() {
                draw_glyph(ui.painter(), pos2(16.0 + 32.0 * k as f32, 16.0), *g, LABEL);
            }
        });
        harness.step();
        save(&harness.render().unwrap(), "overlay-symbols.png");
    }

    /// The overlay in each state: ready, R10 away, GSPro away (with the last shot dimmed under
    /// the message), no shot yet, and in one line – all of the same size.
    #[test]
    #[ignore = "writes screenshots; run on demand"]
    fn overlay_states() {
        use egui_kittest::Harness;
        demo_state();
        let (ball, club) = engine::test_shot();
        let shot = state::LastShot { ball, club: Some(club), tempo: None, at: Instant::now(), delivered: true, shot_number: 1, misread: None };
        let one_line = OverlayStyle { one_line: true, ..OverlayStyle::default() };
        let states = [
            ("ready", OverlayStyle::default(), (R10Status::Ready, "Ready", true), Some(&shot)),
            ("no-r10", OverlayStyle::default(), (R10Status::Searching, "Searching for R10 …", true), Some(&shot)),
            ("no-gspro", OverlayStyle::default(), (R10Status::Ready, "Ready", false), Some(&shot)),
            ("no-shot", OverlayStyle::default(), (R10Status::Ready, "Ready", true), None),
            ("one-line", one_line, (R10Status::Searching, "Searching for R10 …", true), Some(&shot)),
        ];
        for (name, style, status, last) in states {
            let shot = last.cloned();
            let drawn = std::rc::Rc::new(std::cell::Cell::new(Rect::ZERO));
            let size_out = drawn.clone();
            let mut harness = Harness::builder()
                .with_size(vec2(1100.0, OVERLAY_IMPACT_H + 20.0))
                .with_pixels_per_point(2.0)
                .wgpu()
                .build_ui(move |ui| {
                    if ui.ctx().fonts(|f| f.families().contains(&FontFamily::Name("cond".into()))) {
                        let battery = (status.0 != R10Status::Searching).then_some(82);
                        let content = OverlayContent::new(&style, true, status, Some("I7"), battery, shot.as_ref(), None);
                        let rect = Rect::from_min_size(ui.max_rect().min, content.size(ui.ctx()));
                        size_out.set(rect);
                        overlay_ui(ui.painter(), rect, &content);
                    } else {
                        setup_style(ui.ctx());
                    }
                });
            for _ in 0..3 {
                harness.step();
            }
            let r = drawn.get();
            println!("{name}: {:.0}×{:.0}", r.width(), r.height());
            let (min, size) = (r.min * 2.0, r.size() * 2.0);
            let image = image::imageops::crop_imm(&harness.render().unwrap(), min.x as u32, min.y as u32, size.x as u32, size.y as u32).to_image();
            save(&image, &format!("overlay-{name}.png"));
        }
    }

    #[test]
    fn overlays_placed_by_hand_keep_their_place() {
        let visible = Rect::from_min_size(pos2(0.0, 25.0), vec2(1512.0, 900.0));
        let size = vec2(364.0, 274.0);
        // Dropped somewhere: kept as fractions, back at the same spot.
        let at = pos2(40.0, 330.0);
        let spot = overlay_spot(visible, size, at);
        assert!((overlay_origin(visible, size, (0.0, 0.0), Some(spot)) - at).length() < 0.01);
        // Dropped half off the screen: kept on it.
        let spot = overlay_spot(visible, size, pos2(-50.0, 2000.0));
        assert_eq!(spot, [0.0, 1.0]);
        assert_eq!(overlay_origin(visible, size, (0.5, 0.5), Some(spot)), pos2(0.0, 25.0 + 900.0 - 274.0));
        // Without a spot, the anchor with the margin.
        assert_eq!(overlay_origin(visible, size, (0.0, 0.0), None), pos2(OVERLAY_MARGIN, 25.0 + OVERLAY_MARGIN));
    }

    #[test]
    fn overlay_graphics_move_as_columns() {
        use OverlayItem::*;
        let mut layout = OverlayItem::default_layout();
        // Ball speed onto the side graphic: the two columns swap.
        swap_tiles(&mut layout, (1, 0), (5, 0));
        assert_eq!(layout[1], [Some(ImpactSide), None]);
        assert_eq!(layout[5], [Some(BallSpeed), Some(ClubSpeed)]);
        // Never with the status column.
        swap_tiles(&mut layout, (0, 1), (1, 0));
        assert_eq!(layout[0], [Some(Status), Some(Club)]);
        // A graphic put into a tile takes the column; a value put there leaves the rest empty.
        set_tile(&mut layout, 3, 1, Some(ImpactTop));
        assert_eq!(layout[3], [Some(ImpactTop), None]);
        set_tile(&mut layout, 3, 0, Some(Tempo));
        assert_eq!(layout[3], [Some(Tempo), None]);
        set_tile(&mut layout, 3, 1, Some(Pin));
        assert_eq!(layout[3], [Some(Tempo), Some(Pin)]);
        // A tile of double height keeps its height with another value and moves as a column.
        layout[2] = [Some(BallSpeed), Some(BallSpeed)];
        set_tile(&mut layout, 2, 0, Some(Spin));
        assert_eq!(layout[2], [Some(Spin), Some(Spin)]);
        swap_tiles(&mut layout, (2, 0), (3, 1));
        assert_eq!((layout[2], layout[3]), ([Some(Tempo), Some(Pin)], [Some(Spin), Some(Spin)]));
    }

    #[test]
    fn overlay_tiles_go_between_columns() {
        use OverlayItem::*;
        let mut layout = OverlayItem::default_layout();
        let n = layout.len();
        // Spin onto the line between status and speeds: a column of its own there.
        insert_tile(&mut layout, (2, 1), 1);
        assert_eq!(layout.len(), n + 1);
        assert_eq!(layout[1], [Some(Spin), None]);
        assert_eq!(layout[3], [Some(Smash), None]);
        // Its old column emptied: gone. Smash after the last column.
        let end = layout.len();
        insert_tile(&mut layout, (3, 0), end);
        assert_eq!(layout.len(), n + 1);
        assert_eq!(layout.last(), Some(&[Some(Smash), None]));
        assert!(!layout.contains(&[None, None]));
        // Never before the status; the club below the status may leave.
        insert_tile(&mut layout, (1, 0), 0);
        assert_eq!(layout[1], [Some(Spin), None]);
        insert_tile(&mut layout, (0, 1), 2);
        assert_eq!(layout[0], [Some(Status), None]);
        assert_eq!(layout[2], [Some(Club), None]);
        // A graphic moves with its column.
        let side = layout.iter().position(|c| c[0] == Some(ImpactSide)).unwrap();
        insert_tile(&mut layout, (side, 0), 1);
        assert_eq!(layout[1], [Some(ImpactSide), None]);
    }

    /// Dragging a tile in the overlay preview onto another swaps the two.
    #[test]
    fn overlay_tiles_are_dragged_into_place() {
        use egui_kittest::Harness;

        demo_state();
        let settings = Settings { overlay: OverlayPos::TopRight, ..Settings::default() };
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut harness = Harness::builder().with_size(BASE_SIZE).build_eframe(|cc| {
            setup_style(&cc.egui_ctx);
            App::new(settings, tx)
        });
        harness.state_mut().page = Page::Overlay;
        harness.state_mut().show_overlay = false;
        harness.state_mut().remember_size = false;
        for _ in 0..3 {
            harness.step();
        }
        // Find two tiles by hovering over the preview: ball speed (column 1, top) and VLA (column 3, top).
        let ids: Vec<(usize, usize, Rect)> = harness.state().overlay_tile_rects.borrow().clone();
        let find = |k: usize, r: usize| ids.iter().find(|t| t.0 == k && t.1 == r).map(|t| t.2.center()).expect("tile");
        let (from, to) = (find(1, 0), find(3, 0));
        let press = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        for event in [egui::Event::PointerMoved(from), press(from, true), egui::Event::PointerMoved(from + vec2(15.0, 0.0)), egui::Event::PointerMoved(to), press(to, false)] {
            harness.input_mut().events.push(event);
            harness.step();
        }
        harness.step();
        let layout = &harness.state().saved.overlay_style.layout;
        assert_eq!((layout[1][0], layout[3][0]), (Some(OverlayItem::Vla), Some(OverlayItem::BallSpeed)));

        // Spin dropped on the line before the speeds' column: a new column there.
        let rects = harness.state().overlay_tile_rects.borrow().clone();
        let spin = rects.iter().find(|t| t.0 == 2 && t.1 == 1).unwrap().2.center();
        let line = rects.iter().filter(|t| t.0 == 1).map(|t| t.2.left()).fold(f32::MAX, f32::min);
        let to = pos2(line, spin.y);
        for event in [egui::Event::PointerMoved(spin), press(spin, true), egui::Event::PointerMoved(spin - vec2(15.0, 0.0)), egui::Event::PointerMoved(to), press(to, false)] {
            harness.input_mut().events.push(event);
            harness.step();
        }
        harness.step();
        assert_eq!(harness.state().saved.overlay_style.layout[1], [Some(OverlayItem::Spin), None]);

        // + after the last column adds an empty one, which can be clicked in the preview.
        let rects = harness.state().overlay_tile_rects.borrow().clone();
        let right = rects.iter().map(|t| t.2.right()).fold(f32::MIN, f32::max);
        let (top, bottom) = rects.iter().fold((f32::MAX, f32::MIN), |(a, b), t| (a.min(t.2.top()), b.max(t.2.bottom())));
        let plus = pos2(right, (top + bottom) / 2.0);
        for event in [egui::Event::PointerMoved(plus), press(plus, true), press(plus, false)] {
            harness.input_mut().events.push(event);
            harness.step();
        }
        harness.step();
        let n = harness.state().saved.overlay_style.layout.len();
        assert_eq!(n, OverlayItem::default_layout().len() + 2);
        assert!(harness.state().overlay_tile_rects.borrow().iter().any(|t| t.0 == n - 1), "the empty column has places");
    }

    /// Scaling the overlay must not shrink the main window.
    #[test]
    fn overlay_scaling_leaves_the_main_window_alone() {
        use egui_kittest::Harness;

        let settings = Settings { overlay: OverlayPos::TopRight, ..Settings::default() };
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut harness = Harness::builder()
            .with_size(vec2(1700.0, 780.0))
            .build_eframe(|cc| {
                setup_style(&cc.egui_ctx);
                App::new(settings, tx)
            });
        harness.state_mut().remember_size = false;
        for _ in 0..4 {
            // The overlay is placed on the monitor; kittest has none by default.
            harness.input_mut().viewports.entry(egui::ViewportId::ROOT).or_default().monitor_size =
                Some(vec2(2560.0, 1440.0));
            harness.step();
        }
        let ctx = &harness.ctx;
        assert!(ctx.zoom_factor() > 1.0);
        // The overlay is sized by the zoom of its own pass; the main window keeps its zoom.
        assert_eq!(ctx.layer_transform_to_global(egui::LayerId::background()), None);
        assert_eq!(ctx.layer_transform_to_global(overlay_layer()), None);
    }

    #[test]
    fn window_keeps_the_aspect_ratio() {
        assert_eq!(window_height(BASE_SIZE.x), BASE_SIZE.y);
        assert_eq!(window_height(1500.0), 705.0);
        // The zoom uses the whole height and width.
        assert_eq!(fit_zoom(vec2(1500.0, window_height(1500.0))), 1.5);
    }

    #[test]
    fn window_size_follows_the_interface_size() {
        assert_eq!(min_size(None), BASE_SIZE);
        assert_eq!(min_size(Some(1.5)), vec2(1500.0, 705.0));
        assert_eq!(start_size(None, None), BASE_SIZE);
        assert_eq!(start_size(None, Some(1.25)), vec2(1250.0, 587.5));
        // Fitting keeps the base shape; a fixed size keeps the saved shape, at least the minimum.
        assert_eq!(start_size(Some([1500.0, 900.0]), None), vec2(1500.0, 705.0));
        assert_eq!(start_size(Some([1600.0, 900.0]), Some(1.0)), vec2(1600.0, 900.0));
        assert_eq!(start_size(Some([1200.0, 900.0]), Some(1.5)), vec2(1500.0, 900.0));
    }

    #[test]
    fn window_scales_with_the_interface_size() {
        let room = Some(vec2(1470.0, 890.0));
        // 100 % → 125 %: grows by the same factor, back down shrinks again.
        assert_eq!(resized_window(vec2(1200.0, 600.0), 1.0, Some(1.25), room), vec2(1470.0, 750.0));
        assert_eq!(resized_window(vec2(1250.0, 700.0), 1.25, Some(1.0), room), vec2(1000.0, 560.0));
        // Never below the minimum of the new size.
        assert_eq!(resized_window(vec2(1000.0, 470.0), 1.25, Some(1.0), room), BASE_SIZE);
        // Auto: the base shape at the current zoom, within the screen.
        assert_eq!(resized_window(vec2(1300.0, 800.0), 1.25, None, room), vec2(1250.0, 588.0));
        assert_eq!(resized_window(vec2(1300.0, 800.0), 1.5, None, room), vec2(1470.0, 691.0));
    }

    #[test]
    fn zoom_fits_the_base_layout() {
        assert_eq!(fit_zoom(BASE_SIZE), 1.0);
        assert_eq!(fit_zoom(vec2(800.0, 400.0)), 1.0, "never below 1");
        assert_eq!(fit_zoom(vec2(1500.0, 705.0)), 1.5);
        // Limited by the smaller ratio: wide windows by the height, tall ones by the width.
        assert_eq!(fit_zoom(vec2(3000.0, 705.0)), 1.5);
        assert_eq!(fit_zoom(vec2(1500.0, 2000.0)), 1.5);
        assert_eq!(fit_zoom(vec2(1234.0, 999.0)), 1.23);
    }

    #[test]
    fn club_names() {
        assert_eq!(club_name("DR"), "Driver");
        assert_eq!(club_name("I7"), "7-Iron");
        assert_eq!(club_name("W3"), "3-Wood");
    }
}
