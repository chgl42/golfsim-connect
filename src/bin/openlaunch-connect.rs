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
use openlaunch_connect::history::{self, ExportFormat, ShotRecord, Stat};
use openlaunch_connect::i18n::{self, Language, tr, tr_detail};
use openlaunch_connect::launcher;
use openlaunch_connect::num;
use openlaunch_connect::log;
use openlaunch_connect::settings::{OverlayPos, OverlayStyle, Settings};
use openlaunch_connect::state::{self, R10Status};
use openlaunch_connect::update;

const APP_NAME: &str = "OpenLaunch Connect";
const MPH_TO_KMH: f64 = 1.609_344;
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

    fn shots(&mut self, ui: &mut egui::Ui) {
        let session: Vec<ShotRecord> = state::with(|s| s.session.clone());
        let metric = self.saved.metric;
        let unit = if metric { "km/h" } else { "mph" };
        let speed = move |mph: f64| if metric { mph * MPH_TO_KMH } else { mph };

        ui.horizontal(|ui| {
            section(ui, tr("BY CLUB"));
            ui.add_space(16.0);
            let before = self.all_time;
            ui.radio_value(&mut self.all_time, false, tr("This session ({n})").replace("{n}", &session.len().to_string()));
            ui.radio_value(&mut self.all_time, true, tr("All time"));
            if self.all_time && (!before || self.all_shots_session != session.len()) {
                self.all_shots = history::load_all();
                self.all_shots_session = session.len();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(150.0, 30.0), Sense::hover());
                let shown = if self.all_time { &self.all_shots } else { &session };
                if button(ui, rect, tr("EXPORT CSV"), Icon::None, false, !shown.is_empty(), tr("No shots to export")) {
                    self.export_note = Some(export_csv(shown, self.saved.export_format));
                }
                let before = self.saved.export_format;
                egui::ComboBox::from_id_salt("export_format")
                    .selected_text(before.label())
                    .show_ui(ui, |ui| {
                        for f in ExportFormat::ALL {
                            ui.selectable_value(&mut self.saved.export_format, f, f.label());
                        }
                    })
                    .response
                    .on_hover_text(tr("CSV layout: OpenLaunch, or as exported by the Garmin Golf app or Awesome Golf"));
                if self.saved.export_format != before {
                    // Remember the choice without touching unsaved edits on the settings page.
                    self.edit.export_format = self.saved.export_format;
                    let _ = self.saved.save();
                }
                if let Some(note) = &self.export_note {
                    ui.label(RichText::new(note).color(MUTED));
                }
            });
        });
        let shots = if self.all_time { &self.all_shots } else { &session };
        ui.add_space(4.0);

        let fmt = |st: Option<Stat>, digits: usize, f: &dyn Fn(f64) -> f64| {
            st.map_or_else(
                || "–".to_string(),
                |st| format!("{} ±{}", num(f(st.mean), digits), num(f(st.sd), digits)),
            )
        };
        let id = |v: f64| v;
        egui::Frame::new().fill(TILE).inner_margin(Margin::same(10)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            if shots.is_empty() {
                ui.label(RichText::new(tr("No shots yet.")).color(MUTED));
                return;
            }
            egui::ScrollArea::vertical().id_salt("by_club").max_height(130.0).show(ui, |ui| {
                egui::Grid::new("by_club_grid").num_columns(10).spacing([22.0, 5.0]).striped(true).show(ui, |ui| {
                    for h in [
                        tr("Club").to_string(),
                        tr("Shots").into(),
                        format!("{} ({unit})", tr("Ball")),
                        format!("{} ({unit})", tr("Club")),
                        tr("Smash").into(),
                        "VLA".into(),
                        tr("Spin").into(),
                        "HLA".into(),
                        tr("Face to Path").into(),
                        tr("Tempo").into(),
                    ] {
                        ui.label(RichText::new(h).color(MUTED));
                    }
                    ui.end_row();
                    for c in history::club_stats(shots) {
                        let name = c.club_code.as_deref().map_or(tr("Unknown").to_string(), club_name);
                        ui.label(RichText::new(name).color(WHITE));
                        ui.label(c.shots.to_string());
                        ui.label(fmt(c.ball_speed, 1, &speed));
                        ui.label(fmt(c.club_speed, 1, &speed));
                        ui.label(fmt(c.smash, 2, &id));
                        ui.label(fmt(c.vla, 1, &id));
                        ui.label(fmt(c.total_spin, 0, &id));
                        ui.label(fmt(c.hla, 1, &id));
                        ui.label(fmt(c.face_to_path, 1, &id));
                        ui.label(fmt(c.tempo, 1, &id));
                        ui.end_row();
                    }
                });
            });
        });

        ui.add_space(10.0);
        section(ui, tr("SHOTS"));
        // (time, shot number, excluded) of a shot whose switch was clicked, applied after the table.
        let mut toggled: Option<(String, u32, bool)> = None;
        egui::Frame::new().fill(TILE).inner_margin(Margin::same(10)).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("shots").auto_shrink([false, false]).show(ui, |ui| {
                egui::Grid::new("shots_grid").num_columns(12).spacing([22.0, 5.0]).striped(true).show(ui, |ui| {
                    for h in [
                        tr("Time"),
                        tr("Club"),
                        tr("Ball"),
                        "VLA",
                        "HLA",
                        tr("Spin"),
                        tr("Axis"),
                        tr("Club"),
                        tr("Smash"),
                        tr("Face to Path"),
                        tr("Tempo"),
                        "",
                    ] {
                        ui.label(RichText::new(h).color(MUTED));
                    }
                    ui.end_row();
                    for s in shots.iter().rev() {
                        let c = s.club.as_ref();
                        // Excluded shots stay listed, struck through and dimmed.
                        let cell = |text: String| {
                            let text = RichText::new(text);
                            if s.excluded { text.color(MUTED).strikethrough() } else { text }
                        };
                        let time = if self.all_time { s.time.get(..16).unwrap_or(&s.time).replace('T', " ") } else { s.clock().to_string() };
                        let time = if s.delivered { cell(time) } else { cell(format!("{time} ✗")).color(AMBER) };
                        ui.label(time).on_hover_text(if s.delivered { tr("Delivered to GSPro") } else { tr("Not delivered to GSPro") });
                        ui.label(cell(s.club_code.as_deref().unwrap_or("–").into()));
                        ui.label(cell(format!("{:.1}", speed(s.ball.speed))));
                        ui.label(cell(num(s.ball.vla, 1)));
                        ui.label(cell(num(s.ball.hla, 1)));
                        ui.label(cell(format!("{:.0}", s.ball.total_spin)));
                        ui.label(cell(num(s.ball.spin_axis, 1)));
                        ui.label(cell(c.map_or("–".into(), |c| format!("{:.1}", speed(c.speed)))));
                        ui.label(cell(s.smash().map_or("–".into(), |v| format!("{v:.2}"))));
                        ui.label(cell(c.map_or("–".into(), |c| dir(c.face_to_target - c.path, "O", "C"))));
                        ui.label(cell(s.tempo.map_or("–".into(), |t| format!("{:.1}", t.ratio()))));
                        let (label, hint) = if s.excluded {
                            (tr("Count"), tr("Count this shot in the stats and exports again"))
                        } else {
                            (tr("Exclude"), tr("Leave this shot out of the stats and exports (e.g. a misread); it stays listed"))
                        };
                        let link = egui::Button::new(RichText::new(label).size(12.0).color(if s.excluded { ICON } else { MUTED })).frame(false);
                        if ui.add(link).on_hover_text(hint).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            toggled = Some((s.time.clone(), s.shot_number, !s.excluded));
                        }
                        ui.end_row();
                    }
                });
            });
        });
        if let Some((time, number, excluded)) = toggled {
            let mark = |shots: &mut Vec<ShotRecord>| {
                for s in shots.iter_mut().filter(|s| s.time == time && s.shot_number == number) {
                    s.excluded = excluded;
                }
            };
            state::with(|s| mark(&mut s.session));
            mark(&mut self.all_shots);
            if let Err(e) = history::set_excluded(&time, number, excluded) {
                self.export_note = Some(tr("Saving failed: {e}").replace("{e}", &e));
            }
        }
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
        let content = OverlayContent::live(style, self.saved.metric);
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
            let s = &mut style;
            let items: [(&mut bool, &str); 10] = [
                (&mut s.club, tr("Club")),
                (&mut s.battery, tr("Battery")),
                (&mut s.tempo, tr("Tempo")),
                (&mut s.ball_speed, tr("Ball Speed")),
                (&mut s.vla, "VLA"),
                (&mut s.hla, "HLA"),
                (&mut s.spin, tr("Total Spin")),
                (&mut s.spin_axis, tr("Spin Axis")),
                (&mut s.club_speed, tr("Club Speed")),
                (&mut s.smash, tr("Smash Factor")),
            ];
            let body = ui.max_rect();
            let cell_w = body.width() / 2.0;
            for (i, (on, label)) in items.into_iter().enumerate() {
                let r = Rect::from_min_size(body.min + vec2((i % 2) as f32 * cell_w, (i / 2) as f32 * 38.0), vec2(cell_w, 34.0));
                ui.scope_builder(egui::UiBuilder::new().max_rect(r).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
                    toggle(ui, on, label);
                });
            }
        });
        ui.painter().text(
            pos2(right.right() - 16.0, right.top() + 21.0),
            Align2::RIGHT_CENTER,
            tr("The status is always shown"),
            FontId::proportional(12.0),
            MUTED,
        );

        // Below the cards, down to the page's bottom margin (the cards move the layout cursor).
        let preview = Rect::from_min_max(pos2(area.left(), area.bottom() + 10.0), pos2(area.right(), ui.max_rect().bottom()));
        self.overlay_preview(ui, preview, pos, &style);

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
    fn overlay_preview(&self, ui: &egui::Ui, area: Rect, pos: OverlayPos, style: &OverlayStyle) {
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
        let content = OverlayContent::new(style, self.saved.metric, ready, Some("I7"), Some(82), Some(&self.preview_shot));
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
        overlay_ui(&ui.ctx().layer_painter(layer).with_clip_rect(clip), Rect::from_min_size(egui::Pos2::ZERO, size), &content);
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
/// Space for the battery at the right of the overlay's status row.
const OVERLAY_BATTERY_W: f32 = 96.0;

/// What the overlay shows: the status word, the chosen extras and one line with the
/// chosen values of the last shot (or what's wrong).
struct OverlayContent {
    color: Color32,
    word: &'static str,
    club: Option<String>,
    tempo: Option<String>,
    battery: Option<u8>,
    line: String,
    line_color: Color32,
    one_line: bool,
    opacity: f32,
}

impl OverlayContent {
    /// From the shared status.
    fn live(style: &OverlayStyle, metric: bool) -> Self {
        let (r10, r10_detail, gs_ok, club, battery, last) = state::with(|s| {
            (s.r10, s.r10_detail.clone(), s.gspro_connected, s.gspro_club.clone(), s.r10_battery, s.last_shot.clone())
        });
        Self::new(style, metric, (r10, &r10_detail, gs_ok), club.as_deref(), battery, last.as_ref())
    }

    fn new(
        style: &OverlayStyle,
        metric: bool,
        (r10, r10_detail, gs_ok): (R10Status, &str, bool),
        club: Option<&str>,
        battery: Option<u8>,
        last: Option<&state::LastShot>,
    ) -> Self {
        let (color, word) = status_word(r10, gs_ok);
        let ready = r10 == R10Status::Ready && gs_ok;
        let tempo = last
            .and_then(|s| s.tempo)
            .filter(|_| style.tempo && ready)
            .map(|t| format!("{} {:.1}:1", tr("Tempo"), t.ratio()));
        let (line, line_color) = match (last, ready) {
            (_, false) if r10 != R10Status::Ready => (tr_detail(r10_detail), AMBER),
            (_, false) => (tr("GSPro not connected").to_string(), AMBER),
            (Some(s), true) if !s.delivered => (tr("Last shot NOT delivered to GSPro").to_string(), AMBER),
            (Some(s), true) => {
                let mut values = shot_values(s, style, metric);
                // One line: the tempo joins the values instead of sitting next to the battery.
                if style.one_line {
                    values.extend(tempo.clone());
                }
                (values.join("  ·  "), WHITE)
            }
            (None, true) => (tr("Hit away").to_string(), SOFT),
        };
        Self {
            color,
            word,
            club: club.filter(|_| style.club).map(club_name),
            tempo: tempo.filter(|_| !style.one_line),
            battery: battery.filter(|_| style.battery),
            line,
            line_color,
            one_line: style.one_line,
            opacity: style.opacity,
        }
    }

    /// Panel size at scale 1: two lines at least `OVERLAY_SIZE`, wider for long contents.
    fn size(&self, ctx: &egui::Context) -> egui::Vec2 {
        let w = |text: &str, font: FontId| ctx.fonts_mut(|f| f.layout_no_wrap(text.to_string(), font, WHITE).size().x);
        let extra = |text: &Option<String>, gap: f32| text.as_deref().map_or(0.0, |t| gap + w(t, FontId::proportional(14.0)));
        let battery = if self.battery.is_some() { OVERLAY_BATTERY_W } else { 14.0 };
        if self.one_line {
            let line = if self.line.is_empty() { 0.0 } else { 16.0 + w(&self.line, FontId::proportional(14.0)) };
            let width = 18.0 + 16.0 + w(self.word, cond(20.0)) + extra(&self.club, 12.0) + line + battery;
            vec2(width.ceil(), 40.0)
        } else {
            let top = 18.0 + 16.0 + w(self.word, cond(24.0)) + extra(&self.club, 12.0) + extra(&self.tempo, 24.0) + battery;
            let bottom = 18.0 + w(&self.line, FontId::proportional(14.0)) + 14.0;
            vec2(top.max(bottom).max(OVERLAY_SIZE.x).ceil(), OVERLAY_SIZE.y)
        }
    }
}

/// The chosen values of a shot, e.g. "198.3 km/h", "VLA 16.5°", "7146 rpm".
fn shot_values(s: &state::LastShot, style: &OverlayStyle, metric: bool) -> Vec<String> {
    let (factor, unit) = if metric { (MPH_TO_KMH, "km/h") } else { (1.0, "mph") };
    let b = &s.ball;
    let mut values = Vec::new();
    if style.ball_speed {
        values.push(format!("{:.1} {unit}", b.speed * factor));
    }
    if style.vla {
        values.push(format!("VLA {}°", num(b.vla, 1)));
    }
    if style.hla {
        values.push(format!("HLA {}°", num(b.hla, 1)));
    }
    if style.spin {
        values.push(format!("{:.0} rpm", b.total_spin));
    }
    if style.spin_axis {
        values.push(format!("{} {}°", tr("Axis"), num(b.spin_axis, 1)));
    }
    if let Some(c) = s.club.as_ref().filter(|_| style.club_speed) {
        values.push(format!("{} {:.1} {unit}", tr("Club"), c.speed * factor));
    }
    if let Some(smash) = history::smash(b, s.club.as_ref()).filter(|_| style.smash) {
        values.push(format!("Smash {smash:.2}"));
    }
    values
}

fn overlay_ui(p: &egui::Painter, rect: Rect, c: &OverlayContent) {
    p.rect(rect, 8.0, PAGE.gamma_multiply(c.opacity), Stroke::new(1.0, LINE), StrokeKind::Inside);
    p.rect_filled(
        Rect::from_min_size(rect.min + vec2(1.0, 1.0), vec2(5.0, rect.height() - 2.0)),
        CornerRadius { nw: 7, sw: 7, ne: 0, se: 0 },
        c.color,
    );

    let left = rect.left() + 18.0;
    let row_y = if c.one_line { rect.center().y } else { rect.top() + 20.0 };
    p.circle_filled(pos2(left + 4.0, row_y), 5.0, c.color);
    let size = if c.one_line { 20.0 } else { 24.0 };
    let mut x = caps_text(p, pos2(left + 16.0, row_y), egui::Align::Min, c.word, size, c.color).right();
    if let Some(club) = &c.club {
        x = p.text(pos2(x + 12.0, row_y), Align2::LEFT_CENTER, club, FontId::proportional(14.0), SOFT).right();
    }
    if let Some(level) = c.battery {
        draw_battery(p, pos2(rect.right() - 12.0, row_y), level);
    }
    if let Some(tempo) = &c.tempo {
        let right = rect.right() - if c.battery.is_some() { OVERLAY_BATTERY_W } else { 14.0 };
        p.text(pos2(right, row_y), Align2::RIGHT_CENTER, tempo, FontId::proportional(14.0), SOFT);
    }
    let line_pos = if c.one_line { pos2(x + 16.0, row_y) } else { pos2(left, rect.bottom() - 20.0) };
    p.text(line_pos, Align2::LEFT_CENTER, &c.line, FontId::proportional(14.0), c.line_color);
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
    fn demo_state() {
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
    fn save(img: &image::RgbaImage, name: &str) {
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

        let mut harness = Harness::builder()
            .with_size(OVERLAY_SIZE)
            .with_pixels_per_point(2.0)
            .wgpu()
            .build_ui(|ui| {
                // Fonts set in one frame apply from the next; draw only once they are there.
                if ui.ctx().fonts(|f| f.families().contains(&FontFamily::Name("cond".into()))) {
                    let content = OverlayContent::live(&OverlayStyle::default(), true);
                    let rect = Rect::from_min_size(ui.max_rect().min, content.size(ui.ctx()));
                    overlay_ui(ui.painter(), rect, &content);
                } else {
                    setup_style(ui.ctx());
                }
            });
        for _ in 0..3 {
            harness.step();
        }
        save(&harness.render().unwrap(), "screenshot-overlay.png");
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
