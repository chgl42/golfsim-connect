//! SHOTS page: the shots explorer. Pages of tiles on a grid of near-square cells; the filter and
//! the selection are shared by all tiles.
//!
//! What a tile shows is up to its kind ([`TileKind`]): the core's kinds are the modules below
//! (table, details, dispersion, correlation, 3D flight), another crate adds its own with
//! [`register_tile_kind`]. A tile of a kind this build doesn't have keeps its settings and shows
//! what it needs.

use std::any::{Any, TypeId};
use std::cell::{Cell, RefCell, RefMut};
use std::collections::{BTreeSet, HashMap};
use std::sync::{PoisonError, RwLock};

use crate::flight::{self, Flight};

use crate::settings::{DEFAULT_PAGE_NAMES, ExplorerPage, Tile, TileOverlay, TileSize};

use super::*;

// The core's tile kinds, in explorer/.
mod details;
mod dispersion;
mod flight3d;
mod scatter;
mod table;

/// A kind of tile of the shots explorer and the tile overlays: its name, its switches and what it
/// draws. The core's kinds are the modules of `explorer`; another crate adds its own with
/// [`register_tile_kind`].
///
/// A kind keeps its settings in the tile ([`Tile::config`], read as its own type with defaults for
/// what is missing, changed with [`Tile::change_config`]) and what it keeps for the session in
/// [`TileCtx::state`] (per tile) or [`TileCtx::shared`] (all tiles).
pub trait TileKind: Sync {
    /// Id in the settings (e.g. `"table"`); never changed once released.
    fn id(&self) -> &'static str;

    /// Name in the menus and the tile's title, translated (`tr`).
    fn name(&self) -> &'static str;

    /// Its switches on the Overlay page, below the choice of tile.
    fn options(&self, _ui: &mut egui::Ui, _tile: &mut Tile, _units: Units) {}

    /// Controls at the right of the tile's header, laid out right to left (not while configuring).
    fn header(&self, _ui: &mut egui::Ui, _tile: &mut Tile, _ctx: &TileCtx) {}

    /// Width of the tile from which its title is shown next to the header's controls; narrower
    /// tiles leave it out (the controls say enough).
    fn title_from(&self) -> f32 {
        0.0
    }

    /// Draws what the tile shows into `ui` (the tile below its header). Changes to the tile's
    /// settings (`tile.config`) are saved, here and in `header`; on display only (`ctx.display()`,
    /// the tile overlays) they are dropped.
    fn body(&self, ui: &mut egui::Ui, tile: &mut Tile, ctx: &TileCtx);
}

/// The core's kinds, in the order of the menus.
const CORE_KINDS: [&dyn TileKind; 5] = [&table::Table, &details::Details, &dispersion::Dispersion, &scatter::Scatter, &flight3d::Flight3d];

/// Kinds added by other crates, after the core's.
static REGISTERED: RwLock<Vec<&'static dyn TileKind>> = RwLock::new(Vec::new());

/// Adds a kind of tile: it shows up in the menus after the core's kinds, and tiles of its id are
/// drawn by it. Call it before `ui::run`. A kind with an id that is already there (the core's or
/// one added before) is left out.
pub fn register_tile_kind(kind: &'static dyn TileKind) {
    let mut kinds = REGISTERED.write().unwrap_or_else(PoisonError::into_inner);
    if !CORE_KINDS.iter().chain(kinds.iter()).any(|k| k.id() == kind.id()) {
        kinds.push(kind);
    }
}

/// Every kind: the core's, then the registered ones.
fn kinds() -> Vec<&'static dyn TileKind> {
    let registered = REGISTERED.read().unwrap_or_else(PoisonError::into_inner);
    CORE_KINDS.iter().copied().chain(registered.iter().copied()).collect()
}

/// The kind with the id `id`, if this build has it.
fn kind_of(id: &str) -> Option<&'static dyn TileKind> {
    kinds().into_iter().find(|k| k.id() == id)
}

/// Name of a kind; its id if this build doesn't have it.
fn kind_name(id: &str) -> &str {
    kind_of(id).map_or(id, |k| k.name())
}


/// Gap between tiles.
const GAP: f32 = 8.0;
/// Cell size the grid aims for; the rows are fitted to the height.
const CELL_TARGET: f32 = 170.0;
/// Club colours in bag order of the clubs played: fourteen, one per club of a full bag.
const CLUB_COLORS: [Color32; 14] = [
    AMBER,
    ICON,
    Color32::from_rgb(0x4f, 0xd1, 0xc5),
    Color32::from_rgb(0xe0, 0x7b, 0xd8),
    GREEN,
    Color32::from_rgb(0xff, 0x8a, 0x65),
    Color32::from_rgb(0xb3, 0x9d, 0xff),
    Color32::from_rgb(0xf4, 0xe0, 0x6d),
    Color32::from_rgb(0x5b, 0xc0, 0xeb),
    RED,
    Color32::from_rgb(0xb5, 0xe3, 0x5a),
    Color32::from_rgb(0xff, 0x9e, 0xc4),
    Color32::from_rgb(0xd9, 0xa0, 0x6a),
    LABEL,
];

/// A shot across the session and the history file.
type Key = (String, u32);

fn key(s: &ShotRecord) -> Key {
    (s.time.clone(), s.shot_number)
}

/// Filter, selection and arranging; kept for the session, not saved.
#[derive(Default)]
pub(super) struct ExplorerState {
    page: usize,
    arrange: bool,
    hidden_clubs: BTreeSet<Option<String>>,
    hide_excluded: bool,
    only_selected: bool,
    selected: BTreeSet<Key>,
    /// Shot shown in the details tiles (the last one clicked).
    focus: Option<Key>,
    /// The export dialog is open.
    export_dialog: bool,
    /// Page whose removal waits for the answer to "Remove the page?".
    confirm_remove: Option<usize>,
    /// Estimated flight of every shot seen, computed once.
    flights: HashMap<Key, FlightSummary>,
    /// What the tiles' kinds keep for the session (e.g. the 3D tiles' cameras), in the explorer and
    /// the tile overlay.
    states: TileStates,
    /// A shot whose flight is being animated, since when.
    anim: Option<(Key, Instant)>,
    /// The session's latest shot when the page was last drawn (`None` before): a new one starts
    /// its animation.
    last_latest: Option<Option<Key>>,
    /// Where the tiles of the page shown were drawn (last frame).
    tile_rects: Vec<Rect>,
    /// The tile overlay: the shot flying in it, the latest shot it has seen.
    overlay_anim: Option<(Key, Instant)>,
    overlay_latest: Option<Option<Key>>,
}

/// The flight values the tiles use, in metres: the estimate (`flight::fly` without the path), or
/// GSPro's numbers where it sent them and they are chosen.
#[derive(Clone, Copy)]
pub struct FlightSummary {
    pub carry: f64,
    pub total: f64,
    /// Sideways where the ball lands and where it stops (right is positive).
    pub carry_side: f64,
    pub total_side: f64,
    pub apex: f64,
}

impl FlightSummary {
    /// GSPro's numbers in place of the estimate. GSPro reports the side where the ball stopped;
    /// the side at the landing is the estimate's, scaled to it.
    fn with_gspro(self, g: Option<&crate::gspro_db::GsproResult>) -> Self {
        let Some(g) = g else { return self };
        let carry_side = if self.total_side.abs() > 0.5 { self.carry_side * g.side / self.total_side } else { g.side };
        Self { carry: g.carry, total: g.total, carry_side, total_side: g.side, apex: g.apex }
    }
}

impl From<&Flight> for FlightSummary {
    fn from(f: &Flight) -> Self {
        Self { carry: f.carry, total: f.total, carry_side: f.carry_side, total_side: f.total_side, apex: f.apex }
    }
}

/// Grid of near-square cells filling `size`: columns, rows and the cell size.
fn grid(size: egui::Vec2) -> (usize, usize, egui::Vec2) {
    let rows = (((size.y + GAP) / (CELL_TARGET + GAP)).round() as usize).max(2);
    let h = ((size.y - (rows - 1) as f32 * GAP) / rows as f32).max(40.0);
    let cols = (((size.x + GAP) / (h + GAP)).floor() as usize).max(2);
    let w = (size.x - (cols - 1) as f32 * GAP) / cols as f32;
    (cols, rows, vec2(w, h))
}

/// Place the tiles row by row in the first free spot (gaps are filled by later, smaller tiles).
/// Returns column, row, width and height in cells; a page-sized tile covers `cols` × `rows`.
fn place(sizes: &[TileSize], cols: usize, rows: usize) -> Vec<(usize, usize, usize, usize)> {
    let mut used: Vec<Vec<bool>> = Vec::new();
    sizes
        .iter()
        .map(|size| {
            let (w, h) = size.cells().map_or((cols, rows), |(w, h)| (w.min(cols), h.min(rows)));
            let mut r = 0;
            loop {
                while used.len() < r + h {
                    used.push(vec![false; cols]);
                }
                let free = |c: usize| (r..r + h).all(|y| (c..c + w).all(|x| !used[y][x]));
                if let Some(c) = (0..=cols - w).find(|&c| free(c)) {
                    for row in used.iter_mut().skip(r).take(h) {
                        row[c..c + w].fill(true);
                    }
                    return (c, r, w, h);
                }
                r += 1;
            }
        })
        .collect()
}

fn size_name(size: TileSize) -> &'static str {
    match size {
        TileSize::W1H1 => "1×1",
        TileSize::W2H1 => "2×1",
        TileSize::W1H2 => "1×2",
        TileSize::W2H2 => "2×2",
        TileSize::W3H2 => "3×2",
        TileSize::W4H2 => "4×2",
        TileSize::Page => tr("Page"),
    }
}

