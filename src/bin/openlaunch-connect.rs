//! OpenLaunch Connect — desktop app around the R10 → GSPro bridge,
//! styled after GSPro's in-game interface.

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
use openlaunch_connect::settings::{OverlayItem, OverlayPos, OverlayStyle, Settings};
use openlaunch_connect::shape::Shape;
use openlaunch_connect::state::{self, R10Status};
use openlaunch_connect::update;

// Next to this file (the crate root of this binary is src/bin/).
#[path = "openlaunch-connect/explorer.rs"]
mod explorer;

const APP_NAME: &str = "OpenLaunch Connect";
const MPH_TO_KMH: f64 = 1.609_344;
const M_TO_YD: f64 = 1.093_613;
const OVERLAY_TITLE: &str = "OpenLaunch Overlay";
/// Size the layout is designed for. Larger windows zoom the whole UI uniformly, or, with a fixed
/// interface size, give the pages more room.
const BASE_SIZE: egui::Vec2 = vec2(1000.0, 470.0);
const OVERLAY_SIZE: egui::Vec2 = vec2(400.0, 72.0);
/// Distance of the overlay from the screen edges.
const OVERLAY_MARGIN: f32 = 14.0;

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
    /// Where the overlay preview's tiles were drawn (last frame).
    overlay_tile_rects: std::cell::RefCell<Vec<(usize, usize, Rect)>>,
    /// Settings page: asking before the shots view is reset.
    confirm_reset: bool,
    /// Shots page: page shown, filter and selection.
    explorer: explorer::ExplorerState,
    /// Sample shot shown in the overlay preview.
    preview_shot: state::LastShot,
}

