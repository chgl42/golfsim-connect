//! SHOTS page: the shots explorer. Pages of tiles (table, details) on a grid of near-square
//! cells; the filter and the selection are shared by all tiles.

use std::collections::{BTreeSet, HashMap};

use openlaunch_connect::flight::{self, Flight};
use openlaunch_connect::history::Stat;
use openlaunch_connect::settings::{ExplorerPage, Metric, Tile, TileKind, TileSize};

use super::*;

/// Gap between tiles.
const GAP: f32 = 8.0;
/// Cell size the grid aims for; the rows are fitted to the height.
const CELL_TARGET: f32 = 170.0;
const ROW_H: f32 = 21.0;
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
    /// Estimated flight of every shot seen, computed once.
    flights: HashMap<Key, FlightSummary>,
}

/// The estimated flight values the tiles use (`flight::fly` without the path).
#[derive(Clone, Copy)]
struct FlightSummary {
    carry: f64,
    total: f64,
    carry_side: f64,
    total_side: f64,
    apex: f64,
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

fn kind_name(kind: TileKind) -> &'static str {
    match kind {
        TileKind::Table => tr("Table"),
        TileKind::Details => tr("Details"),
        TileKind::Dispersion => tr("Dispersion"),
        TileKind::Scatter => tr("Correlation"),
    }
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
    if page.name.trim().is_empty() { tr("Page {n}").replace("{n}", &(i + 1).to_string()) } else { page.name.clone() }
}

/// Changes to a page from its tab's menu, applied afterwards.
enum PageEdit {
    Rename(usize, String),
    AddPage,
    Remove(usize),
}

/// Changes to the layout made while drawing, applied afterwards.
enum Edit {
    Kind(usize, TileKind),
    Size(usize, TileSize),
    Averages(usize, bool),
    Total(usize, bool),
    Axes(usize, Metric, Metric),
    Move(usize, isize),
    /// Dragged onto another tile's place.
    MoveTo(usize, usize),
    Add(TileKind),
    Remove(usize),
}

/// Units and conversions for the values in the tiles.
#[derive(Clone, Copy)]
struct Units {
    metric: bool,
    meters: bool,
}

impl Units {
    fn speed(self, mph: f64) -> f64 {
        if self.metric { mph * MPH_TO_KMH } else { mph }
    }
    fn speed_unit(self) -> &'static str {
        if self.metric { "km/h" } else { "mph" }
    }
    fn dist(self, m: f64) -> f64 {
        if self.meters { m } else { m * M_TO_YD }
    }
    fn dist_unit(self) -> &'static str {
        if self.meters { "m" } else { "yd" }
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