fn page_name(page: &ExplorerPage, i: usize) -> String {
    if page.name.trim().is_empty() {
        return tr("Page {n}").replace("{n}", &(i + 1).to_string());
    }
    // The default pages' names (in either language) in the language shown.
    let name = page.name.trim();
    DEFAULT_PAGE_NAMES.iter().find(|n| i18n::is_text(n, name)).map_or_else(|| page.name.clone(), |n| tr(n).to_string())
}

/// Changes to a page from its tab's menu, applied afterwards.
enum PageEdit {
    Rename(usize, String),
    AddPage,
    Move(usize, isize),
    Remove(usize),
}

/// Changes to the layout made while drawing, applied afterwards.
enum Edit {
    Kind(usize, String),
    Size(usize, TileSize),
    /// The tile's settings, changed by its kind.
    Config(usize, serde_json::Map<String, serde_json::Value>),
    Move(usize, isize),
    /// Dragged onto another tile's place.
    MoveTo(usize, usize),
    /// Play a shot's flight again (3D tiles).
    Replay(Key),
    Add(&'static str),
    Remove(usize),
}

/// Units and conversions for the values in the tiles.
#[derive(Clone, Copy)]
pub struct Units {
    metric: bool,
    meters: bool,
}

impl Units {
    /// A speed (mph) in the unit shown.
    #[must_use]
    pub fn speed(self, mph: f64) -> f64 {
        if self.metric { mph * MPH_TO_KMH } else { mph }
    }
    #[must_use]
    pub fn speed_unit(self) -> &'static str {
        if self.metric { "km/h" } else { "mph" }
    }
    /// A distance (metres) in the unit shown.
    #[must_use]
    pub fn dist(self, m: f64) -> f64 {
        if self.meters { m } else { m * M_TO_YD }
    }
    #[must_use]
    pub fn dist_unit(self) -> &'static str {
        if self.meters { "m" } else { "yd" }
    }
}