impl App {
    fn new(settings: Settings, tx: Sender<Cmd>) -> Self {
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
                }
            },
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

    fn play(&self, ui: &mut egui::Ui) {
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

        section(ui, tr("BALL"));
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
                let (dot, what) = if s.delivered {
                    (GREEN, tr("delivered to GSPro"))
                } else {
                    (AMBER, tr("NOT delivered to GSPro"))
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
        let Some((ax, ay)) = self.saved.overlay.anchor().filter(|_| self.show_overlay) else {
            return;
        };
        let Some(screen) = ctx.input(|i| i.viewport().monitor_size) else {
            return;
        };
        let style = &self.saved.overlay_style;
        let content = OverlayContent::live(style, self.saved.metric, self.saved.meters);
        let base = content.size(ctx);
        // Sizes are in points, which grow with the main window's zoom; divide it out so the
        // overlay keeps its size on screen however large the main window is.
        let zoom = ctx.zoom_factor();
        let scale = style.scale / zoom;
        let (size, margin) = (base * scale, OVERLAY_MARGIN / zoom);
        // Never off the left or top edge, even when wider than the screen.
        let free = (screen - size - vec2(2.0, 2.0) * margin).max(egui::Vec2::ZERO);
        let pos = pos2(margin + free.x * ax, margin + free.y * ay);
        let builder = egui::ViewportBuilder::default()
            .with_title(OVERLAY_TITLE)
            .with_position(pos)
            .with_inner_size(size)
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_maximize_button(false)
            .with_window_level(egui::WindowLevel::AlwaysOnTop)
            .with_mouse_passthrough(true)
            .with_active(false)
            .with_taskbar(false);
        let capture = &mut self.capture;
        ctx.show_viewport_immediate(overlay_id(), builder, |ui, _class| {
            #[cfg(target_os = "macos")]
            mac::float_over_fullscreen(OVERLAY_TITLE);
            // Draw at the base size and scale the layer by size ÷ zoom (no input, so this is safe).
            // Own layer: layer transforms apply to every viewport, and the main window draws on
            // `ui.layer_id()` (the background layer) too.
            let layer = overlay_layer();
            ui.ctx().set_transform_layer(layer, egui::emath::TSTransform::from_scaling(scale));
            // The clip rect is scaled with the layer, so it has to be the unscaled size.
            let rect = Rect::from_min_size(egui::Pos2::ZERO, base);
            overlay_ui(&ui.ctx().layer_painter(layer).with_clip_rect(rect), rect, &content);
            capture.save_from(ui.ctx());
        });
    }

    /// Overlay position, size and contents with a preview; changes apply and are saved at once.
    fn overlay_page(&mut self, ui: &mut egui::Ui) {
        let mut pos = self.saved.overlay;
        let mut style = self.saved.overlay_style.clone();
        let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), 254.0), Sense::hover());
        let col_w = (area.width() - 10.0) / 2.0;
        let left = Rect::from_min_size(area.min, vec2(col_w, area.height()));
        let right = Rect::from_min_size(pos2(area.left() + col_w + 10.0, area.top()), vec2(col_w, area.height()));

        card(ui, left, tr("LAYOUT"), Icon::Layout, |ui| {
            ui.spacing_mut().item_spacing.y = 7.0;
            ui.horizontal(|ui| {
                setting_label(ui, tr("Position"), 72.0);
                overlay_picker(ui, &mut pos);
                ui.add_space(12.0);
                // Text block centred on the picker.
                let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 72.0), Sense::hover());
                let p = ui.painter();
                p.text(r.left_center() - vec2(0.0, 12.0), Align2::LEFT_CENTER, overlay_name(pos), FontId::proportional(14.0), WHITE);
                p.text(r.left_center() + vec2(0.0, 7.0), Align2::LEFT_CENTER, tr("On top of GSPro, also in full screen."), FontId::proportional(12.0), MUTED);
                p.text(r.left_center() + vec2(0.0, 23.0), Align2::LEFT_CENTER, tr("Choose the middle to turn it off."), FontId::proportional(12.0), MUTED);
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
            ui.add_space(2.0);
            toggle(ui, &mut style.impact_side, tr("Impact from the side"));
            toggle(ui, &mut style.impact_top, tr("Impact from above"));
        });

        // Below the cards, down to the page's bottom margin (the cards move the layout cursor).
        let preview = Rect::from_min_max(pos2(area.left(), area.bottom() + 10.0), pos2(area.right(), ui.max_rect().bottom()));
        self.overlay_preview(ui, preview, pos, &mut style);

        if pos != self.saved.overlay || style != self.saved.overlay_style {
            self.saved.overlay = pos;
            self.edit.overlay = pos;
            self.saved.overlay_style = style.clone();
            self.edit.overlay_style = style;
            if let Err(e) = self.saved.save() {
                log::info(&format!("Saving the overlay settings failed: {e}"));
            }
        }
    }

    /// The overlay with a sample shot on a GSPro-like backdrop, at its size and position.
    fn overlay_preview(&self, ui: &egui::Ui, area: Rect, pos: OverlayPos, style: &mut OverlayStyle) {
        const SKY: Color32 = Color32::from_rgb(0x1f, 0x3a, 0x5c);
        const FAIRWAY: Color32 = Color32::from_rgb(0x23, 0x50, 0x2f);
        let p = ui.painter();
        p.rect_filled(area, 6.0, SKY);
        let ground = Rect::from_min_max(pos2(area.left(), area.top() + area.height() * 0.58), area.max);
        p.rect_filled(ground, CornerRadius { nw: 0, ne: 0, sw: 6, se: 6 }, FAIRWAY);
        p.rect_stroke(area, 6.0, Stroke::new(1.0, LINE), StrokeKind::Inside);
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
        let ready = (R10Status::Ready, "", true);
        let pin = Some(pin_text(139.0, self.saved.meters));
        let content = OverlayContent::new(style, self.saved.metric, ready, Some("I7"), Some(82), Some(&self.preview_shot), pin);
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
        for &(k, row, r) in &tiles {
            let resp = ui.interact(r, ui.id().with(("overlay-tile", k, row)), Sense::click_and_drag());
            let target = pointer.and_then(|at| tiles.iter().find(|t| t.2.contains(at))).filter(|t| (t.0, t.1) != (k, row));
            if resp.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                let fg = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, ui.id().with("overlay-drag")));
                fg.rect_filled(r, 4.0, Color32::from_black_alpha(90));
                if let Some(t) = target {
                    fg.rect_stroke(t.2.shrink(1.0), 4.0, Stroke::new(2.0, ACCENT), StrokeKind::Inside);
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
            if resp.drag_stopped()
                && let Some(t) = target
            {
                swap = Some(((k, row), (t.0, t.1)));
            }
            let resp = resp.on_hover_cursor(egui::CursorIcon::Grab).on_hover_text(tr("Click to choose what this tile shows, drag to move it"));
            egui::Popup::menu(&resp).show(|ui| {
                let current = style.layout.get(k).and_then(|c| c[row]);
                for item in OverlayItem::CHOICES {
                    if ui.selectable_label(current == Some(item), overlay_item_name(item)).clicked() {
                        style.layout[k][row] = Some(item);
                    }
                }
                ui.separator();
                if ui.selectable_label(current.is_none(), tr("Empty")).clicked() {
                    style.layout[k][row] = None;
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
        if let Some(((k1, r1), (k2, r2))) = swap
            && k1 < style.layout.len()
            && k2 < style.layout.len()
        {
            let a = style.layout[k1][r1];
            style.layout[k1][r1] = style.layout[k2][r2];
            style.layout[k2][r2] = a;
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
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
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
        self.overlay(ui.ctx());
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
    use objc2_app_kit::{NSApplication, NSScreenSaverWindowLevel, NSWindowCollectionBehavior};
    use std::ptr::NonNull;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};

    use block2::RcBlock;
    use objc2_app_kit::{NSWindow, NSWindowDidEndLiveResizeNotification, NSWindowWillStartLiveResizeNotification};
    use objc2_foundation::{NSNotification, NSNotificationCenter, NSSize, NSString};

    /// Aspect ratio the main window keeps while resizing (`None`: none, e.g. in full screen).
    static ASPECT: Mutex<Option<(f64, f64)>> = Mutex::new(None);

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
    /// separate Space of a full-screen app, and above GSPro's own full-screen window.
    pub fn float_over_fullscreen(title: &str) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let title = NSString::from_str(title);
        let behavior = NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle;
        for window in NSApplication::sharedApplication(mtm).windows() {
            if !window.title().isEqualToString(&title) {
                continue;
            }
            if window.collectionBehavior() != behavior {
                window.setCollectionBehavior(behavior);
            }
            if window.level() != NSScreenSaverWindowLevel {
                window.setLevel(NSScreenSaverWindowLevel);
            }
        }
    }
}

// ── widgets ──

fn section(ui: &mut egui::Ui, title: &str) {
    ui.label(RichText::new(title).font(cond(15.0)).color(MUTED).extra_letter_spacing(1.5));
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
    ui.horizontal(|ui| {
        ui.set_min_height(34.0);
        setting_label(ui, label, 34.0);
        add(ui);
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
        (_, false) => (AMBER, tr("WAITING FOR GSPRO")),
        (R10Status::Connected, true) => (AMBER, tr("R10 NOT READY")),
    }
}

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

/// What a tile shows.
#[derive(Clone, PartialEq, Debug)]
enum Cell {
    /// Dot and status word in the status colour.
    Status,
    /// A value, with its symbol in front.
    Value { glyph: Option<Glyph>, text: String, color: Color32 },
    /// Battery level of the R10.
    Battery(u8),
}

/// What the overlay shows: the tiles (columns of two), or the status and what's wrong.
struct OverlayContent {
    color: Color32,
    word: &'static str,
    /// The tiles in columns (top, bottom); the first one holds the status.
    columns: Vec<[Option<Cell>; 2]>,
    /// Not ready, no shot yet or not delivered: shown next to the first column instead of the
    /// others (the battery stays).
    message: Option<(String, Color32)>,
    one_line: bool,
    opacity: f32,
    /// Angles for the impact graphics (when on and a shot is shown).
    impact: Option<Impact>,
}

/// What the impact graphics show, in degrees: angle of attack and club path need club data.
#[derive(Clone, Copy)]
struct Impact {
    side: bool,
    top: bool,
    spin_axis: f64,
    aoa: Option<f64>,
    vla: f64,
    path: Option<f64>,
    hla: f64,
}

impl OverlayContent {
    /// From the shared status.
    fn live(style: &OverlayStyle, metric: bool, meters: bool) -> Self {
        let (r10, r10_detail, gs_ok, club, battery, last, to_pin) = state::with(|s| {
            let to_pin = s.gspro_result.and_then(|r| r.to_pin);
            (s.r10, s.r10_detail.clone(), s.gspro_connected, s.gspro_club.clone(), s.r10_battery, s.last_shot.clone(), to_pin)
        });
        let pin = to_pin.map(|m| pin_text(m, meters));
        Self::new(style, metric, (r10, &r10_detail, gs_ok), club.as_deref(), battery, last.as_ref(), pin)
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
            (Some(_), true) => None,
            (None, true) => Some((tr("Hit away").to_string(), SOFT)),
        };
        let shot = last.filter(|_| message.is_none());
        let (factor, unit) = if metric { (MPH_TO_KMH, "km/h") } else { (1.0, "mph") };
        let value = |glyph, text: String| Some(Cell::Value { glyph, text, color: WHITE });
        let cell = |item: OverlayItem| -> Option<Cell> {
            match item {
                OverlayItem::Status => Some(Cell::Status),
                OverlayItem::Club => club.map(|c| Cell::Value { glyph: None, text: club_name(c), color: SOFT }),
                OverlayItem::Battery => battery.map(Cell::Battery),
                OverlayItem::Pin => pin.clone().filter(|_| shot.is_some()).and_then(|p| value(Some(Glyph::Pin), p)),
                _ => {
                    let s = shot?;
                    let b = &s.ball;
                    match item {
                        OverlayItem::BallSpeed => value(Some(Glyph::BallSpeed), format!("{:.1} {unit}", b.speed * factor)),
                        OverlayItem::ClubSpeed => value(Some(Glyph::ClubSpeed), format!("{:.1} {unit}", s.club?.speed * factor)),
                        OverlayItem::Smash => value(Some(Glyph::Smash), format!("{:.2}", history::smash(b, s.club.as_ref())?)),
                        OverlayItem::Spin => value(Some(Glyph::Spin), format!("{:.0}", b.total_spin)),
                        OverlayItem::Vla => value(None, format!("VLA {}°", num(b.vla, 1))),
                        OverlayItem::Hla => value(None, format!("HLA {}°", num(b.hla, 1))),
                        OverlayItem::SpinAxis => value(None, format!("{} {}°", tr("Axis"), num(b.spin_axis, 1))),
                        OverlayItem::Tempo => value(None, format!("{} {:.1}:1", tr("Tempo"), s.tempo?.ratio())),
                        _ => None,
                    }
                }
            }
        };
        let mut columns: Vec<[Option<Cell>; 2]> =
            style.layout.iter().map(|[a, b]| [a.and_then(cell), b.and_then(cell)]).collect();
        // The status always comes first.
        match columns.first_mut() {
            Some(first) if first[0] != Some(Cell::Status) => first[0] = Some(Cell::Status),
            None => columns.push([Some(Cell::Status), None]),
            _ => {}
        }
        let impact = last.filter(|s| (style.impact_side || style.impact_top) && ready && s.delivered).map(|s| Impact {
            side: style.impact_side,
            top: style.impact_top,
            spin_axis: s.ball.spin_axis,
            aoa: s.club.map(|c| c.angle_of_attack),
            vla: s.ball.vla,
            path: s.club.map(|c| c.path),
            hla: s.ball.hla,
        });
        Self { color, word, columns, message, one_line: style.one_line, opacity: style.opacity, impact }
    }

    /// The columns drawn: all of them with a shot; else the first one and the battery.
    fn shown_columns(&self) -> Vec<[Option<&Cell>; 2]> {
        self.columns
            .iter()
            .enumerate()
            .map(|(k, [a, b])| {
                let shows = |c: &Cell| self.message.is_none() || k == 0 || matches!(c, Cell::Battery(_));
                [a.as_ref().filter(|c| shows(c)), b.as_ref().filter(|c| shows(c))]
            })
            .filter(|[a, b]| a.is_some() || b.is_some())
            .collect()
    }

    /// Panel size at scale 1.
    fn size(&self, ctx: &egui::Context) -> egui::Vec2 {
        let measure = |text: &str, font: FontId| ctx.fonts_mut(|f| f.layout_no_wrap(text.to_string(), font, WHITE).size().x);
        let (tiles, height) = self.tiles(&measure);
        let width = tiles.last().map_or(TILE_PAD_L, |t| t.0 + t.1) + TILE_PAD_R;
        vec2(width.ceil(), height) + vec2(self.graphics_width(), 0.0)
    }

    /// Where the columns go (left edge and width, from the panel's left) and the panel's height.
    /// The message, if any, comes after the first column as one more "column".
    fn tiles(&self, measure: &dyn Fn(&str, FontId) -> f32) -> (Vec<(f32, f32)>, f32) {
        let columns = self.shown_columns();
        let mut x = TILE_PAD_L;
        let mut out = Vec::new();
        if self.one_line {
            // One row: every tile after the other.
            for cell in columns.iter().flatten().flatten() {
                let w = cell_width(cell, self.word, true, measure);
                out.push((x, w));
                x += w + 16.0;
            }
            if let Some((text, _)) = &self.message {
                out.insert(1.min(out.len()), (0.0, measure(text, FontId::proportional(14.0))));
                // Re-lay out in order.
                let mut x = TILE_PAD_L;
                for t in &mut out {
                    t.0 = x;
                    x += t.1 + 16.0;
                }
            }
            return (out, 40.0);
        }
        for (k, col) in columns.iter().enumerate() {
            let w = col.iter().flatten().map(|c| cell_width(c, self.word, false, measure)).fold(0.0, f32::max);
            out.push((x, w));
            x += w + TILE_GAP;
            if k == 0
                && let Some((text, _)) = &self.message
            {
                let w = measure(text, FontId::proportional(14.0));
                out.push((x, w));
                x += w + TILE_GAP;
            }
        }
        let height = if self.impact.is_some() { OVERLAY_IMPACT_H } else { OVERLAY_SIZE.y };
        (out, height)
    }

    /// Room for the impact graphics on the right (none without them).
    fn graphics_width(&self) -> f32 {
        let count = self.impact.map_or(0, |i| usize::from(i.side) + usize::from(i.top));
        count as f32 * (impact_panel_width(self.one_line) + 1.0)
    }
}

/// Width of a tile's contents.
fn cell_width(cell: &Cell, word: &str, one_line: bool, measure: &dyn Fn(&str, FontId) -> f32) -> f32 {
    match cell {
        Cell::Status => 16.0 + measure(word, cond(if one_line { 20.0 } else { 24.0 })),
        Cell::Value { glyph, text, .. } => (if glyph.is_some() { GLYPH_W } else { 0.0 }) + measure(text, FontId::proportional(14.0)),
        Cell::Battery(level) => 32.0 + measure(&format!("{level} %"), FontId::proportional(13.0)),
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
    // The impact graphics take the right end; the tiles the rest.
    let graphics = c.graphics_width();
    if let Some(impact) = c.impact {
        let w = impact_panel_width(c.one_line);
        let labels = !c.one_line;
        let mut x = rect.right() - graphics + 1.0;
        let panels: [(bool, ImpactGraphic); 2] = [(impact.side, impact_side), (impact.top, impact_top)];
        for (_, draw) in panels.into_iter().filter(|(on, _)| *on) {
            let r = Rect::from_min_size(pos2(x, rect.top()), vec2(w, rect.height()));
            p.line_segment([pos2(r.left() - 0.5, rect.top() + 1.0), pos2(r.left() - 0.5, rect.bottom() - 1.0)], Stroke::new(1.0, LINE));
            draw(p, r, impact, labels);
            x += w + 1.0;
        }
    }

    let measure = |text: &str, font: FontId| p.layout_no_wrap(text.to_string(), font, WHITE).size().x;
    let (tiles, _) = c.tiles(&measure);
    let draw_cell = |cell: &Cell, at: egui::Pos2| match cell {
        Cell::Status => {
            p.circle_filled(pos2(at.x + 4.0, at.y), 5.0, c.color);
            caps_text(p, pos2(at.x + 16.0, at.y), egui::Align::Min, c.word, if c.one_line { 20.0 } else { 24.0 }, c.color);
        }
        Cell::Value { glyph, text, color } => {
            let mut x = at.x;
            if let Some(g) = glyph {
                draw_glyph(p, pos2(x + 8.0, at.y), *g, LABEL);
                x += GLYPH_W;
            }
            p.text(pos2(x, at.y), Align2::LEFT_CENTER, text, FontId::proportional(14.0), *color);
        }
        Cell::Battery(level) => {
            let w = cell_width(cell, c.word, c.one_line, &measure);
            draw_battery(p, pos2(at.x + w, at.y), *level);
        }
    };
    // Tiles of the layout drawn, by their place in it.
    let places: Vec<(usize, usize)> = {
        let mut v = Vec::new();
        for (k, [a, b]) in c.columns.iter().enumerate() {
            let keep = |x: &Option<Cell>| x.as_ref().is_some_and(|x| c.message.is_none() || k == 0 || matches!(x, Cell::Battery(_)));
            if keep(a) || keep(b) {
                v.push((k, [keep(a), keep(b)]));
            }
        }
        v.into_iter().flat_map(|(k, kept)| (0..2).filter(move |r| kept[*r]).map(move |r| (k, r))).collect()
    };
    let mut hit = Vec::new();
    let message_color = c.message.as_ref().map_or(WHITE, |m| m.1);
    if c.one_line {
        let y = rect.center().y;
        let mut tile = tiles.iter();
        let mut first = true;
        for (k, r) in &places {
            let Some(cell) = c.columns[*k][*r].as_ref() else { continue };
            let Some(&(x, w)) = tile.next() else { break };
            draw_cell(cell, pos2(rect.left() + x, y));
            hit.push((*k, *r, Rect::from_min_size(pos2(rect.left() + x, rect.top()), vec2(w, rect.height()))));
            if first {
                first = false;
                // The message has its own place after the first tile.
                if let Some((text, _)) = &c.message
                    && let Some(&(mx, _)) = tile.next()
                {
                    p.text(pos2(rect.left() + mx, y), Align2::LEFT_CENTER, text, FontId::proportional(14.0), message_color);
                }
            }
        }
        return hit;
    }
    // Two rows, a thin line between the columns.
    let inset = 20.0 + ((rect.height() - OVERLAY_SIZE.y) / 2.0).max(0.0);
    let rows = [rect.top() + inset, rect.bottom() - inset];
    let columns = c.shown_columns();
    let mut tile = tiles.iter();
    let mut k_layout = places.iter().map(|(k, _)| *k).collect::<Vec<_>>();
    k_layout.dedup();
    for (n, col) in columns.iter().enumerate() {
        let Some(&(x, w)) = tile.next() else { break };
        let k = k_layout.get(n).copied().unwrap_or(n);
        if n > 0 {
            let lx = rect.left() + x - TILE_GAP / 2.0;
            p.line_segment([pos2(lx, rect.top() + 12.0), pos2(lx, rect.bottom() - 12.0)], Stroke::new(1.0, LINE));
        }
        for (r, cell) in col.iter().enumerate() {
            let y = rows[r];
            if let Some(cell) = cell {
                draw_cell(cell, pos2(rect.left() + x, y));
            }
            let half = (rows[1] - rows[0]) / 2.0;
            hit.push((k, r, Rect::from_center_size(pos2(rect.left() + x + w / 2.0, y), vec2(w + TILE_GAP - 2.0, half * 2.0))));
        }
        if n == 0
            && let Some((text, color)) = &c.message
            && let Some(&(mx, _)) = tile.next()
        {
            p.text(pos2(rect.left() + mx, rect.center().y), Align2::LEFT_CENTER, text, FontId::proportional(14.0), *color);
        }
    }
    hit
}

/// Symbols in front of the overlay's values.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Glyph {
    /// A golf ball with speed lines: ball speed.
    BallSpeed,
    /// An iron's head on its shaft: club speed.
    ClubSpeed,
    /// Club face meeting the ball: smash factor.
    Smash,
    /// A golf ball with an arrow around it: spin.
    Spin,
    /// A flag: distance to the pin.
    Pin,
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
            // Shaft down to an iron's head: a blade with a flat sole and a rounded toe.
            p.line_segment([c + vec2(5.5, -8.0), c + vec2(2.5, 2.5)], Stroke::new(1.5, color));
            let head = vec![
                c + vec2(1.5, 1.5),
                c + vec2(4.0, 1.5),
                c + vec2(4.0, 6.5),
                c + vec2(-5.5, 7.5),
                c + vec2(-7.5, 6.5),
                c + vec2(-7.5, 4.0),
                c + vec2(-5.0, 2.5),
            ];
            p.add(egui::Shape::convex_polygon(head, color, Stroke::NONE));
            // Grooves.
            for y in [4.0, 5.6] {
                p.line_segment([c + vec2(-5.5, y), c + vec2(1.5, y - 0.4)], Stroke::new(0.7, PAGE));
            }
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
        Glyph::Pin => {
            p.line_segment([c + vec2(-3.0, 7.5), c + vec2(-3.0, -7.5)], Stroke::new(1.4, color));
            p.add(egui::Shape::convex_polygon(vec![c + vec2(-2.5, -7.5), c + vec2(6.5, -4.6), c + vec2(-2.5, -1.7)], RED, Stroke::NONE));
            p.add(egui::Shape::ellipse_filled(c + vec2(-3.0, 7.5), vec2(4.0, 1.3), color.gamma_multiply(0.6)));
        }
    }
}

/// A shot shape's name in the language shown ("Straight" is the only one golfers translate).
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
    let bend = (i.spin_axis as f32 * 2.2).clamp(-40.0, 40.0);
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
        let shape = Shape::of(i.hla, i.spin_axis, false);
        p.text(r.left_top() + vec2(6.0, 4.0), Align2::LEFT_TOP, shape_name(shape), FontId::proportional(11.5), WHITE);
    }
    if labels {
        let mut parts: Vec<(String, Color32)> = i.path.map(|path| (format!("{} {}", tr("Path"), dir(path, "I-O", "O-I")), ICON)).into_iter().collect();
        parts.push((format!("Start {}", dir(i.hla, "R", "L")), WHITE));
        impact_caption(p, r, &parts);
    }
}

