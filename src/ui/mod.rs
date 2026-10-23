//! The desktop app around the R10 → GSPro bridge (egui/eframe), styled after GSPro's in-game
//! interface: pages, overlay windows, the shots explorer. Behind the `gui` feature; the binary
//! `openlaunch-connect` only calls [`run`]. Another crate builds its own app with
//! [`run_with`] and its own [`Page`]s.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, Instant};

/// The egui and eframe this UI is built with, for code that adds to it.
pub use eframe::{self, egui};
use eframe::egui::{
    Align2, Color32, CornerRadius, FontId, Margin, Rect, RichText, Sense, Stroke, StrokeKind,
    pos2, vec2,
};
use crate::engine;
use crate::gspro::Cmd;
use crate::history::{self, ExportFormat, ShotRecord};
use crate::i18n::{self, Language, tr, tr_detail};
use crate::launcher;
use crate::num;
use crate::log;
use crate::misread::Misread;
use crate::settings::{Arrangement, DEFAULT_ARRANGEMENT, OverlayItem, OverlayPos, OverlayStyle, Settings, TileOverlay};
use crate::shape::{Curve, Shape};
use crate::state::{self, R10Status};
use crate::update;

pub mod explorer;
pub mod kit;
#[cfg(target_os = "macos")]
mod mac;
mod overlay;
mod page;

use kit::*;
pub use page::{AppCtx, Page};
use overlay::*;

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

/// Starts the bridge and runs the app until its window is closed.
///
/// # Errors
/// Returns eframe's error if the window can't be created.
pub fn run() -> eframe::Result {
    run_with(Vec::new())
}