impl App {
    pub(super) fn shots(&mut self, ui: &mut egui::Ui) {
        let session: Vec<ShotRecord> = state::with(|s| s.session.clone());
        // A new shot flies in the 3D tiles (not the ones there when the page first shows).
        let latest = session.last().map(key);
        if let Some(seen) = &self.explorer.last_latest
            && latest.is_some()
            && *seen != latest
        {
            self.explorer.anim = latest.clone().map(|k| (k, Instant::now()));
        }
        self.explorer.last_latest = Some(latest);
        if self.explorer.anim.as_ref().is_some_and(|(_, t)| t.elapsed() > Duration::from_secs(15)) {
            self.explorer.anim = None;
        }
        let units = Units { metric: self.saved.metric, meters: self.saved.meters };

        // Source, pages, arranging and export.
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
        let mut bar_ui = ui.new_child(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let before = self.all_time;
        bar_ui.radio_value(&mut self.all_time, false, tr("This session ({n})").replace("{n}", &session.len().to_string()));
        bar_ui.radio_value(&mut self.all_time, true, tr("All time"));
        if self.all_time && (!before || self.all_shots_session != session.len()) {
            // The demo doesn't save its shots, so its "all time" is the demo session.
            self.all_shots = if history::PERSIST.load(std::sync::atomic::Ordering::Relaxed) {
                history::load_all()
            } else {
                session.clone()
            };
            self.all_shots_session = session.len();
        }
        let shots: Vec<ShotRecord> = if self.all_time { self.all_shots.clone() } else { session };
        self.explorer.page = self.explorer.page.min(self.saved.explorer.pages.len().saturating_sub(1));
        bar_ui.add_space(18.0);
        let mut layout_changed = false;
        // A page's tab: click shows it, right-click opens its settings.
        let mut page_edit: Option<PageEdit> = None;
        let removable = self.saved.explorer.pages.len() > 1;
        let page_count = self.saved.explorer.pages.len();
        for (i, page) in self.saved.explorer.pages.iter().enumerate() {
            // While configuring, the page shown is edited right in its tab: move, rename, remove.
            if self.explorer.arrange && i == self.explorer.page {
                let ui = &mut bar_ui;
                if i > 0 && link(ui, "◀", LABEL).on_hover_text(tr("Move the page left")).clicked() {
                    page_edit = Some(PageEdit::Move(i, -1));
                }
                let mut name = if page.name.trim().is_empty() { String::new() } else { page_name(page, i) };
                let hint = tr("Page {n}").replace("{n}", &(i + 1).to_string());
                let w = (text_width(ui, if name.is_empty() { &hint } else { &name }, 13.0) + 24.0).clamp(70.0, 180.0);
                if ui.add(egui::TextEdit::singleline(&mut name).hint_text(hint).desired_width(w)).on_hover_text(tr("Name of the page")).changed() {
                    page_edit = Some(PageEdit::Rename(i, name));
                }
                if i + 1 < page_count && link(ui, "▶", LABEL).on_hover_text(tr("Move the page right")).clicked() {
                    page_edit = Some(PageEdit::Move(i, 1));
                }
                if removable {
                    let remove = ui.add(egui::Button::new(RichText::new("×").size(19.0).color(RED)).frame(false));
                    if remove.on_hover_text(tr("Remove page")).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        page_edit = Some(PageEdit::Remove(i));
                    }
                }
                ui.add_space(6.0);
                continue;
            }
            let mut tab = page_tab(&mut bar_ui, &page_name(page, i), i == self.explorer.page);
            if self.explorer.arrange {
                tab = tab.on_hover_text(tr("Right-click for the page's settings"));
            }
            if tab.clicked() || tab.secondary_clicked() {
                self.explorer.page = i;
            }
            // Stays open while typing the name; the entries close it themselves.
            egui::Popup::context_menu(&tab).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                let mut name = if page.name.trim().is_empty() { String::new() } else { page_name(page, i) };
                let hint = tr("Page {n}").replace("{n}", &(i + 1).to_string());
                if ui.add(egui::TextEdit::singleline(&mut name).hint_text(hint).desired_width(160.0)).changed() {
                    page_edit = Some(PageEdit::Rename(i, name));
                }
                ui.separator();
                if ui.button(tr("Add a page")).clicked() {
                    page_edit = Some(PageEdit::AddPage);
                    ui.close();
                }
                if removable && ui.button(RichText::new(tr("Remove page")).color(RED)).clicked() {
                    page_edit = Some(PageEdit::Remove(i));
                    ui.close();
                }
            });
        }
        if let Some(edit) = page_edit {
            let pages = &mut self.saved.explorer.pages;
            match edit {
                PageEdit::Rename(i, name) => pages[i].name = name,
                PageEdit::AddPage => {
                    pages.push(ExplorerPage { name: String::new(), tiles: vec![Tile::default()] });
                    self.explorer.page = pages.len() - 1;
                }
                PageEdit::Move(i, by) => {
                    let j = i.saturating_add_signed(by);
                    if j < pages.len() {
                        pages.swap(i, j);
                        self.explorer.page = j;
                    }
                }
                // Asks first (below): the page's tiles go with it.
                PageEdit::Remove(i) => self.explorer.confirm_remove = Some(i),
            }
            layout_changed = true;
        }
        if let Some(i) = self.explorer.confirm_remove {
            let name = self.saved.explorer.pages.get(i).map(|p| page_name(p, i)).unwrap_or_default();
            let title = tr("Remove the page “{name}”?").replace("{name}", &name);
            let text = tr("Its tiles go with it. Your shots are kept.");
            if let Some(remove) = confirm(bar_ui.ctx(), "remove-page", &title, text, tr("REMOVE")) {
                self.explorer.confirm_remove = None;
                let pages = &mut self.saved.explorer.pages;
                if remove && i < pages.len() && pages.len() > 1 {
                    pages.remove(i);
                    self.explorer.page = self.explorer.page.min(pages.len() - 1);
                    layout_changed = true;
                }
            }
        }
        if self.explorer.arrange && link(&mut bar_ui, "+", ICON).on_hover_text(tr("Add a page")).clicked() {
            self.saved.explorer.pages.push(ExplorerPage { name: String::new(), tiles: vec![Tile::default()] });
            self.explorer.page = self.saved.explorer.pages.len() - 1;
            layout_changed = true;
        }
        bar_ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(150.0, 30.0), Sense::hover());
            // Asks for the layout in a dialog (below).
            if button(ui, rect, tr("EXPORT CSV"), Icon::None, false, !shots.is_empty(), tr("No shots to export")) {
                self.explorer.export_dialog = true;
            }
            let label = if self.explorer.arrange { tr("DONE") } else { tr("CONFIGURE") };
            let w = ui.painter().layout_no_wrap(tr("CONFIGURE").to_string(), cond(19.0), WHITE).size().x + 36.0;
            let (rect, _) = ui.allocate_exact_size(vec2(w.max(110.0), 30.0), Sense::hover());
            if button(ui, rect, label, Icon::None, self.explorer.arrange, true, "") {
                self.explorer.arrange = !self.explorer.arrange;
            }
            if let Some(note) = &self.export_note {
                ui.label(RichText::new(note).size(12.0).color(MUTED));
            }
        });
        ui.add_space(6.0);

        let codes = club_codes(&shots);
        let color_of = |code: &Option<String>| {
            CLUB_COLORS[codes.iter().position(|c| c == code).unwrap_or(0) % CLUB_COLORS.len()]
        };
        let ex = &mut self.explorer;
        ex.selected.retain(|k| shots.iter().any(|s| &key(s) == k));
        if ex.selected.is_empty() {
            ex.only_selected = false;
        }
        let shown: Vec<&ShotRecord> = shots
            .iter()
            .filter(|s| !ex.hidden_clubs.contains(&s.club_code))
            .filter(|s| !(ex.hide_excluded && s.excluded))
            .filter(|s| !ex.only_selected || ex.selected.contains(&key(s)))
            .collect();
        let export: Vec<ShotRecord> = if ex.export_dialog { shown.iter().map(|s| (*s).clone()).collect() } else { Vec::new() };
        let use_gspro = self.saved.gspro_results;
        let flights = ex.flights_of(&shown, use_gspro);

        // Second row: the filter and what to do with the selection, or the page while arranging.
        let mut exclude: Option<bool> = None;
        let gspro_toggled;
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
        let mut row = ui.new_child(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::left_to_right(egui::Align::Center)));
        row.spacing_mut().item_spacing.x = 6.0;
        {
            row.label(RichText::new(tr("Filter")).size(13.0).color(LABEL));
            row.add_space(2.0);
            // One chip per club while they fit next to the other controls, else a menu.
            let selection_w = if ex.selected.is_empty() {
                0.0
            } else {
                let n = tr("{n} selected").replace("{n}", &ex.selected.len().to_string());
                [n.as_str(), tr("Show only these"), tr("Exclude"), tr("Clear")]
                    .iter()
                    .map(|t| text_width(&row, t, 13.0) + 20.0)
                    .sum::<f32>()
            };
            let chips_w: f32 = codes.iter().map(|c| chip_width(&row, c.as_deref().unwrap_or("–"), true) + 6.0).sum();
            let room = row.available_width() - selection_w - chip_width(&row, tr("Hide excluded"), false) - chip_width(&row, tr("GSPro results"), false) - 18.0;
            if chips_w <= room {
                for code in &codes {
                    let on = !ex.hidden_clubs.contains(code);
                    let name = code.as_deref().unwrap_or("–");
                    if chip(&mut row, name, Some(color_of(code)), on).clicked() {
                        if on {
                            ex.hidden_clubs.insert(code.clone());
                        } else {
                            ex.hidden_clubs.remove(code);
                        }
                    }
                }
            } else {
                club_menu(&mut row, &codes, &mut ex.hidden_clubs, &color_of);
            }
            if chip(&mut row, tr("Hide excluded"), None, ex.hide_excluded).clicked() {
                ex.hide_excluded = !ex.hide_excluded;
            }
            let gspro_chip = chip(&mut row, tr("GSPro results"), None, self.saved.gspro_results)
                .on_hover_text(tr("Carry, total, apex and side as GSPro computed them, where it did; else the estimate (*)"));
            gspro_toggled = gspro_chip.clicked();
            if !ex.selected.is_empty() {
                row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    if link(ui, tr("Clear"), MUTED).clicked() {
                        ex.selected.clear();
                        ex.only_selected = false;
                    }
                    let all_excluded = shots.iter().filter(|s| ex.selected.contains(&key(s))).all(|s| s.excluded);
                    let (label, hint) = if all_excluded {
                        (tr("Count"), tr("Count these shots in the stats and exports again"))
                    } else {
                        (tr("Exclude"), tr("Leave these shots out of the stats and exports (e.g. misreads); they stay listed"))
                    };
                    if link(ui, label, AMBER).on_hover_text(hint).clicked() {
                        exclude = Some(!all_excluded);
                    }
                    let only = if ex.only_selected { tr("Show all") } else { tr("Show only these") };
                    if link(ui, only, ICON).clicked() {
                        ex.only_selected = !ex.only_selected;
                    }
                    ui.label(RichText::new(tr("{n} selected").replace("{n}", &ex.selected.len().to_string())).size(13.0).color(WHITE));
                });
            }
        }
        ui.add_space(8.0);

        // The tiles of the page shown.
        let area = ui.available_rect_before_wrap();
        let (cols, rows, cell) = grid(area.size());
        let page_tiles = self.saved.explorer.pages[ex.page].tiles.clone();
        // While configuring, an empty tile with a + comes last: it adds a tile.
        let mut sizes: Vec<TileSize> = page_tiles.iter().map(|t| t.size).collect();
        if ex.arrange {
            sizes.push(TileSize::W1H1);
        }
        let spots = place(&sizes, cols, rows);
        let used_rows = spots.iter().map(|&(_, r, _, h)| r + h).max().unwrap_or(0);
        let mut edits: Vec<Edit> = Vec::new();
        let mut clicked: Option<Key> = None;
        let anim = ex.anim.as_ref().map(|(k, t)| (k, t.elapsed().as_secs_f32()));
        let focus = ex
            .focus
            .as_ref()
            .and_then(|k| shown.iter().find(|s| &key(s) == k))
            .or_else(|| shown.last())
            .copied();
        egui::ScrollArea::vertical().id_salt(("explorer", ex.page)).auto_shrink([false, false]).show(ui, |ui| {
            let height = used_rows as f32 * (cell.y + GAP) - GAP;
            let (grid_rect, _) = ui.allocate_exact_size(vec2(area.width(), height.max(0.0)), Sense::hover());
            if page_tiles.is_empty() && !ex.arrange {
                ui.painter().text(
                    area.center(),
                    Align2::CENTER_CENTER,
                    tr("No tiles on this page – CONFIGURE adds some."),
                    FontId::proportional(13.0),
                    MUTED,
                );
            }
            let mut rects: Vec<Rect> = Vec::new();
            for (i, (tile, &(c, r, w, h))) in page_tiles.iter().zip(&spots).enumerate() {
                let min = grid_rect.min + vec2(c as f32 * (cell.x + GAP), r as f32 * (cell.y + GAP));
                let size = vec2(w as f32 * cell.x + (w - 1) as f32 * GAP, h as f32 * cell.y + (h - 1) as f32 * GAP);
                let rect = Rect::from_min_size(min, size);
                let ctx = TileCtx {
                    shown: &shown,
                    flights: &flights,
                    gspro: use_gspro,
                    page: ex.page,
                    index: i,
                    states: &ex.states,
                    anim,
                    selected: &ex.selected,
                    color_of: &color_of,
                    units,
                    all_time: self.all_time,
                    display: false,
                    focus,
                    clicked: Cell::new(None),
                    replay: Cell::new(None),
                };
                tile_ui(ui, rect, i, tile, ex.arrange, page_tiles.len(), &ctx, &mut edits);
                if let Some(k) = ctx.clicked.take() {
                    clicked = Some(k);
                }
                if let Some(k) = ctx.replay.take() {
                    edits.push(Edit::Replay(k));
                }
                rects.push(rect);
            }
            ex.tile_rects.clone_from(&rects);
            if ex.arrange {
                drag_tiles(ui, &rects, &page_tiles, &mut edits);
                if let Some(&(c, r, _, _)) = spots.last() {
                    let rect = Rect::from_min_size(grid_rect.min + vec2(c as f32 * (cell.x + GAP), r as f32 * (cell.y + GAP)), cell);
                    if let Some(kind) = add_tile(ui, rect) {
                        edits.push(Edit::Add(kind));
                    }
                }
            }
        });

        // Apply what was clicked.
        if let Some(k) = clicked {
            if ex.selected.remove(&k) {
                if ex.focus.as_ref() == Some(&k) {
                    ex.focus = ex.selected.iter().next_back().cloned();
                }
            } else {
                ex.selected.insert(k.clone());
                ex.focus = Some(k);
            }
        }
        let mut replay: Option<Key> = None;
        let changes_layout = edits.iter().any(|e| !matches!(e, Edit::Replay(_)));
        if !edits.is_empty() {
            let tiles = &mut self.saved.explorer.pages[ex.page].tiles;
            for edit in edits {
                match edit {
                    Edit::Kind(i, kind) => tiles[i].kind = kind,
                    Edit::Size(i, size) => tiles[i].size = size,
                    Edit::Config(i, config) => tiles[i].config = config,
                    Edit::Add(kind) => tiles.push(Tile { kind: kind.to_string(), ..Tile::default() }),
                    Edit::Replay(k) => replay = Some(k),
                    Edit::MoveTo(from, to) => {
                        let tile = tiles.remove(from);
                        tiles.insert(to.min(tiles.len()), tile);
                    }
                    Edit::Move(i, by) => {
                        let j = i.saturating_add_signed(by);
                        if j < tiles.len() {
                            tiles.swap(i, j);
                        }
                    }
                    Edit::Remove(i) => {
                        tiles.remove(i);
                    }
                }
            }
            layout_changed |= changes_layout;
        }
        if let Some(k) = replay {
            ex.anim = Some((k, Instant::now()));
        }
        if gspro_toggled {
            let on = !self.saved.gspro_results;
            let _ = self.change_settings(|s| s.gspro_results = on);
        }
        if layout_changed {
            // Applied above already; saved at once, like the overlay's settings.
            let explorer = self.saved.explorer.clone();
            let _ = self.change_settings(|s| s.explorer.clone_from(&explorer));
        }
        if let Some(excluded) = exclude {
            let keys = self.explorer.selected.clone();
            let mark = |shots: &mut Vec<ShotRecord>| {
                for s in shots.iter_mut().filter(|s| keys.contains(&key(s))) {
                    s.excluded = excluded;
                }
            };
            state::with(|s| mark(&mut s.session));
            mark(&mut self.all_shots);
            for (time, number) in &keys {
                if let Err(e) = history::set_excluded(time, *number, excluded) {
                    self.export_note = Some(tr("Saving failed: {e}").replace("{e}", &e));
                }
            }
        }
        if self.explorer.export_dialog {
            let mut picked = None;
            let modal = dialog(ui.ctx(), "export-csv", 380.0, |ui| {
                let title = tr("Export {n} shots as CSV").replace("{n}", &export.len().to_string());
                ui.label(RichText::new(title).size(16.0).color(WHITE));
                ui.add_space(4.0);
                ui.label(RichText::new(tr("The shots shown, with the filter. Choose the layout:")).size(13.0).color(SOFT));
                ui.add_space(10.0);
                for f in ExportFormat::ALL {
                    if format_choice(ui, f) {
                        picked = Some(f);
                        ui.close();
                    }
                    ui.add_space(6.0);
                }
                ui.add_space(8.0);
                let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover());
                let cancel_r = Rect::from_min_max(pos2(row.right() - 130.0, row.top()), row.max);
                if button(ui, cancel_r, tr("CANCEL"), Icon::None, false, true, "") {
                    ui.close();
                }
            });
            if modal.should_close() {
                self.explorer.export_dialog = false;
            }
            if let Some(f) = picked {
                self.export_note = Some(export_csv(&export, f));
            }
        }
    }
}