impl App {
    pub(super) fn shots(&mut self, ui: &mut egui::Ui) {
        let session: Vec<ShotRecord> = state::with(|s| s.session.clone());
        let units = Units { metric: self.saved.metric, meters: self.saved.meters };

        // Source, pages, arranging and export.
        let mut export: Option<Vec<ShotRecord>> = None;
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
        let mut bar_ui = ui.new_child(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let before = self.all_time;
        bar_ui.radio_value(&mut self.all_time, false, tr("This session ({n})").replace("{n}", &session.len().to_string()));
        bar_ui.radio_value(&mut self.all_time, true, tr("All time"));
        if self.all_time && (!before || self.all_shots_session != session.len()) {
            self.all_shots = history::load_all();
            self.all_shots_session = session.len();
        }
        let shots: Vec<ShotRecord> = if self.all_time { self.all_shots.clone() } else { session };
        self.explorer.page = self.explorer.page.min(self.saved.explorer.pages.len().saturating_sub(1));
        bar_ui.add_space(18.0);
        let mut layout_changed = false;
        // A page's tab: click shows it, right-click opens its settings.
        let mut page_edit: Option<PageEdit> = None;
        let removable = self.saved.explorer.pages.len() > 1;
        for (i, page) in self.saved.explorer.pages.iter().enumerate() {
            let mut tab = page_tab(&mut bar_ui, &page_name(page, i), i == self.explorer.page);
            if self.explorer.arrange {
                tab = tab.on_hover_text(tr("Right-click for the page's settings"));
            }
            if tab.clicked() || tab.secondary_clicked() {
                self.explorer.page = i;
            }
            // Stays open while typing the name; the entries close it themselves.
            egui::Popup::context_menu(&tab).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                let mut name = page.name.clone();
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
                PageEdit::Remove(i) => {
                    pages.remove(i);
                    self.explorer.page = self.explorer.page.min(pages.len() - 1);
                }
            }
            layout_changed = true;
        }
        if self.explorer.arrange && link(&mut bar_ui, "+", ICON).on_hover_text(tr("Add a page")).clicked() {
            self.saved.explorer.pages.push(ExplorerPage { name: String::new(), tiles: vec![Tile::default()] });
            self.explorer.page = self.saved.explorer.pages.len() - 1;
            layout_changed = true;
        }
        bar_ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(150.0, 30.0), Sense::hover());
            if button(ui, rect, tr("EXPORT CSV"), Icon::None, false, !shots.is_empty(), tr("No shots to export")) {
                export = Some(Vec::new());
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
            ui.add_space(4.0);
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

        // Clubs in bag order (driver to putter): their colours stay put while filtering.
        let mut codes: Vec<Option<String>> = Vec::new();
        for s in &shots {
            if !codes.contains(&s.club_code) {
                codes.push(s.club_code.clone());
            }
        }
        codes.sort_by_key(|c| club_rank(c.as_deref()));
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
        if let Some(list) = &mut export {
            list.extend(shown.iter().map(|s| (*s).clone()));
        }
        let flights: Vec<FlightSummary> = shown
            .iter()
            .map(|s| *ex.flights.entry(key(s)).or_insert_with(|| FlightSummary::from(&flight::fly(&s.ball))))
            .collect();

        // Second row: the filter and what to do with the selection, or the page while arranging.
        let mut exclude: Option<bool> = None;
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
            let room = row.available_width() - selection_w - chip_width(&row, tr("Hide excluded"), false) - 12.0;
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
                    selected: &ex.selected,
                    color_of: &color_of,
                    units,
                    all_time: self.all_time,
                };
                tile_ui(ui, rect, i, tile, ex.arrange, page_tiles.len(), &ctx, focus, &mut edits, &mut clicked);
                rects.push(rect);
            }
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
        if !edits.is_empty() {
            let tiles = &mut self.saved.explorer.pages[ex.page].tiles;
            for edit in edits {
                match edit {
                    Edit::Kind(i, kind) => tiles[i].kind = kind,
                    Edit::Size(i, size) => tiles[i].size = size,
                    Edit::Averages(i, on) => tiles[i].averages = on,
                    Edit::Total(i, on) => tiles[i].total = on,
                    Edit::Axes(i, x, y) => (tiles[i].x, tiles[i].y) = (x, y),
                    Edit::Add(kind) => tiles.push(Tile { kind, ..Tile::default() }),
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
            layout_changed = true;
        }
        if layout_changed {
            // Applies and saves at once, like the overlay's settings.
            self.edit.explorer = self.saved.explorer.clone();
            if let Err(e) = self.saved.save() {
                log::info(&format!("Could not save the settings: {e}"));
            }
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
        if let Some(list) = export {
            self.export_note = Some(export_csv(&list, self.saved.export_format));
        }
    }
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
                let text = kind_name(tiles[i].kind);
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
fn add_tile(ui: &mut egui::Ui, rect: Rect) -> Option<TileKind> {
    let resp = ui.interact(rect, ui.id().with("add-tile"), Sense::click());
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
        for kind in TileKind::ALL {
            if ui.button(kind_name(kind)).clicked() {
                picked = Some(kind);
            }
        }
    });
    picked
}

/// What every tile draws from.
struct TileCtx<'a> {
    shown: &'a [&'a ShotRecord],
    /// Estimated flight of each shot shown, in the same order.
    flights: &'a [FlightSummary],
    selected: &'a BTreeSet<Key>,
    color_of: &'a dyn Fn(&Option<String>) -> Color32,
    units: Units,
    all_time: bool,
}