/// The overlay position as a small screen with eight spots; the middle turns the overlay off.
fn overlay_picker(ui: &mut egui::Ui, value: &mut OverlayPos) {
    let (screen, _) = ui.allocate_exact_size(vec2(128.0, 72.0), Sense::hover());
    ui.painter().rect(screen, 6.0, PAGE, Stroke::new(2.0, FIELD_LINE), StrokeKind::Inside);
    let inner = screen.shrink2(vec2(10.0, 7.0));
    let cell = vec2(inner.width() / 3.0, inner.height() / 3.0);
    for (row, cells) in OverlayPos::GRID.iter().enumerate() {
        for (col, &pos) in cells.iter().enumerate() {
            let c = inner.min + vec2((col as f32 + 0.5) * cell.x, (row as f32 + 0.5) * cell.y);
            let hit = Rect::from_center_size(c, cell);
            let resp = ui.interact(hit, ui.id().with(("overlay_pos", row, col)), Sense::click());
            let selected = *value == pos;
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
            }
        }
    }
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
                        let impact = Impact { side: false, top: true, spin_axis, aoa: None, vla: 15.0, path: Some(path), hla };
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
    }

    /// Layer transforms apply to every viewport: scaling the overlay must not shrink the main window.
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
        assert_eq!(ctx.layer_transform_to_global(egui::LayerId::background()), None);
        assert!(ctx.layer_transform_to_global(overlay_layer()).is_some(), "overlay drawn");
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