impl ExplorerState {
    /// The estimate of every shot (cached), with GSPro's numbers where chosen and sent.
    fn flights_of(&mut self, shown: &[&ShotRecord], use_gspro: bool) -> Vec<FlightSummary> {
        shown
            .iter()
            .map(|s| {
                let estimate = *self.flights.entry(key(s)).or_insert_with(|| FlightSummary::from(&flight::fly(&s.ball)));
                estimate.with_gspro(s.gspro.filter(|_| use_gspro).as_ref())
            })
            .collect()
    }

    /// The tile overlay's tile in `rect` (its window, or the preview `slot` 1): the session's shots,
    /// or those with the club chosen in GSPro. Nothing to click; a new shot flies in the 3D tile.
    #[expect(clippy::too_many_arguments, reason = "the overlay window and the preview pass their own slot, config, shots, club and units; a struct would only rename them")]
    pub(super) fn overlay_tile(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        slot: usize,
        cfg: &TileOverlay,
        shots: &[ShotRecord],
        club: Option<&str>,
        units: (bool, bool),
        use_gspro: bool,
    ) {
        let latest = shots.last().map(key);
        if let Some(seen) = &self.overlay_latest
            && latest.is_some()
            && *seen != latest
        {
            self.overlay_anim = latest.clone().map(|k| (k, Instant::now()));
        }
        self.overlay_latest = Some(latest);
        if self.overlay_anim.as_ref().is_some_and(|(_, t)| t.elapsed() > Duration::from_secs(15)) {
            self.overlay_anim = None;
        }
        let codes = club_codes(shots);
        let color_of = |code: &Option<String>| CLUB_COLORS[codes.iter().position(|c| c == code).unwrap_or(0) % CLUB_COLORS.len()];
        let club = club.filter(|_| cfg.club_only);
        let shown: Vec<&ShotRecord> = shots.iter().filter(|s| club.is_none() || s.club_code.as_deref() == club).collect();
        let flights = self.flights_of(&shown, use_gspro);
        let selected = BTreeSet::new();
        let ctx = TileCtx {
            shown: &shown,
            flights: &flights,
            gspro: use_gspro,
            page: usize::MAX,
            index: 1000 + slot,
            states: &self.states,
            anim: self.overlay_anim.as_ref().map(|(k, t)| (k, t.elapsed().as_secs_f32())),
            selected: &selected,
            color_of: &color_of,
            units: Units { metric: units.0, meters: units.1 },
            all_time: false,
            display: true,
            // The last shot is the one the details show.
            focus: shown.last().copied(),
            clicked: Cell::new(None),
            replay: Cell::new(None),
        };
        let p = ui.painter();
        p.rect(rect, 8.0, TILE.gamma_multiply(cfg.opacity), Stroke::new(1.0, LINE), StrokeKind::Inside);
        let head = Rect::from_min_size(rect.min, vec2(rect.width(), 28.0)).shrink2(vec2(10.0, 0.0));
        let mut title = kind_name(&cfg.tile.kind).to_uppercase();
        if let Some(code) = club {
            title = format!("{title} · {}", club_name(code).to_uppercase());
        }
        p.text(head.left_center(), Align2::LEFT_CENTER, title, cond(14.0), LABEL);
        p.line_segment([pos2(rect.left(), head.bottom()), pos2(rect.right(), head.bottom())], Stroke::new(1.0, LINE));
        let body = Rect::from_min_max(pos2(rect.left() + 8.0, head.bottom() + 4.0), rect.max - vec2(8.0, 6.0));
        // Nothing to change on display: the tile's settings are those of the Overlay page.
        tile_body(ui, body, &mut cfg.tile.clone(), &ctx);
    }
}

/// What the tile overlay shows, on the Overlay page: the kind of tile and its switches.
/// `extra` goes next to the choice of tile.
pub(super) fn tile_options(ui: &mut egui::Ui, tile: &mut Tile, metric: bool, meters: bool, extra: impl FnOnce(&mut egui::Ui)) {
    setting_row(ui, tr("Tile"), |ui| {
        egui::ComboBox::from_id_salt("tile-overlay-kind").width(150.0).selected_text(kind_name(&tile.kind)).show_ui(ui, |ui| {
            for k in kinds() {
                ui.selectable_value(&mut tile.kind, k.id().to_string(), k.name());
            }
        });
        extra(ui);
    });
    if let Some(kind) = kind_of(&tile.kind) {
        kind.options(ui, tile, Units { metric, meters });
    }
}

/// Clubs in bag order (driver to putter), so their colours stay put while filtering.
fn club_codes(shots: &[ShotRecord]) -> Vec<Option<String>> {
    let mut codes: Vec<Option<String>> = Vec::new();
    for s in shots {
        if !codes.contains(&s.club_code) {
            codes.push(s.club_code.clone());
        }
    }
    codes.sort_by_key(|c| club_rank(c.as_deref()));
    codes
}

/// While arranging, tiles are dragged by their body onto another tile's place. The tile under
/// the pointer is outlined, a label follows the pointer.
fn drag_tiles(ui: &mut egui::Ui, rects: &[Rect], tiles: &[Tile], edits: &mut Vec<Edit>) {
    let pointer = ui.ctx().pointer_interact_pos();
    for (i, rect) in rects.iter().enumerate() {
        // Below the header, which keeps its controls; on top of the tile's content.
        let handle = Rect::from_min_max(rect.min + vec2(0.0, 28.0), rect.max);
        let resp = ui.interact(handle, ui.id().with(("drag-tile", i)), Sense::drag());
        let target = pointer.and_then(|at| rects.iter().position(|r| r.contains(at)));
        if resp.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            let layer = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, ui.id().with("drag-tiles")));
            layer.rect_filled(*rect, 8.0, Color32::from_black_alpha(110));
            if let Some(j) = target.filter(|&j| j != i) {
                layer.rect_stroke(rects[j], 8.0, Stroke::new(2.0, ACCENT), StrokeKind::Inside);
            }
            if let Some(at) = pointer {
                let text = kind_name(&tiles[i].kind);
                let galley = layer.layout_no_wrap(text.to_string(), FontId::proportional(13.0), WHITE);
                let label = Rect::from_min_size(at + vec2(12.0, 8.0), galley.size() + vec2(16.0, 8.0));
                layer.rect(label, 5.0, BUTTON, Stroke::new(1.0, ACCENT), StrokeKind::Inside);
                layer.galley(label.min + vec2(8.0, 4.0), galley, WHITE);
            }
        } else if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        if resp.drag_stopped()
            && let Some(j) = target.filter(|&j| j != i)
        {
            edits.push(Edit::MoveTo(i, j));
        }
    }
}

/// The empty tile with a + shown while configuring; returns the kind of tile picked from its menu.
fn add_tile(ui: &mut egui::Ui, rect: Rect) -> Option<&'static str> {
    let resp = ui.interact(rect, ui.id().with("add-tile"), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tr("Add tile")));
    let p = ui.painter();
    let color = if resp.hovered() { ICON } else { BUTTON_LINE };
    let outline = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom(), rect.left_top()];
    p.extend(egui::Shape::dashed_line(&outline.map(|q| q + (rect.center() - q).normalized() * 1.0), Stroke::new(1.5, color), 6.0, 5.0));
    let c = rect.center() - vec2(0.0, 10.0);
    p.line_segment([c - vec2(14.0, 0.0), c + vec2(14.0, 0.0)], Stroke::new(3.0, color));
    p.line_segment([c - vec2(0.0, 14.0), c + vec2(0.0, 14.0)], Stroke::new(3.0, color));
    p.text(c + vec2(0.0, 30.0), Align2::CENTER_CENTER, tr("Add tile"), FontId::proportional(12.5), if resp.hovered() { SOFT } else { MUTED });
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let mut picked = None;
    egui::Popup::menu(&resp).show(|ui| {
        for kind in kinds() {
            if ui.button(kind.name()).clicked() {
                picked = Some(kind.id());
            }
        }
    });
    picked
}

/// One layout in the export dialog: its name and what it is for.
fn format_choice(ui: &mut egui::Ui, format: ExportFormat) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 50.0), Sense::click());
    let p = ui.painter();
    let fill = if resp.hovered() { BUTTON_HOVER } else { BUTTON };
    p.rect(rect, 6.0, fill, Stroke::new(1.0, if resp.hovered() { ACCENT } else { BUTTON_LINE }), StrokeKind::Inside);
    let what = match format {
        ExportFormat::OpenLaunch => tr("All values of this app, one row per shot"),
        ExportFormat::GarminGolf => tr("Columns and units like the Garmin Golf app's export"),
        ExportFormat::AwesomeGolf => tr("Columns like Awesome Golf's export"),
    };
    p.text(rect.left_top() + vec2(14.0, 16.0), Align2::LEFT_CENTER, format.label(), FontId::proportional(14.0), WHITE);
    p.text(rect.left_top() + vec2(14.0, 35.0), Align2::LEFT_CENTER, what, FontId::proportional(12.0), LABEL);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// What an empty tile says: the history may simply have no saved shots yet.