#[allow(clippy::too_many_arguments)]
fn tile_ui(
    ui: &mut egui::Ui,
    rect: Rect,
    i: usize,
    tile: &Tile,
    arrange: bool,
    count: usize,
    ctx: &TileCtx,
    focus: Option<&ShotRecord>,
    edits: &mut Vec<Edit>,
    clicked: &mut Option<Key>,
) {
    let p = ui.painter();
    p.rect(rect, 8.0, TILE, Stroke::new(1.0, if arrange { BUTTON_LINE } else { LINE }), StrokeKind::Inside);
    let head_h = 28.0;
    let head = Rect::from_min_size(rect.min, vec2(rect.width(), head_h)).shrink2(vec2(8.0, 0.0));
    p.line_segment([pos2(rect.left(), head.bottom()), pos2(rect.right(), head.bottom())], Stroke::new(1.0, LINE));
    let body = Rect::from_min_max(pos2(rect.left() + 8.0, head.bottom() + 4.0), rect.max - vec2(8.0, 6.0));

    let mut head_ui = ui.new_child(egui::UiBuilder::new().max_rect(head).layout(egui::Layout::left_to_right(egui::Align::Center)));
    head_ui.spacing_mut().item_spacing.x = 6.0;
    if arrange {
        let mut kind = tile.kind;
        egui::ComboBox::from_id_salt(("tile-kind", i)).width(90.0).selected_text(kind_name(kind)).show_ui(&mut head_ui, |ui| {
            for k in TileKind::ALL {
                ui.selectable_value(&mut kind, k, kind_name(k));
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
            if i + 1 < count && link(ui, "▶", LABEL).on_hover_text(tr("Move later")).clicked() {
                edits.push(Edit::Move(i, 1));
            }
            if i > 0 && link(ui, "◀", LABEL).on_hover_text(tr("Move earlier")).clicked() {
                edits.push(Edit::Move(i, -1));
            }
        });
    } else {
        // Narrow tiles with a switch in the header leave out the title; the switch says enough.
        let switch = matches!(tile.kind, TileKind::Table | TileKind::Dispersion);
        if !switch || rect.width() >= 250.0 {
            head_ui.label(RichText::new(kind_name(tile.kind).to_uppercase()).font(cond(14.0)).color(LABEL).extra_letter_spacing(1.2));
        }
        head_ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| match tile.kind {
            TileKind::Table => {
                for (on, label) in [(true, tr("Averages")), (false, tr("Shots"))] {
                    let color = if tile.averages == on { WHITE } else { MUTED };
                    if link(ui, label, color).clicked() && tile.averages != on {
                        edits.push(Edit::Averages(i, on));
                    }
                }
            }
            TileKind::Dispersion => {
                for (on, label) in [(true, tr("Total")), (false, tr("Carry"))] {
                    let color = if tile.total == on { WHITE } else { MUTED };
                    if link(ui, label, color).clicked() && tile.total != on {
                        edits.push(Edit::Total(i, on));
                    }
                }
            }
            TileKind::Scatter => {
                if let Some(r) = correlation(ctx, tile.x, tile.y) {
                    ui.label(RichText::new(format!("r = {r:.2}")).size(12.5).color(SOFT))
                        .on_hover_text(tr("Correlation of the two values (−1 to 1) over the shots shown, excluded ones left out"));
                }
            }
            TileKind::Details => {}
        });
    }

    let mut body_ui = ui.new_child(egui::UiBuilder::new().max_rect(body).layout(egui::Layout::top_down(egui::Align::Min)));
    body_ui.set_clip_rect(body.intersect(ui.clip_rect()));
    match tile.kind {
        TileKind::Table if tile.averages => averages_table(&mut body_ui, i, ctx),
        TileKind::Table => {
            if let Some(k) = shots_table(&mut body_ui, i, ctx) {
                *clicked = Some(k);
            }
        }
        TileKind::Details => details(&mut body_ui, focus, ctx),
        TileKind::Dispersion => {
            if let Some(k) = dispersion(&mut body_ui, i, tile.total, ctx) {
                *clicked = Some(k);
            }
        }
        TileKind::Scatter => {
            if let Some(k) = scatter(&mut body_ui, i, tile, ctx, edits) {
                *clicked = Some(k);
            }
        }
    }
}