/// Like [`run`], with more pages: `pages` (e.g. of another crate) come in the navigation after
/// the core pages, before *Settings*, in the order given.
///
/// # Errors
/// Returns eframe's error if the window can't be created.
pub fn run_with(pages: Vec<Box<dyn Page>>) -> eframe::Result {
    Settings::migrate_data_dir();
    let settings = Settings::load();
    i18n::set(settings.language);
    crate::notify::ENABLED.store(settings.notifications, std::sync::atomic::Ordering::Relaxed);
    log::info(&format!("{APP_NAME} {} started", env!("CARGO_PKG_VERSION")));
    let tx = engine::start(settings.to_config());
    crate::gspro::DEBUG.store(settings.debug, std::sync::atomic::Ordering::Relaxed);

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
            Ok(Box::new(App::with_pages(settings, tx, pages)))
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

struct App {
    tx: Sender<Cmd>,
    saved: Settings,
    /// The Settings page's copy, which every change through `AppCtx::change_settings` reaches too.
    edit: Settings,
    /// The pages in the order of the navigation (`page::pages`), and the one shown.
    pages: Vec<Box<dyn Page>>,
    page: usize,
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
    /// Shots page: page shown, filter and selection; the tile overlays' cameras and animation.
    explorer: explorer::ExplorerState,
    /// Where the status overlay is on the screen this frame (native points), if shown.
    status_rect: Option<Rect>,
    /// Like `overlay_first`, for the tile overlays.
    tile_overlay_first: HashMap<usize, (egui::Pos2, egui::Vec2)>,
    /// An overlay being moved by hand.
    moving: Option<Moving>,
    /// Whether clicks were last set to go through the status (`None`) and the tile overlays.
    passthrough: HashMap<Option<usize>, bool>,
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
    /// The app with the core pages.
    #[cfg(test)]
    fn new(settings: Settings, tx: Sender<Cmd>) -> Self {
        Self::with_pages(settings, tx, Vec::new())
    }

    /// The app with the core pages and `extra` (see [`run_with`]).
    fn with_pages(mut settings: Settings, tx: Sender<Cmd>, extra: Vec<Box<dyn Page>>) -> Self {
        // The overlays as they are become the first arrangement.
        if settings.arrangements.is_empty() {
            settings.keep_arrangement();
        }
        let mut edit = settings.clone();
        if edit.launcher_path.is_none() {
            edit.launcher_path = launcher::find_gspro_launchers().into_iter().next();
        }
        let pages = page::pages(extra);
        // OPENLAUNCH_PAGE=<id> opens another page first (e.g. settings, log).
        let page = page::start_page(&pages, std::env::var("OPENLAUNCH_PAGE").ok().as_deref());
        Self {
            tx,
            saved: settings,
            edit,
            pages,
            page,
            capture: Capture::from_env(),
            show_overlay: true,
            remember_size: true,
            size_changed: None,
            aspect_locked: None,
            applied_scale: None,
            overlay_first: None,
            screen_size: None,
            all_covered: false,
            overlay_base: std::cell::Cell::new(egui::Vec2::ZERO),
            explorer: explorer::ExplorerState::default(),
            status_rect: None,
            tile_overlay_first: HashMap::new(),
            moving: None,
            passthrough: HashMap::new(),
        }
    }

    /// What the pages share, for the app's own use (the overlay windows, the window size).
    fn ctx(&mut self) -> AppCtx<'_> {
        AppCtx::new(&mut self.saved, &mut self.edit, &self.tx, &mut self.explorer, &mut self.moving)
    }

    /// Shows the page `id` (tests; `OPENLAUNCH_PAGE` in the app).
    #[cfg(test)]
    fn show_page(&mut self, id: &str) {
        self.page = self.pages.iter().position(|p| p.id() == id).expect("no such page");
    }

    /// The page of type `P`, to look at or set up its state (tests).
    #[cfg(test)]
    fn page_mut<P: Page>(&mut self) -> &mut P {
        self.pages.iter_mut().find_map(|p| (p.as_mut() as &mut dyn std::any::Any).downcast_mut::<P>()).expect("no such page")
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

        // Navigation, right-aligned: the last page rightmost.
        let mut x = rect.right() - 18.0;
        for page in (0..self.pages.len()).rev() {
            let label = self.pages[page].label();
            let galley = ui.painter().layout_no_wrap(label.into(), cond(19.0), WHITE);
            let w = galley.size().x;
            let hit = Rect::from_min_max(pos2(x - w - 6.0, rect.top()), pos2(x + 6.0, rect.bottom()));
            let resp = ui.interact(hit, ui.id().with(label), Sense::click());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
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
}

/// *Play*: the last shot's numbers, the test shot and starting GSPro, the status.
struct PlayPage;

impl Page for PlayPage {
    fn id(&self) -> &'static str {
        "play"
    }

    fn label(&self) -> &'static str {
        tr("PLAY")
    }

    fn ui(&mut self, ui: &mut egui::Ui, app: &mut AppCtx<'_>) {
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
        let metric = app.settings().metric;
        let speed = |mph: f64| if metric { mph * MPH_TO_KMH } else { mph };
        let unit = if metric { "km/h" } else { "mph" };

        let head = section(ui, tr("BALL"));
        // With more than one arrangement of the overlays: which one, to switch.
        if app.settings().arrangements.len() > 1 {
            let text = format!("{}: {}", tr("Overlay"), app.arrangement_name(app.settings().arrangement));
            let galley = ui.painter().layout_no_wrap(text, FontId::proportional(13.0), SOFT);
            let r = Rect::from_min_size(pos2(ui.max_rect().right() - galley.size().x - 16.0, head.rect.center().y - galley.size().y / 2.0 - 2.0), galley.size() + vec2(16.0, 0.0));
            let resp = ui.interact(r, ui.id().with("play-arrangement"), Sense::click());
            let color = if resp.hovered() { WHITE } else { SOFT };
            ui.painter().galley_with_override_text_color(r.min, galley, color);
            let c = pos2(r.right() - 5.0, r.center().y);
            ui.painter().add(egui::Shape::convex_polygon(vec![c + vec2(-4.0, -2.0), c + vec2(4.0, -2.0), c + vec2(0.0, 3.0)], color, Stroke::NONE));
            let resp = resp.on_hover_text(tr("Arrangement of the overlays")).on_hover_cursor(egui::CursorIcon::PointingHand);
            // Only choosing here: nothing to delete.
            let _ = app.arrangement_menu(&resp, false);
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
            let tx = app.commands().clone();
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
        let launcher = app.settings().launcher_path.clone().or_else(|| app.edit.launcher_path.clone());
        let why = if gs_ok { tr("GSPro is already connected") } else { tr("No GSPro launcher selected (Settings)") };
        if button(ui, b2, tr("START GSPRO"), Icon::Play, false, launcher.is_some() && !gs_ok, why)
            && let Some(path) = launcher
        {
            let addr = app.settings().gspro_addr.clone();
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
}

/// *Settings*: edited in a copy (`AppCtx::edit`) and saved with SAVE; the interface size and the
/// reset of the shots view apply at once.
#[derive(Default)]
struct SettingsPage {
    /// The outcome of the last save or search, shown next to SAVE.
    note: Option<String>,
    /// Asking before the shots view is reset.
    confirm_reset: bool,
}

impl Page for SettingsPage {
    fn id(&self) -> &'static str {
        "settings"
    }

    fn label(&self) -> &'static str {
        tr("SETTINGS")
    }

    fn ui(&mut self, ui: &mut egui::Ui, app: &mut AppCtx<'_>) {
        // 2 × 2 cards; cards side by side share their row's height.
        let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), 184.0 + 10.0 + 140.0), Sense::hover());
        let col_w = (area.width() - 10.0) / 2.0;
        let cell = |col: f32, top: f32, h: f32| {
            Rect::from_min_size(pos2(area.left() + col * (col_w + 10.0), area.top() + top), vec2(col_w, h))
        };
        let (r10_device, r10_battery, r10) = state::with(|s| (s.r10_device.clone(), s.r10_battery, s.r10));

        card(ui, cell(0.0, 0.0, 184.0), tr("GSPRO"), Icon::Monitor, |ui| {
            setting_row(ui, tr("Launcher"), |ui| {
                let text = app.edit.launcher_path.as_ref().map_or_else(
                    || tr("– not selected –").to_string(),
                    |p| short_path(p.as_path()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    #[cfg(target_os = "macos")]
                    if secondary_button(ui, tr("Choose …"), true).clicked()
                        && let Some(p) = rfd::FileDialog::new()
                            .set_title(tr("Choose GSPro launcher (CrossOver app)"))
                            .add_filter("Applications", &["app"])
                            .pick_file()
                    {
                        app.edit.launcher_path = Some(p);
                    }
                    if secondary_button(ui, tr("Find"), true).clicked() {
                        match launcher::find_gspro_launchers().into_iter().next() {
                            Some(p) => app.edit.launcher_path = Some(p),
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
                    egui::TextEdit::singleline(&mut app.edit.gspro_addr)
                        .desired_width(130.0)
                        .vertical_align(egui::Align::Center)
                        // Height from the margin: 34 px like the other controls.
                        .margin(Margin::symmetric(10, 11)),
                );
                ui.label(RichText::new(tr("Applies after restart")).size(12.0).color(MUTED));
            });
            setting_row(ui, tr("Autostart"), |ui| {
                toggle(ui, &mut app.edit.auto_start_gspro, tr("Open GSPro on start"));
            });
        });

        card(ui, cell(1.0, 0.0, 184.0), tr("DISPLAY"), Icon::Eye, |ui| {
            setting_row(ui, tr("Language"), |ui| {
                egui::ComboBox::from_id_salt("language")
                    .width(128.0)
                    .selected_text(app.edit.language.label())
                    .show_ui(ui, |ui| {
                        for l in Language::ALL {
                            ui.selectable_value(&mut app.edit.language, l, l.label());
                        }
                    });
            });
            setting_row(ui, tr("Units"), |ui| {
                segments(ui, &mut app.edit.metric, &[(true, "km/h"), (false, "mph")]);
                ui.add_space(4.0);
                segments(ui, &mut app.edit.meters, &[(true, "m"), (false, "yd")]);
            });
            setting_row(ui, tr("Interface"), |ui| {
                let options = [(None, tr("Auto")), (Some(1.0), "100 %"), (Some(1.25), "125 %"), (Some(1.5), "150 %")];
                let mut scale = app.settings().ui_scale;
                // Sizes whose smallest window doesn't fit on the screen can't be chosen.
                let room = screen_room(ui.ctx());
                let fits = |s: Option<f32>| room.is_none_or(|r| min_size(s).x <= r.x + 0.5 && min_size(s).y <= r.y + 0.5);
                if segments_where(ui, &mut scale, &options, fits, tr("Too large for this screen")) {
                    // Applies and saves at once, like the overlay's settings.
                    let _ = app.change_settings(|s| s.ui_scale = scale);
                }
            });
        });

        card(ui, cell(0.0, 194.0, 140.0), "R10", Icon::Signal, |ui| {
            setting_row(ui, tr("Ball distance"), |ui| {
                stepper(ui, &mut app.edit.tee_distance, 1.5..=4.0, 0.05, "m")
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
                if toggle(ui, &mut app.edit.notifications, tr("Notify about problems"))
                    .on_hover_text(tr("Low R10 battery, R10 errors (e.g. tilted), lost connections, shots not delivered"))
                    .changed()
                {
                    // Takes effect immediately; saved with the other settings.
                    crate::notify::ENABLED.store(app.edit.notifications, std::sync::atomic::Ordering::Relaxed);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if secondary_button(ui, tr("Test"), true)
                        .on_hover_text(tr("Sends a notification now. If none appears, allow notifications for OpenLaunch Connect in System Settings → Notifications."))
                        .clicked()
                    {
                        crate::notify::send_now("Notifications are working.");
                    }
                });
            });
            setting_row(ui, tr("Updates"), |ui| {
                toggle(ui, &mut app.edit.check_updates, tr("Check on start"))
                    .on_hover_text(tr("Asks GitHub for the latest release when the app starts"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if secondary_button(ui, tr("Update"), check != Some(update::Check::Running))
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
        let dirty = *app.edit != *app.settings();
        let save_r = Rect::from_min_max(pos2(bar.right() - 160.0, bar.top()), bar.max);
        if button(ui, save_r, tr("SAVE"), Icon::None, true, dirty, tr("No changes")) {
            let restart = app.edit.gspro_addr != app.settings().gspro_addr
                || (app.edit.tee_distance - app.settings().tee_distance).abs() > f32::EPSILON;
            let edit = app.edit.clone();
            match app.change_settings(|s| s.clone_from(&edit)) {
                Ok(()) => {
                    i18n::set(app.settings().language);
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
        let is_default = app.settings().explorer == crate::settings::Explorer::default();
        if button(ui, reset_r, label, Icon::None, false, !is_default, tr("The shots view is as it came")) {
            self.confirm_reset = true;
        }
        if self.confirm_reset {
            let text = tr("All pages and tiles go back to how they came. Your shots are kept.");
            if let Some(reset) = confirm(ui.ctx(), "reset-shot-pages", tr("Reset the shots view?"), text, tr("RESET")) {
                self.confirm_reset = false;
                if reset {
                    self.note = Some(match app.change_settings(|s| s.explorer = crate::settings::Explorer::default()) {
                        Ok(()) => tr("Shots view reset.").into(),
                        Err(e) => tr("Saving failed: {e}").replace("{e}", &e),
                    });
                }
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
}

/// *Log*: the app log, with copying it and the switch for GSPro's messages.
struct LogPage;

impl Page for LogPage {
    fn id(&self) -> &'static str {
        "log"
    }

    fn label(&self) -> &'static str {
        tr("LOG")
    }

    fn ui(&mut self, ui: &mut egui::Ui, app: &mut AppCtx<'_>) {
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
            let mut debug = app.settings().debug;
            if toggle(ui, &mut debug, tr("Log GSPro messages")).changed() {
                crate::gspro::DEBUG.store(debug, std::sync::atomic::Ordering::Relaxed);
                let _ = app.change_settings(|s| s.debug = debug);
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
                let mut app = AppCtx::new(&mut self.saved, &mut self.edit, &self.tx, &mut self.explorer, &mut self.moving);
                self.pages[self.page].ui(ui, &mut app);
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
            let _ = self.ctx().change_settings(|s| s.window_size = Some(size));
        }
    }
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
            let rgba: Vec<u8> = image.pixels.iter().flat_map(Color32::to_srgba_unmultiplied).collect();
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

/// A shot shape's name in the language shown ("Straight" is the only one golfers translate).
fn shape_name(shape: Shape) -> String {
    let name = shape.name();
    if name == "Straight" { tr("Straight").to_string() } else { name }
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
    let target = crate::settings::Settings::data_dir().map(|d| d.join(&name));
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
    use eframe::egui::FontFamily;

    /// Status as during a real session, for the README screenshots. Hold the returned lock while
    /// the test runs: the lib's tests beside it record shots into the same state.
    pub(super) fn demo_state() -> std::sync::MutexGuard<'static, ()> {
        let lock = crate::test_util::state_lock();
        // Screenshot tests drive the app; they must not touch the real settings file.
        crate::settings::SAVE.store(false, std::sync::atomic::Ordering::Relaxed);
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
            s.gspro_result = Some(crate::gspro_db::GsproResult {
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
                tempo: Some(crate::mapping::Tempo { backswing: 0.81, downswing: 0.27 }),
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
        lock
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
    /// `cargo test --lib readme_screenshots -- --ignored`
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
        let _state = demo_state();
        // Every core page (the Shots page again with a range session below).
        for page in page::pages(Vec::new()).iter().map(|p| p.id()) {
            let name = if page == "overlay" { "screenshot-overlay-settings.png".to_string() } else { format!("screenshot-{page}.png") };
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
            harness.state_mut().show_page(page);
            harness.state_mut().show_overlay = false; // rendered separately below
            harness.state_mut().remember_size = false;
            // Grids size their rows from the previous frame.
            for _ in 0..6 {
                harness.step();
            }
            save(&harness.render().unwrap(), &name);
        }

        // The shots explorer with a range session (seven of the fourteen clubs): its default pages,
        // a page with the correlation charts and averages, and the configure mode.
        state::with(|s| s.session = engine::demo_range());
        let first = state::with(|s| s.session.iter().rev().nth(10).map(|r| (r.time.clone(), r.shot_number)));
        let hidden: std::collections::BTreeSet<Option<String>> =
            ["W5", "H3", "H4", "I6", "I8", "GW", "LW"].into_iter().map(|c| Some(c.to_string())).collect();
        let analysis = {
            use crate::settings::{ExplorerPage, Tile, TileSize};
            let scatter = |x: &str, y: &str| Tile::new("scatter", TileSize::W2H2).with(&serde_json::json!({ "x": x, "y": y }));
            ExplorerPage {
                name: "Analysis".into(),
                tiles: vec![
                    scatter("face_to_path", "spin_axis"),
                    scatter("ball_speed", "carry"),
                    Tile::new("table", TileSize::W2H2).with(&serde_json::json!({ "averages": true })),
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
                settings.explorer.pages[2].tiles[0].size = crate::settings::TileSize::W3H2;
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
            app.show_page("shots");
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
    /// `OPENLAUNCH_SCREENSHOT_DIR=<dir> cargo test --lib zoomed_window -- --ignored`
    #[test]
    #[ignore = "writes screenshots; run on demand"]
    fn zoomed_window() {
        use egui_kittest::Harness;

        i18n::set(Language::English);
        let _state = demo_state();
        for (page, name) in [("play", "zoomed-play.png"), ("shots", "zoomed-shots.png")] {
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
            harness.state_mut().show_page(page);
            harness.state_mut().remember_size = false;
            for _ in 0..8 {
                harness.step();
            }
            assert!((harness.ctx.zoom_factor() - fit_zoom(vec2(1700.0, 780.0))).abs() < 0.01);
            save(&harness.render().unwrap(), name);
        }
    }

    /// Renders every page in a 1600×900 window at a fixed interface size of 100 %, where the pages
    /// get the extra room: `OPENLAUNCH_SCREENSHOT_DIR=<dir> cargo test --lib large_window -- --ignored`
    #[test]
    #[ignore = "writes screenshots; run on demand"]
    fn large_window() {
        use egui_kittest::Harness;

        i18n::set(Language::English);
        let _state = demo_state();
        for page in page::pages(Vec::new()).iter().map(|p| p.id()) {
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
            harness.state_mut().show_page(page);
            harness.state_mut().remember_size = false;
            for _ in 0..8 {
                harness.step();
            }
            assert_eq!(harness.ctx.zoom_factor(), 1.0);
            save(&harness.render().unwrap(), &format!("large-{page}.png"));
        }
    }

    /// Settings changed at once (window size, overlays, arrangements) reach the Settings page's
    /// copy, without saving or losing what is edited there.
    #[test]
    fn changes_outside_the_settings_page_keep_its_unsaved_edits() {
        crate::settings::SAVE.store(false, std::sync::atomic::Ordering::Relaxed);
        let settings = Settings { launcher_path: Some(PathBuf::from("/GSPro.app")), ..Settings::default() };
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut app = App::new(settings, tx);
        assert_eq!(app.edit, app.saved);
        // An unsaved edit on the Settings page, then changes applied at once elsewhere.
        app.edit.metric = false;
        app.ctx().change_settings(|s| s.window_size = Some([1100.0, 517.0])).unwrap();
        app.ctx().change_overlays(|s| s.tile_overlays.push(TileOverlay::default()));
        app.ctx().use_arrangement(|s| {
            s.arrangements.push(Arrangement { name: "Range".into(), ..Arrangement::default() });
            1
        });
        assert!(app.saved.metric && !app.edit.metric, "the edit stays unsaved");
        assert_eq!(app.edit, Settings { metric: false, ..app.saved.clone() }, "everything else is mirrored");
        assert_eq!((app.saved.arrangement, app.saved.tile_overlays.len()), (1, 0));
        assert_eq!(app.saved.arrangements[0].tiles.len(), 1, "kept in the arrangement it was added to");
        // The page's SAVE takes the edit.
        let edit = app.edit.clone();
        app.ctx().change_settings(|s| s.clone_from(&edit)).unwrap();
        assert_eq!(app.edit, app.saved);
        assert!(!app.saved.metric);
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