fn no_shots(ctx: &TileCtx) -> &'static str {
    if ctx.all_time { tr("No shots saved yet – shots hit with the R10 are saved, test shots aren't.") } else { tr("No shots yet.") }
}

/// What a kind keeps for the session: per tile (by page and place) and per type for all tiles.
#[derive(Default)]
struct TileStates {
    tiles: RefCell<HashMap<(usize, usize), Box<dyn Any>>>,
    shared: RefCell<HashMap<TypeId, Box<dyn Any>>>,
}

#[cfg(test)]
impl TileStates {
    /// The state of the tile at `place` (page and index), if it is a `T`.
    fn get<T: 'static>(&self, place: (usize, usize)) -> Option<std::cell::Ref<'_, T>> {
        std::cell::Ref::filter_map(self.tiles.borrow(), |m| m.get(&place).and_then(|b| b.downcast_ref::<T>())).ok()
    }

    /// Sets the state of the tile at `place`.
    fn set<T: 'static>(&self, place: (usize, usize), state: T) {
        self.tiles.borrow_mut().insert(place, Box::new(state));
    }
}

/// What every tile draws from: the shots shown (after the filter), their flights, the selection
/// and the units. A kind answers clicks through it, too.
pub struct TileCtx<'a> {
    shown: &'a [&'a ShotRecord],
    /// Estimated flight of each shot shown, in the same order.
    flights: &'a [FlightSummary],
    /// GSPro's distances where it sent them (else the estimate).
    gspro: bool,
    /// Where the tile is: its page (`usize::MAX` for the tile overlays) and its place on it.
    page: usize,
    index: usize,
    states: &'a TileStates,
    /// The shot being animated and for how many seconds.
    anim: Option<(&'a Key, f32)>,
    selected: &'a BTreeSet<Key>,
    color_of: &'a dyn Fn(&Option<String>) -> Color32,
    units: Units,
    all_time: bool,
    /// Shown, not used (the tile overlay): no controls in the tiles.
    display: bool,
    /// The shot the details are about: the one clicked last, or the latest.
    focus: Option<&'a ShotRecord>,
    /// A shot clicked in the tile (selects it or takes it out of the selection).
    clicked: Cell<Option<Key>>,
    /// A shot whose flight is to be played again.
    replay: Cell<Option<Key>>,
}

impl<'a> TileCtx<'a> {
    /// The shots shown, oldest first.
    #[must_use]
    pub fn shots(&self) -> &'a [&'a ShotRecord] {
        self.shown
    }

    /// Carry, total, side and apex of each shot shown, in the same order: GSPro's where it sent
    /// them and they are chosen, else the estimate.
    #[must_use]
    pub fn flights(&self) -> &'a [FlightSummary] {
        self.flights
    }

    /// Whether GSPro's numbers are chosen where it sent them.
    #[must_use]
    pub fn gspro(&self) -> bool {
        self.gspro
    }

    #[must_use]
    pub fn units(&self) -> Units {
        self.units
    }

    /// The shots are of all time (the history), not the session.
    #[must_use]
    pub fn all_time(&self) -> bool {
        self.all_time
    }

    /// Shown, not used (the tile overlays): no controls.
    #[must_use]
    pub fn display(&self) -> bool {
        self.display
    }

    /// The shot clicked last, or the latest.
    #[must_use]
    pub fn focus(&self) -> Option<&'a ShotRecord> {
        self.focus
    }

    #[must_use]
    pub fn is_selected(&self, shot: &ShotRecord) -> bool {
        self.selected.contains(&key(shot))
    }

    /// The colour of a club (by its GSPro code), the same in every tile.
    #[must_use]
    pub fn club_color(&self, code: &Option<String>) -> Color32 {
        (self.color_of)(code)
    }

    /// Seconds since `shot` started flying (a new shot, or played again), if it is.
    #[must_use]
    pub fn animation(&self, shot: &ShotRecord) -> Option<f32> {
        self.anim.filter(|(k, _)| **k == key(shot)).map(|(_, t)| t)
    }

    /// An id for a widget of this tile.
    #[must_use]
    pub fn id(&self, salt: impl std::hash::Hash + std::fmt::Debug) -> egui::Id {
        egui::Id::new((salt, self.index))
    }

    /// The user clicked `shot`: it is selected, or taken out of the selection.
    pub fn click(&self, shot: &ShotRecord) {
        self.clicked.set(Some(key(shot)));
    }

    /// Plays `shot`'s flight again.
    pub fn replay(&self, shot: &ShotRecord) {
        self.replay.set(Some(key(shot)));
    }

    /// What this tile keeps for the session (its 3D camera, say), made with `T::default()`
    /// the first time. Held while borrowed: drop it before asking again.
    pub fn state<T: Default + 'static>(&self) -> RefMut<'a, T> {
        RefMut::map(self.states.tiles.borrow_mut(), |m| {
            let slot = m.entry((self.page, self.index)).or_insert_with(|| Box::new(T::default()));
            if !slot.is::<T>() {
                // Another kind's, before the tile's kind changed.
                *slot = Box::new(T::default());
            }
            slot.downcast_mut::<T>().expect("a T was just put there")
        })
    }

    /// What all tiles that keep a `T` share for the session (e.g. a cache), in the explorer and
    /// the tile overlays.
    pub fn shared<T: Default + 'static>(&self) -> RefMut<'a, T> {
        RefMut::map(self.states.shared.borrow_mut(), |m| {
            m.entry(TypeId::of::<T>()).or_insert_with(|| Box::new(T::default())).downcast_mut::<T>().expect("kept by its type")
        })
    }
}

#[expect(clippy::too_many_arguments, reason = "a tile's place, shared context and the edits are passed through as is")]
fn tile_ui(ui: &mut egui::Ui, rect: Rect, i: usize, tile: &Tile, arrange: bool, count: usize, ctx: &TileCtx, edits: &mut Vec<Edit>) {
    let p = ui.painter();
    p.rect(rect, 8.0, TILE, Stroke::new(1.0, if arrange { BUTTON_LINE } else { LINE }), StrokeKind::Inside);
    let head_h = 28.0;
    let head = Rect::from_min_size(rect.min, vec2(rect.width(), head_h)).shrink2(vec2(8.0, 0.0));
    p.line_segment([pos2(rect.left(), head.bottom()), pos2(rect.right(), head.bottom())], Stroke::new(1.0, LINE));
    let body = Rect::from_min_max(pos2(rect.left() + 8.0, head.bottom() + 4.0), rect.max - vec2(8.0, 6.0));

    let mut head_ui = ui.new_child(egui::UiBuilder::new().max_rect(head).layout(egui::Layout::left_to_right(egui::Align::Center)));
    head_ui.spacing_mut().item_spacing.x = 6.0;
    // The kind changes the settings in its copy of the tile; changed ones are saved.
    let mut changed = tile.clone();
    if arrange {
        let mut kind = tile.kind.clone();
        // Narrow tiles: a narrower menu and no arrows (tiles can be dragged).
        let narrow = rect.width() < 230.0;
        let kind_w = if narrow { (rect.width() - 120.0).max(40.0) } else { 90.0 };
        egui::ComboBox::from_id_salt(("tile-kind", i)).width(kind_w).selected_text(kind_name(&tile.kind)).show_ui(&mut head_ui, |ui| {
            for k in kinds() {
                ui.selectable_value(&mut kind, k.id().to_string(), k.name());
            }
        });
        if kind != tile.kind {
            edits.push(Edit::Kind(i, kind));
        }
        head_ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let remove = ui.add(egui::Button::new(RichText::new("×").size(19.0).color(RED)).frame(false));
            if remove.on_hover_text(tr("Remove tile")).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                edits.push(Edit::Remove(i));
            }
            let mut size = tile.size;
            egui::ComboBox::from_id_salt(("tile-size", i)).width(56.0).selected_text(size_name(size)).show_ui(ui, |ui| {
                for s in TileSize::ALL {
                    ui.selectable_value(&mut size, s, size_name(s));
                }
            });
            if size != tile.size {
                edits.push(Edit::Size(i, size));
            }
            if !narrow && i + 1 < count && link(ui, "▶", LABEL).on_hover_text(tr("Move later")).clicked() {
                edits.push(Edit::Move(i, 1));
            }
            if !narrow && i > 0 && link(ui, "◀", LABEL).on_hover_text(tr("Move earlier")).clicked() {
                edits.push(Edit::Move(i, -1));
            }
        });
    } else {
        let kind = kind_of(&tile.kind);
        if rect.width() >= kind.map_or(0.0, TileKind::title_from) {
            head_ui.label(RichText::new(kind_name(&tile.kind).to_uppercase()).font(cond(14.0)).color(LABEL).extra_letter_spacing(1.2));
        }
        if let Some(kind) = kind {
            head_ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| kind.header(ui, &mut changed, ctx));
        }
    }

    tile_body(ui, body, &mut changed, ctx);
    if changed.config != tile.config {
        edits.push(Edit::Config(i, changed.config));
    }
}