/// A value of a shot in the units shown; `None` where the shot doesn't have it.
fn metric_value(m: Metric, s: &ShotRecord, f: &FlightSummary, u: Units) -> Option<f64> {
    let c = s.club.as_ref();
    Some(match m {
        Metric::BallSpeed => u.speed(s.ball.speed),
        Metric::ClubSpeed => u.speed(c?.speed),
        Metric::Smash => s.smash()?,
        Metric::Vla => s.ball.vla,
        Metric::Hla => s.ball.hla,
        Metric::Spin => s.ball.total_spin,
        Metric::SpinAxis => s.ball.spin_axis,
        Metric::AngleOfAttack => c?.angle_of_attack,
        Metric::Path => c?.path,
        Metric::FaceToTarget => c?.face_to_target,
        Metric::FaceToPath => c?.face_to_target - c?.path,
        Metric::Carry => u.dist(f.carry),
        Metric::Total => u.dist(f.total),
        Metric::Side => u.dist(f.total_side),
        Metric::Apex => u.dist(f.apex),
        Metric::Tempo => s.tempo?.ratio(),
    })
}

/// Name of a value with its unit; estimated ones are marked with *.
fn metric_label(m: Metric, u: Units) -> String {
    let (name, unit) = match m {
        Metric::BallSpeed => (tr("Ball Speed"), u.speed_unit()),
        Metric::ClubSpeed => (tr("Club Speed"), u.speed_unit()),
        Metric::Smash => (tr("Smash Factor"), ""),
        Metric::Vla => ("VLA", "°"),
        Metric::Hla => ("HLA", "°"),
        Metric::Spin => (tr("Total Spin"), "rpm"),
        Metric::SpinAxis => (tr("Spin Axis"), "°"),
        Metric::AngleOfAttack => (tr("Club AoA"), "°"),
        Metric::Path => (tr("Club Path"), "°"),
        Metric::FaceToTarget => (tr("Face to Target"), "°"),
        Metric::FaceToPath => (tr("Face to Path"), "°"),
        Metric::Carry => return format!("{}* ({})", tr("Carry"), u.dist_unit()),
        Metric::Total => return format!("{}* ({})", tr("Total"), u.dist_unit()),
        Metric::Side => return format!("{}* ({})", tr("Side"), u.dist_unit()),
        Metric::Apex => return format!("{}* ({})", tr("Apex"), u.dist_unit()),
        Metric::Tempo => (tr("Tempo"), ""),
    };
    if unit.is_empty() { name.to_string() } else { format!("{name} ({unit})") }
}