/// What a tile shows, drawn into `body` by its kind; what it needs if this build doesn't have it.
fn tile_body(ui: &mut egui::Ui, body: Rect, tile: &mut Tile, ctx: &TileCtx) {
    let mut body_ui = ui.new_child(egui::UiBuilder::new().max_rect(body).layout(egui::Layout::top_down(egui::Align::Min)));
    body_ui.set_clip_rect(body.intersect(ui.clip_rect()));
    match kind_of(&tile.kind) {
        Some(kind) => kind.body(&mut body_ui, tile, ctx),
        None => {
            let text = tr("This tile needs a newer or extended version of the app (“{kind}”).").replace("{kind}", &tile.kind);
            body_ui.label(RichText::new(text).color(MUTED));
        }
    }
}

/// A shot drawn as a dot in a chart.
struct Dot {
    pos: egui::Pos2,
    color: Color32,
    selected: bool,
    excluded: bool,
    key: Key,
    tip: String,
}

/// Draw the dots (selected ones on top); hovering shows the shot, a click returns it.
fn draw_dots(ui: &mut egui::Ui, id: egui::Id, rect: Rect, dots: &[Dot]) -> Option<Key> {
    let resp = ui.interact(rect, id, Sense::click());
    let p = ui.painter_at(rect);
    for selected in [false, true] {
        for d in dots.iter().filter(|d| d.selected == selected) {
            let alpha = if d.excluded { 0.3 } else { 1.0 };
            if d.selected {
                p.circle(d.pos, 5.5, d.color.gamma_multiply(alpha), Stroke::new(2.0, WHITE));
            } else {
                p.circle(d.pos, 3.8, d.color.gamma_multiply(alpha), Stroke::new(1.0, PAGE));
            }
        }
    }
    let near = resp.hover_pos().and_then(|at| {
        dots.iter()
            .map(|d| (d, d.pos.distance(at)))
            .filter(|(_, dist)| *dist <= 9.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(d, _)| d)
    });
    let clicked = resp.clicked();
    if let Some(d) = near {
        p.circle_stroke(d.pos, 7.5, Stroke::new(1.0, SOFT));
        resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(&d.tip);
        if clicked {
            return Some(d.key.clone());
        }
    }
    None
}

/// Mean (cross) and an ellipse around all of each club's points that count (dashed). The ellipse
/// follows the spread of the points (its axes may be tilted) and is grown until every point is inside.
fn draw_spread(p: &egui::Painter, groups: &[(Color32, Vec<egui::Pos2>)]) {
    for (color, pts) in groups {
        if pts.is_empty() {
            continue;
        }
        let n = pts.len() as f32;
        let mean = pts.iter().fold(egui::Vec2::ZERO, |a, q| a + q.to_vec2()) / n;
        if pts.len() > 1
            && let Some(ring) = enclosing_ellipse(mean, pts)
        {
            p.extend(egui::Shape::dashed_line(&ring, Stroke::new(1.3, *color), 5.0, 4.0));
        }
        let c = mean.to_pos2();
        p.line_segment([c - vec2(7.0, 0.0), c + vec2(7.0, 0.0)], Stroke::new(2.0, WHITE));
        p.line_segment([c - vec2(0.0, 7.0), c + vec2(0.0, 7.0)], Stroke::new(2.0, WHITE));
        p.circle_filled(c, 3.0, *color);
    }
}

/// Outline of an ellipse around `mean` that holds all `pts`, with a little room: shaped by the
/// points' covariance (principal axes), scaled by the farthest point.
fn enclosing_ellipse(mean: egui::Vec2, pts: &[egui::Pos2]) -> Option<Vec<egui::Pos2>> {
    let (mut sxx, mut syy, mut sxy) = (0.0_f32, 0.0_f32, 0.0_f32);
    for q in pts {
        let d = q.to_vec2() - mean;
        sxx += d.x * d.x;
        syy += d.y * d.y;
        sxy += d.x * d.y;
    }
    let angle = 0.5 * (2.0 * sxy).atan2(sxx - syy);
    let (cos, sin) = (angle.cos(), angle.sin());
    // Spread along the two axes; a floor keeps a line of points from collapsing the ellipse.
    let along = |q: &egui::Pos2| {
        let d = q.to_vec2() - mean;
        (d.x * cos + d.y * sin, -d.x * sin + d.y * cos)
    };
    let (mut a, mut b) = pts.iter().map(along).fold((0.0_f32, 0.0_f32), |(a, b), (u, v)| (a + u * u, b + v * v));
    a = (a / pts.len() as f32).sqrt().max(3.0);
    b = (b / pts.len() as f32).sqrt().max(3.0);
    let k = pts.iter().map(along).map(|(u, v)| ((u / a).powi(2) + (v / b).powi(2)).sqrt()).fold(0.0_f32, f32::max);
    if !k.is_finite() {
        return None;
    }
    let (ra, rb) = (a * k + 6.0, b * k + 6.0);
    Some(
        (0..=64)
            .map(|i| {
                let t = i as f32 / 64.0 * std::f32::consts::TAU;
                let (u, v) = (t.cos() * ra, t.sin() * rb);
                (mean + vec2(u * cos - v * sin, u * sin + v * cos)).to_pos2()
            })
            .collect(),
    )
}

/// Points of the shots that count, grouped by club in the order of the clubs.
fn club_groups(ctx: &TileCtx, mut at: impl FnMut(usize) -> Option<egui::Pos2>) -> Vec<(Color32, Vec<egui::Pos2>)> {
    let mut groups: Vec<(Option<String>, Color32, Vec<egui::Pos2>)> = Vec::new();
    for (n, s) in ctx.shown.iter().enumerate() {
        if s.excluded {
            continue;
        }
        let Some(q) = at(n) else { continue };
        match groups.iter_mut().find(|g| g.0 == s.club_code) {
            Some(g) => g.2.push(q),
            None => groups.push((s.club_code.clone(), (ctx.color_of)(&s.club_code), vec![q])),
        }
    }
    groups.into_iter().map(|(_, c, pts)| (c, pts)).collect()
}

/// Tab of an explorer page.
fn page_tab(ui: &mut egui::Ui, name: &str, active: bool) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(name.to_string(), FontId::proportional(13.0), WHITE);
    let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 22.0, 28.0), Sense::click());
    let p = ui.painter();
    if active {
        p.rect_filled(rect, 6.0, ACCENT);
    } else if resp.hovered() {
        p.rect_filled(rect, 6.0, BUTTON_HOVER);
    }
    let color = if active { WHITE } else if resp.hovered() { SOFT } else { LABEL };
    p.text(rect.center(), Align2::CENTER_CENTER, name, FontId::proportional(13.0), color);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn text_width(ui: &egui::Ui, text: &str, size: f32) -> f32 {
    ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(size), WHITE).size().x
}

fn chip_width(ui: &egui::Ui, label: &str, dot: bool) -> f32 {
    text_width(ui, label, 12.5) + if dot { 14.0 } else { 0.0 } + 20.0
}

/// Position of a GSPro club code in a bag: driver, woods, hybrids, irons, wedges, putter.
fn club_rank(code: Option<&str>) -> (u8, u8) {
    let Some(code) = code else { return (9, 0) };
    let n = code.get(1..).and_then(|n| n.parse::<u8>().ok()).unwrap_or(0);
    match code {
        "DR" => (0, 0),
        "PW" => (5, 0),
        "GW" | "AW" => (5, 1),
        "SW" => (5, 2),
        "LW" => (5, 3),
        "PT" => (6, 0),
        c if c.starts_with('W') => (1, n),
        c if c.starts_with('H') => (2, n),
        c if c.starts_with('I') => (3, n),
        _ => (8, 0),
    }
}

/// The club filter as a menu, when the chips don't fit: "Clubs 12/14" with a tick per club.
fn club_menu(ui: &mut egui::Ui, codes: &[Option<String>], hidden: &mut BTreeSet<Option<String>>, color_of: &dyn Fn(&Option<String>) -> Color32) {
    let shown = codes.iter().filter(|c| !hidden.contains(*c)).count();
    let label = format!("{} {shown}/{}  ▼", tr("Clubs"), codes.len());
    let resp = chip(ui, &label, None, shown < codes.len());
    egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        ui.horizontal(|ui| {
            if ui.button(tr("All")).clicked() {
                hidden.clear();
            }
            if ui.button(tr("None")).clicked() {
                hidden.extend(codes.iter().cloned());
            }
        });
        ui.separator();
        for code in codes {
            let mut on = !hidden.contains(code);
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(vec2(10.0, 18.0), Sense::hover());
                ui.painter().circle_filled(dot.center(), 4.0, color_of(code));
                let name = code.as_deref().map_or(tr("Unknown").to_string(), |c| format!("{c} · {}", club_name(c)));
                if ui.checkbox(&mut on, name).changed() {
                    if on {
                        hidden.remove(code);
                    } else {
                        hidden.insert(code.clone());
                    }
                }
            });
        }
    });
}

/// Rounded filter chip, with an optional colour dot.
fn chip(ui: &mut egui::Ui, label: &str, dot: Option<Color32>, on: bool) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(12.5), WHITE);
    let dot_w = if dot.is_some() { 14.0 } else { 0.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + dot_w + 20.0, 24.0), Sense::click());
    let p = ui.painter();
    let (fill, border, text) = if on { (BUTTON, ACCENT, WHITE) } else { (Color32::TRANSPARENT, FIELD_LINE, MUTED) };
    p.rect(rect, 12.0, fill, Stroke::new(1.0, if resp.hovered() { ICON } else { border }), StrokeKind::Inside);
    if let Some(c) = dot {
        p.circle_filled(pos2(rect.left() + 14.0, rect.center().y), 4.0, if on { c } else { c.gamma_multiply(0.4) });
    }
    p.text(pos2(rect.left() + 10.0 + dot_w, rect.center().y), Align2::LEFT_CENTER, label, FontId::proportional(12.5), text);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Text button without a frame.
fn link(ui: &mut egui::Ui, label: &str, color: Color32) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(label).size(13.0).color(color)).frame(false))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

#[cfg(test)]
impl ExplorerState {
    /// Page shown, configure mode, clubs filtered out and one shot selected, for the README screenshots.
    pub(super) fn set_for_screenshot(&mut self, page: usize, configure: bool, hidden: BTreeSet<Option<String>>, selected: Option<Key>) {
        self.page = page;
        self.arrange = configure;
        self.hidden_clubs = hidden;
        self.selected = selected.iter().cloned().collect();
        self.focus = selected;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Renders the explorer in a few states (averages page, a selection, arranging):
    /// `OPENLAUNCH_SCREENSHOT_DIR=<dir> cargo test --lib explorer_states -- --ignored`
    #[test]
    #[ignore = "writes screenshots; run on demand"]
    fn explorer_states() {
        use egui_kittest::Harness;

        let language = if std::env::var("OPENLAUNCH_SCREENSHOT_LANG").as_deref() == Ok("de") { Language::German } else { Language::English };
        i18n::set(language);
        let _state = super::super::tests::demo_state();
        let keys: Vec<Key> = state::with(|s| s.session.iter().rev().take(2).map(key).collect());
        state::with(|s| s.session.iter_mut().rev().nth(1).unwrap().excluded = true);
        type Setup = fn(&mut App, &[Key]);
        let states: [(&str, Setup); 10] = [
            ("explorer-averages.png", |app, _| app.explorer.page = 2),
            ("explorer-selection.png", |app, keys| {
                app.explorer.selected = keys.iter().cloned().collect();
                app.explorer.focus = keys.last().cloned();
            }),
            ("explorer-arrange.png", |app, _| {
                app.saved.explorer.pages[0].tiles[0].size = TileSize::W3H2;
                app.explorer.arrange = true;
            }),
            ("explorer-charts.png", |app, _| {
                state::with(|s| s.session = engine::demo_range());
                let keys: Vec<Key> = state::with(|s| s.session.iter().rev().take(1).map(key).collect());
                let axes = scatter::Config { x: scatter::Metric::BallSpeed, y: scatter::Metric::Carry };
                app.saved.explorer.pages[0].tiles = vec![
                    Tile::new("dispersion", TileSize::W2H2),
                    Tile::new("scatter", TileSize::W2H2),
                    Tile::new("dispersion", TileSize::W1H2).with(&serde_json::json!({ "total": true })),
                    Tile::new("scatter", TileSize::W1H1).with(&axes),
                    Tile::new("details", TileSize::W1H1),
                ];
                app.explorer.selected = keys.iter().take(1).cloned().collect();
            }),
            ("explorer-flight.png", |app, _| {
                state::with(|s| s.session = engine::demo_range());
                let tile = |size, config| Tile::new("flight", size).with(&config);
                app.saved.explorer.pages[0].tiles = vec![
                    tile(TileSize::W4H2, serde_json::json!({ "camera": "tee", "averages": true })),
                    tile(TileSize::W2H1, serde_json::json!({ "camera": "side" })),
                    tile(TileSize::W2H1, serde_json::json!({ "camera": "above" })),
                ];
                // A driver selected: glow, apex line, labels.
                let driver = state::with(|s| s.session.iter().find(|r| r.club_code.as_deref() == Some("DR")).map(key));
                app.explorer.selected = driver.iter().cloned().collect();
                app.explorer.hidden_clubs = [Some("W5"), Some("H3"), Some("I6"), Some("I8"), Some("GW"), Some("LW")]
                    .into_iter()
                    .map(|c| c.map(String::from))
                    .collect();
            }),
            ("explorer-anim.png", |app, _| {
                state::with(|s| s.session = engine::demo_range());
                app.explorer.page = 1;
                let driver = state::with(|s| s.session.iter().find(|r| r.club_code.as_deref() == Some("DR")).map(key));
                app.explorer.anim = driver.map(|k| (k, Instant::now() - Duration::from_millis(2200)));
                app.explorer.last_latest = Some(state::with(|s| s.session.last().map(key)));
            }),
            ("explorer-orbit.png", |app, _| {
                state::with(|s| s.session = engine::demo_range());
                app.explorer.page = 1;
                let states = &app.explorer.states;
                states.set((1, 0), flight3d::TileView::turned(0.9, 0.25, 1.0));
                states.set((1, 1), flight3d::TileView::turned(-0.5, 0.5, 1.4));
                states.set((1, 2), flight3d::TileView::turned(2.4, -0.2, 1.0));
            }),
            ("explorer-roll.png", |app, _| {
                state::with(|s| s.session = engine::demo_range());
                app.explorer.page = 1;
                let driver = state::with(|s| s.session.iter().find(|r| r.club_code.as_deref() == Some("DR")).map(key));
                app.explorer.anim = driver.map(|k| (k, Instant::now() - Duration::from_millis(6600)));
                app.explorer.last_latest = Some(state::with(|s| s.session.last().map(key)));
            }),
            ("explorer-export.png", |app, _| app.explorer.export_dialog = true),
            ("explorer-reset.png", |app, _| {
                app.page = Page::Settings;
                app.saved.explorer.pages.pop();
                app.confirm_reset = true;
            }),
        ];
        for (name, setup) in states {
            let settings = Settings { overlay: OverlayPos::Off, language, ..Settings::default() };
            let (tx, _rx) = std::sync::mpsc::channel();
            let mut harness = Harness::builder()
                .with_size(BASE_SIZE)
                .with_pixels_per_point(2.0)
                .wgpu()
                .build_eframe(|cc| {
                    setup_style(&cc.egui_ctx);
                    App::new(settings, tx)
                });
            harness.state_mut().page = Page::Shots;
            harness.state_mut().remember_size = false;
            setup(harness.state_mut(), &keys);
            for _ in 0..6 {
                harness.step();
            }
            super::super::tests::save(&harness.render().unwrap(), name);
        }
    }

    /// Dragging the first tile onto the second one's place swaps them.
    #[test]
    fn tiles_are_dragged_into_place() {
        use egui_kittest::Harness;

        // Shots recorded by the lib's tests beside it would end up in the tiles.
        let _state = crate::test_util::state_lock();
        // Moving a tile saves the layout at once – not into the real settings file.
        crate::settings::SAVE.store(false, std::sync::atomic::Ordering::Relaxed);
        let settings = Settings { overlay: OverlayPos::Off, ..Settings::default() };
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut harness = Harness::builder().with_size(BASE_SIZE).build_eframe(|cc| {
            setup_style(&cc.egui_ctx);
            App::new(settings, tx)
        });
        harness.state_mut().page = Page::Shots;
        harness.state_mut().remember_size = false;
        harness.state_mut().show_overlay = false;
        harness.state_mut().explorer.arrange = true;
        for _ in 0..3 {
            harness.step();
        }
        // Default page: table 3×2, dispersion 2×2, flight 1×2 (six columns).
        let (from, to) = (pos2(250.0, 300.0), pos2(650.0, 300.0));
        let events = [
            egui::Event::PointerMoved(from),
            egui::Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE },
            egui::Event::PointerMoved(from + vec2(20.0, 0.0)),
            egui::Event::PointerMoved(to),
            egui::Event::PointerButton { pos: to, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE },
        ];
        for event in events {
            harness.input_mut().events.push(event);
            harness.step();
        }
        harness.step();
        let kinds: Vec<String> = harness.state().saved.explorer.pages[0].tiles.iter().map(|t| t.kind.clone()).collect();
        assert_eq!(kinds, ["dispersion", "table", "flight"]);
    }