/// Decimals a value is shown with.
fn metric_digits(m: Metric) -> usize {
    match m {
        Metric::Smash => 2,
        Metric::Spin | Metric::Carry | Metric::Total | Metric::Side | Metric::Apex => 0,
        _ => 1,
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

/// Correlation coefficient of two values over the shots shown that count.
fn correlation(ctx: &TileCtx, x: Metric, y: Metric) -> Option<f64> {
    let pairs: Vec<(f64, f64)> = ctx
        .shown
        .iter()
        .zip(ctx.flights)
        .filter(|(s, _)| !s.excluded)
        .filter_map(|(s, f)| Some((metric_value(x, s, f, ctx.units)?, metric_value(y, s, f, ctx.units)?)))
        .collect();
    pearson(&pairs)
}

fn pearson(pairs: &[(f64, f64)]) -> Option<f64> {
    if pairs.len() < 3 {
        return None;
    }
    let n = pairs.len() as f64;
    let (mx, my) = pairs.iter().fold((0.0, 0.0), |(a, b), (x, y)| (a + x / n, b + y / n));
    let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
    for (x, y) in pairs {
        sxy += (x - mx) * (y - my);
        sxx += (x - mx) * (x - mx);
        syy += (y - my) * (y - my);
    }
    (sxx > 0.0 && syy > 0.0).then(|| sxy / (sxx * syy).sqrt())
}

/// Two values of the shots against each other, chosen in the tile. Returns the shot clicked.
fn scatter(ui: &mut egui::Ui, i: usize, tile: &Tile, ctx: &TileCtx, edits: &mut Vec<Edit>) -> Option<Key> {
    let u = ctx.units;
    let (mut x, mut y) = (tile.x, tile.y);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let w = ((ui.available_width() - 36.0) / 2.0).max(40.0);
        for (axis, value) in [("Y", &mut y), ("X", &mut x)] {
            ui.label(RichText::new(axis).size(11.0).color(MUTED));
            egui::ComboBox::from_id_salt(("scatter", i, axis)).width(w).selected_text(RichText::new(metric_label(*value, u)).size(11.5)).show_ui(ui, |ui| {
                for m in Metric::ALL {
                    ui.selectable_value(value, m, metric_label(m, u));
                }
            });
        }
    });
    if (x, y) != (tile.x, tile.y) {
        edits.push(Edit::Axes(i, x, y));
    }
    ui.add_space(4.0);
    let rect = ui.available_rect_before_wrap();
    let values: Vec<Option<(f64, f64)>> = ctx
        .shown
        .iter()
        .zip(ctx.flights)
        .map(|(s, f)| Some((metric_value(x, s, f, u)?, metric_value(y, s, f, u)?)))
        .collect();
    let range = |pick: fn(&(f64, f64)) -> f64| {
        let (lo, hi) = values.iter().flatten().map(pick).fold((f64::MAX, f64::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)));
        if lo > hi {
            (0.0, 1.0)
        } else if hi - lo < 1e-9 {
            (lo - 1.0, hi + 1.0)
        } else {
            let pad = (hi - lo) * 0.08;
            (lo - pad, hi + pad)
        }
    };
    let ((x0, x1), (y0, y1)) = (range(|v| v.0), range(|v| v.1));
    let pos = |vx: f64, vy: f64| {
        pos2(
            rect.left() + ((vx - x0) / (x1 - x0)) as f32 * rect.width(),
            rect.bottom() - ((vy - y0) / (y1 - y0)) as f32 * rect.height(),
        )
    };
    let p = ui.painter_at(rect);
    p.rect(rect, 4.0, PAGE, Stroke::new(1.0, LINE), StrokeKind::Inside);
    // Zero lines split the chart into quadrants where both signs occur.
    if x0 < 0.0 && x1 > 0.0 {
        let zx = pos(0.0, y0).x;
        p.line_segment([pos2(zx, rect.top()), pos2(zx, rect.bottom())], Stroke::new(1.0, FIELD_LINE));
    }
    if y0 < 0.0 && y1 > 0.0 {
        let zy = pos(x0, 0.0).y;
        p.line_segment([pos2(rect.left(), zy), pos2(rect.right(), zy)], Stroke::new(1.0, FIELD_LINE));
    }
    let (dx, dy) = (metric_digits(x), metric_digits(y));
    let small = FontId::proportional(10.5);
    p.text(rect.left_bottom() + vec2(4.0, -2.0), Align2::LEFT_BOTTOM, num(x0, dx), small.clone(), MUTED);
    p.text(rect.right_bottom() + vec2(-4.0, -2.0), Align2::RIGHT_BOTTOM, num(x1, dx), small.clone(), MUTED);
    p.text(rect.left_top() + vec2(4.0, 2.0), Align2::LEFT_TOP, num(y1, dy), small, MUTED);
    if values.iter().all(Option::is_none) {
        p.text(rect.center(), Align2::CENTER_CENTER, tr("No shots with these values"), FontId::proportional(12.0), MUTED);
        return None;
    }
    draw_spread(&p, &club_groups(ctx, |n| values[n].map(|(vx, vy)| pos(vx, vy))));
    let dots: Vec<Dot> = ctx
        .shown
        .iter()
        .zip(&values)
        .filter_map(|(s, v)| {
            let (vx, vy) = (*v)?;
            Some(Dot {
                pos: pos(vx, vy),
                color: (ctx.color_of)(&s.club_code),
                selected: ctx.selected.contains(&key(s)),
                excluded: s.excluded,
                key: key(s),
                tip: format!(
                    "{} · {}\n{}: {}\n{}: {}",
                    s.clock(),
                    s.club_code.as_deref().unwrap_or("–"),
                    metric_label(x, u),
                    num(vx, dx),
                    metric_label(y, u),
                    num(vy, dy),
                ),
            })
        })
        .collect();
    draw_dots(ui, egui::Id::new(("scatter", i)), rect, &dots)
}