    /// The *Shots | Mean* switch in a 3D tile takes the click (not the landing points under it).
    #[test]
    fn flight_tile_switches_to_the_means() {
        use egui_kittest::Harness;

        let _state = crate::test_util::state_lock();
        crate::settings::SAVE.store(false, std::sync::atomic::Ordering::Relaxed);
        let settings = Settings { overlay: OverlayPos::Off, ..Settings::default() };
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut harness = Harness::builder().with_size(BASE_SIZE).build_eframe(|cc| {
            setup_style(&cc.egui_ctx);
            App::new(settings, tx)
        });
        harness.state_mut().page = Page::Shots;
        harness.state_mut().remember_size = false;
        harness.state_mut().show_overlay = false;
        harness.state_mut().explorer.page = 1; // Ball flight: 3D from the tee (2×2) first
        for _ in 0..3 {
            harness.step();
        }
        // The switch sits at the top right of the first tile's view, "Mean" on its right.
        let tile = harness.state().explorer.tile_rects.first().copied().expect("tile drawn");
        let at = pos2(tile.right() - 30.0, tile.top() + 28.0 + 4.0 + 16.0);
        for event in [
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE },
            egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE },
        ] {
            harness.input_mut().events.push(event);
            harness.step();
        }
        harness.step();
        assert_eq!(harness.state().saved.explorer.pages[1].tiles[0].config["averages"], true, "switched to the means");
    }

    /// Dragging in a 3D tile turns its camera, a double click goes back; a new shot flies.
    #[test]
    fn flight_tile_orbits_and_animates() {
        use egui_kittest::Harness;

        let _state = super::super::tests::demo_state();
        let settings = Settings { overlay: OverlayPos::Off, ..Settings::default() };
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut harness = Harness::builder().with_size(BASE_SIZE).build_eframe(|cc| {
            setup_style(&cc.egui_ctx);
            App::new(settings, tx)
        });
        harness.state_mut().page = Page::Shots;
        harness.state_mut().remember_size = false;
        harness.state_mut().show_overlay = false;
        harness.state_mut().explorer.page = 1; // Ball flight
        for _ in 0..3 {
            harness.step();
        }
        let tile = harness.state().explorer.tile_rects[0];
        let from = tile.center();
        let press = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        for event in [egui::Event::PointerMoved(from), press(from, true), egui::Event::PointerMoved(from + vec2(60.0, 20.0)), press(from + vec2(60.0, 20.0), false)] {
            harness.input_mut().events.push(event);
            harness.step();
        }
        let orbit = |h: &Harness<'_, App>| h.state().explorer.states.get::<flight3d::TileView>((1, 0)).map(|v| v.orbit_state());
        let (yaw, pitch, _) = orbit(&harness).expect("view kept");
        assert!(yaw < 0.0 && pitch > 0.0, "turned: {yaw} {pitch}");
        // Both clicks within one frame: kittest's clock doesn't move between steps.
        harness.input_mut().events.extend([egui::Event::PointerMoved(from), press(from, true), press(from, false), press(from, true), press(from, false)]);
        harness.step();
        harness.step();
        assert_eq!(orbit(&harness), Some((0.0, 0.0, 1.0)), "double click resets");

        // A new shot in the session starts its flight.
        assert!(harness.state().explorer.anim.is_none());
        state::with(|s| {
            let mut shot = s.session.last().cloned().unwrap();
            shot.shot_number += 1000;
            s.session.push(shot);
        });
        harness.step();
        assert!(harness.state().explorer.anim.is_some());
    }

    /// The app on the Shots page with one page of `tiles`, configuring or not.
    fn explorer_with(tiles: Vec<Tile>, arrange: bool) -> egui_kittest::Harness<'static, App> {
        crate::settings::SAVE.store(false, std::sync::atomic::Ordering::Relaxed);
        let mut settings = Settings { overlay: OverlayPos::Off, ..Settings::default() };
        settings.explorer.pages = vec![ExplorerPage { name: "Test".into(), tiles }];
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut harness = egui_kittest::Harness::builder().with_size(BASE_SIZE).build_eframe(|cc| {
            setup_style(&cc.egui_ctx);
            App::new(settings, tx)
        });
        harness.state_mut().page = Page::Shots;
        harness.state_mut().remember_size = false;
        harness.state_mut().show_overlay = false;
        harness.state_mut().explorer.arrange = arrange;
        for _ in 0..3 {
            harness.step();
        }
        harness
    }

    /// A kind as another crate would add it: only the public interface.
    struct TestKind;

    impl TileKind for TestKind {
        fn id(&self) -> &'static str {
            "test-kind"
        }

        fn name(&self) -> &'static str {
            "Test kind"
        }

        fn body(&self, ui: &mut egui::Ui, tile: &mut Tile, ctx: &TileCtx) {
            ui.label("Drawn by the test kind");
            // Counts its frames in its state and its settings (saved).
            *ctx.state::<u32>() += 1;
            if !ctx.display() {
                tile.change_config(|n: &mut serde_json::Map<String, serde_json::Value>| {
                    n.insert("drawn".into(), true.into());
                });
            }
        }
    }

    /// A registered kind comes after the core's in the add-tile menu; picked there, it draws its tile.
    #[test]
    fn a_registered_kind_is_added_and_drawn() {
        use egui_kittest::kittest::Queryable;

        register_tile_kind(&TestKind);
        register_tile_kind(&TestKind);
        let ids: Vec<&str> = kinds().iter().map(|k| k.id()).collect();
        assert_eq!(ids[..CORE_KINDS.len()], CORE_KINDS.map(TileKind::id), "the core's first");
        assert_eq!(ids.iter().filter(|id| **id == "test-kind").count(), 1, "registered once");

        let _state = crate::test_util::state_lock();
        let mut harness = explorer_with(vec![Tile::new("details", TileSize::W1H1)], true);
        harness.get_by_label("Add tile").click();
        // The menu is placed in the frame after it opens.
        for _ in 0..3 {
            harness.step();
        }
        harness.get_by_label("Test kind").click();
        for _ in 0..3 {
            harness.step();
        }
        let tiles = harness.state().saved.explorer.pages[0].tiles.clone();
        assert_eq!(tiles.iter().map(|t| t.kind.as_str()).collect::<Vec<_>>(), ["details", "test-kind"]);
        harness.get_by_label("Drawn by the test kind");
        assert_eq!(tiles[1].config["drawn"], true, "its settings are saved");
        assert!(harness.state().explorer.states.get::<u32>((0, 1)).is_some_and(|n| *n >= 2), "its state is kept");
    }

    /// A tile of a kind this build doesn't have says what it needs, and is kept.
    #[test]
    fn a_tile_of_an_unknown_kind_shows_what_it_needs() {
        use egui_kittest::kittest::Queryable;

        let _state = crate::test_util::state_lock();
        let gapping = Tile::new("gapping", TileSize::W2H2).with(&serde_json::json!({ "bins": 5 }));
        let mut harness = explorer_with(vec![gapping.clone(), Tile::new("details", TileSize::W2H2)], false);
        harness.get_by_label("GAPPING");
        harness.get_by_label("This tile needs a newer or extended version of the app (“gapping”).");
        harness.state_mut().explorer.arrange = true;
        harness.step();
        harness.step();
        assert_eq!(harness.state().saved.explorer.pages[0].tiles[0], gapping);
    }

    /// The tiles the app starts with are of the core's kinds.
    #[test]
    fn default_tiles_are_of_the_core_kinds() {
        let pages = crate::settings::Explorer::default().pages;
        let overlay = TileOverlay::default().tile;
        for tile in pages.iter().flat_map(|p| &p.tiles).chain([&Tile::default(), &overlay]) {
            assert!(CORE_KINDS.iter().any(|k| k.id() == tile.kind), "{}", tile.kind);
        }
    }

    #[test]
    fn ellipse_encloses_every_point() {
        let inside = |ring: &[egui::Pos2], q: egui::Pos2| {
            // Ray casting to the right, just off the height of the ring's vertices.
            let q = q + vec2(0.0, 0.013);
            let mut hit = false;
            for w in ring.windows(2) {
                let (a, b) = (w[0], w[1]);
                if (a.y > q.y) != (b.y > q.y) && q.x < a.x + (q.y - a.y) / (b.y - a.y) * (b.x - a.x) {
                    hit = !hit;
                }
            }
            hit
        };
        let sets: [Vec<egui::Pos2>; 3] = [
            // A tilted cloud with an outlier, a straight line, two points.
            vec![pos2(0.0, 0.0), pos2(10.0, 8.0), pos2(20.0, 17.0), pos2(30.0, 24.0), pos2(5.0, 30.0)],
            vec![pos2(0.0, 0.0), pos2(10.0, 0.0), pos2(40.0, 0.0)],
            vec![pos2(100.0, 50.0), pos2(104.0, 90.0)],
        ];
        for pts in sets {
            let mean = pts.iter().fold(egui::Vec2::ZERO, |a, q| a + q.to_vec2()) / pts.len() as f32;
            let ring = enclosing_ellipse(mean, &pts).unwrap();
            assert!(pts.iter().all(|q| inside(&ring, *q)), "{pts:?}");
        }
    }

    #[test]
    fn default_page_names_are_recognised_in_both_languages() {
        // English only: switching the global language would race other tests.
        let page = |name: &str| ExplorerPage { name: name.into(), tiles: Vec::new() };
        assert_eq!(page_name(&page("Ballflug"), 1), "Ball flight", "typed in German, shown in English");
        assert_eq!(page_name(&page("Daten"), 2), "Data");
        assert_eq!(page_name(&page("Driver only"), 0), "Driver only");
        assert_eq!(page_name(&page(""), 3), "Page 4");
    }

    #[test]
    fn clubs_sort_in_bag_order() {
        let mut codes = vec![Some("PW"), None, Some("I7"), Some("H4"), Some("DR"), Some("LW"), Some("W3"), Some("I10"), Some("GW"), Some("PT")];
        codes.sort_by_key(|c| club_rank(*c));
        assert_eq!(codes, [Some("DR"), Some("W3"), Some("H4"), Some("I7"), Some("I10"), Some("PW"), Some("GW"), Some("LW"), Some("PT"), None]);
    }

    #[test]
    fn grid_has_near_square_cells() {
        // Base window: two rows of six.
        let (cols, rows, cell) = grid(vec2(964.0, 316.0));
        assert_eq!((cols, rows), (6, 2));
        assert!((cell.x / cell.y - 1.0).abs() < 0.1, "{cell:?}");
        // A large window gets more rows and columns.
        let (cols, rows, cell) = grid(vec2(1564.0, 744.0));
        assert_eq!(rows, 4);
        assert!(cols >= 8 && (cell.x / cell.y - 1.0).abs() < 0.1, "{cols} {cell:?}");
    }

    #[test]
    fn tiles_fill_gaps_row_by_row() {
        use TileSize::*;
        // 3×2 and 2×2 side by side, then a 1×1 below; a 4×2 doesn't fit next to them.
        assert_eq!(place(&[W3H2, W2H2, W1H1], 5, 2), vec![(0, 0, 3, 2), (3, 0, 2, 2), (0, 2, 1, 1)]);
        assert_eq!(place(&[W2H1, W4H2, W1H1], 5, 2), vec![(0, 0, 2, 1), (0, 1, 4, 2), (2, 0, 1, 1)]);
        // Too wide for the grid: clamped; a page tile covers the visible grid.
        assert_eq!(place(&[W4H2, Page], 3, 2), vec![(0, 0, 3, 2), (0, 2, 3, 2)]);
    }
}