/// The shots, latest first; wide tiles show every column. Returns the shot clicked.
fn shots_table(ui: &mut egui::Ui, i: usize, ctx: &TileCtx) -> Option<Key> {
    let u = ctx.units;
    let carry = format!("{}*", tr("Carry"));
    let total = format!("{}*", tr("Total"));
    // Header, width of the values and priority (lower first) of every column.
    let all: [(&str, f32, u8); 13] = [
        (tr("Time"), if ctx.all_time { 84.0 } else { 52.0 }, 0),
        (tr("Club"), 24.0, 0),
        (&carry, 28.0, 0),
        (&total, 28.0, 3),
        (tr("Ball"), 36.0, 1),
        ("VLA", 30.0, 1),
        ("HLA", 30.0, 4),
        (tr("Spin"), 34.0, 1),
        (tr("Axis"), 30.0, 5),
        (tr("Club"), 36.0, 6),
        (tr("Smash"), 30.0, 2),
        (tr("Face to Path"), 40.0, 7),
        (tr("Tempo"), 24.0, 8),
    ];
    let keep = fit_columns(ui, &all);
    let cols: Vec<(&str, f32)> = keep.iter().map(|&k| (all[k].0, all[k].1)).collect();
    let shown: Vec<&ShotRecord> = ctx.shown.iter().rev().copied().collect();
    let row = |s: &ShotRecord| {
        let f: Flight = flight::fly(&s.ball);
        let c = s.club.as_ref();
        let time = if ctx.all_time { s.time.get(5..16).unwrap_or(&s.time).replace('T', " ") } else { s.clock().to_string() };
        let cells = [
            time,
            s.club_code.clone().unwrap_or_else(|| "–".into()),
            format!("{:.0}", u.dist(f.carry)),
            format!("{:.0}", u.dist(f.total)),
            format!("{:.1}", u.speed(s.ball.speed)),
            num(s.ball.vla, 1),
            num(s.ball.hla, 1),
            format!("{:.0}", s.ball.total_spin),
            num(s.ball.spin_axis, 1),
            c.map_or("–".into(), |c| format!("{:.1}", u.speed(c.speed))),
            s.smash().map_or("–".into(), |v| format!("{v:.2}")),
            c.map_or("–".into(), |c| dir(c.face_to_target - c.path, "O", "C")),
            s.tempo.map_or("–".into(), |t| format!("{:.1}", t.ratio())),
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
        ui.label(RichText::new(tr("No shots yet.")).color(MUTED));
        return None;
    }
    table(ui, egui::Id::new(("shots-table", i)), &cols, shown.len(), |r| row(shown[r])).map(|r| key(shown[r]))
}

/// Mean ± spread per club of the shots shown (excluded shots left out).
fn averages_table(ui: &mut egui::Ui, i: usize, ctx: &TileCtx) {
    let u = ctx.units;
    let shots: Vec<ShotRecord> = ctx.shown.iter().map(|s| (*s).clone()).collect();
    let stats = history::club_stats(&shots);
    if stats.is_empty() {
        ui.label(RichText::new(tr("No shots yet.")).color(MUTED));
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
    let ball = format!("{} ({})", tr("Ball"), u.speed_unit());
    let club = format!("{} ({})", tr("Club"), u.speed_unit());
    let all: [(&str, f32, u8); 12] = [
        (tr("Club"), 90.0, 0),
        (tr("Shots"), 20.0, 0),
        (&carry, 50.0, 0),
        (&total, 50.0, 2),
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

/// All values of one shot: the one clicked last, or the latest.
fn details(ui: &mut egui::Ui, shot: Option<&ShotRecord>, ctx: &TileCtx) {
    let Some(s) = shot else {
        ui.label(RichText::new(tr("No shots yet.")).color(MUTED));
        return;
    };
    let u = ctx.units;
    let f = flight::fly(&s.ball);
    let c = s.club.as_ref();
    let deg = |v: f64| format!("{}°", num(v, 1));
    let side = |v: f64| {
        let d = u.dist(v);
        if d.abs() < 0.5 { format!("0 {}", u.dist_unit()) } else { format!("{:.0} {} {}", d.abs(), u.dist_unit(), if d > 0.0 { "R" } else { "L" }) }
    };
    let mut values: Vec<(String, String)> = vec![
        (format!("{}*", tr("Carry")), format!("{:.0} {}", u.dist(f.carry), u.dist_unit())),
        (format!("{}*", tr("Total")), format!("{:.0} {}", u.dist(f.total), u.dist_unit())),
        (format!("{}*", tr("Side")), side(f.total_side)),
        (format!("{}*", tr("Apex")), format!("{:.0} {}", u.dist(f.apex), u.dist_unit())),
        (format!("{}*", tr("Descent")), format!("{:.0}°", f.descent)),
        (tr("Ball Speed").into(), format!("{:.1} {}", u.speed(s.ball.speed), u.speed_unit())),
        ("VLA".into(), deg(s.ball.vla)),
        ("HLA".into(), deg(s.ball.hla)),
        (tr("Total Spin").into(), format!("{:.0} rpm", s.ball.total_spin)),
        (tr("Spin Axis").into(), deg(s.ball.spin_axis)),
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
            ui.label(RichText::new(tr("excluded")).size(12.0).color(AMBER));
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
mod tests {
    use super::*;

    /// Renders the explorer in a few states (averages page, a selection, arranging):
    /// `OPENLAUNCH_SCREENSHOT_DIR=<dir> cargo test --bin openlaunch-connect explorer_states -- --ignored`
    #[test]
    #[ignore = "writes screenshots; run on demand"]
    fn explorer_states() {
        use egui_kittest::Harness;

        let language = if std::env::var("OPENLAUNCH_SCREENSHOT_LANG").as_deref() == Ok("de") { Language::German } else { Language::English };
        i18n::set(language);
        super::super::tests::demo_state();
        let keys: Vec<Key> = state::with(|s| s.session.iter().rev().take(2).map(key).collect());
        state::with(|s| s.session.iter_mut().rev().nth(1).unwrap().excluded = true);
        type Setup = fn(&mut App, &[Key]);
        let states: [(&str, Setup); 5] = [
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
                let tile = |kind, size| Tile { kind, size, ..Tile::default() };
                app.saved.explorer.pages[0].tiles = vec![
                    tile(TileKind::Dispersion, TileSize::W2H2),
                    tile(TileKind::Scatter, TileSize::W2H2),
                    Tile { total: true, ..tile(TileKind::Dispersion, TileSize::W1H2) },
                    Tile { x: Metric::BallSpeed, y: Metric::Carry, ..tile(TileKind::Scatter, TileSize::W1H1) },
                    tile(TileKind::Details, TileSize::W1H1),
                ];
                app.explorer.selected = keys.iter().take(1).cloned().collect();
            }),
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

        // Moving a tile saves the layout at once – not into the real settings file.
        openlaunch_connect::settings::SAVE.store(false, std::sync::atomic::Ordering::Relaxed);
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
        // Default page: table 4×2 on the left, details 2×2 on the right (six columns).
        let (from, to) = (pos2(250.0, 300.0), pos2(850.0, 300.0));
        let events = [
            egui::Event::PointerMoved(from),
            egui::Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() },
            egui::Event::PointerMoved(from + vec2(20.0, 0.0)),
            egui::Event::PointerMoved(to),
            egui::Event::PointerButton { pos: to, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() },
        ];
        for event in events {
            harness.input_mut().events.push(event);
            harness.step();
        }
        harness.step();
        let kinds: Vec<TileKind> = harness.state().saved.explorer.pages[0].tiles.iter().map(|t| t.kind).collect();
        assert_eq!(kinds, [TileKind::Details, TileKind::Table]);
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
    fn clubs_sort_in_bag_order() {
        let mut codes = vec![Some("PW"), None, Some("I7"), Some("H4"), Some("DR"), Some("LW"), Some("W3"), Some("I10"), Some("GW"), Some("PT")];
        codes.sort_by_key(|c| club_rank(*c));
        assert_eq!(codes, [Some("DR"), Some("W3"), Some("H4"), Some("I7"), Some("I10"), Some("PW"), Some("GW"), Some("LW"), Some("PT"), None]);
    }

    #[test]
    fn correlation_of_two_series() {
        let up: Vec<(f64, f64)> = (0..5).map(|k| (f64::from(k), 2.0 * f64::from(k) + 1.0)).collect();
        assert!((pearson(&up).unwrap() - 1.0).abs() < 1e-9);
        let down: Vec<(f64, f64)> = up.iter().map(|&(x, y)| (x, -y)).collect();
        assert!((pearson(&down).unwrap() + 1.0).abs() < 1e-9);
        assert_eq!(pearson(&up[..2]), None, "too few shots");
        assert_eq!(pearson(&[(1.0, 2.0), (1.0, 3.0), (1.0, 4.0)]), None, "no spread");
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
