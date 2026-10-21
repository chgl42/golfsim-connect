//! SHOTS page: the shots explorer. Pages of tiles (table, details) on a grid of near-square
//! cells; the filter and the selection are shared by all tiles.

use std::collections::BTreeSet;

use openlaunch_connect::flight::{self, Flight};
use openlaunch_connect::history::Stat;
use openlaunch_connect::settings::{ExplorerPage, Tile, TileKind, TileSize};

use super::*;

/// Gap between tiles.
const GAP: f32 = 8.0;
/// Cell size the grid aims for; the rows are fitted to the height.
const CELL_TARGET: f32 = 170.0;
const ROW_H: f32 = 21.0;
/// Club colours, in the order the clubs were first used.
const CLUB_COLORS: [Color32; 6] = [
    AMBER,
    ICON,
    Color32::from_rgb(0x4f, 0xd1, 0xc5),
    Color32::from_rgb(0xe0, 0x7b, 0xd8),
    GREEN,
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

/// Changes to the layout made while drawing, applied afterwards.
enum Edit {
    Kind(usize, TileKind),
    Size(usize, TileSize),
    Averages(usize, bool),
    Move(usize, isize),
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
        for (i, page) in self.saved.explorer.pages.iter().enumerate() {
            if page_tab(&mut bar_ui, &page_name(page, i), i == self.explorer.page) {
                self.explorer.page = i;
            }
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
            let (rect, _) = ui.allocate_exact_size(vec2(110.0, 30.0), Sense::hover());
            let label = if self.explorer.arrange { tr("DONE") } else { tr("ARRANGE") };
            if button(ui, rect, label, Icon::None, self.explorer.arrange, true, "") {
                self.explorer.arrange = !self.explorer.arrange;
            }
            if let Some(note) = &self.export_note {
                ui.label(RichText::new(note).size(12.0).color(MUTED));
            }
        });
        ui.add_space(6.0);

        // Clubs in the order they were first used: their colours stay put while filtering.
        let mut codes: Vec<Option<String>> = Vec::new();
        for s in &shots {
            if !codes.contains(&s.club_code) {
                codes.push(s.club_code.clone());
            }
        }
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

        // Second row: the filter and what to do with the selection, or the page while arranging.
        let mut exclude: Option<bool> = None;
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
        let mut row = ui.new_child(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::left_to_right(egui::Align::Center)));
        row.spacing_mut().item_spacing.x = 6.0;
        if ex.arrange {
            let pages = &mut self.saved.explorer.pages;
            row.label(RichText::new(tr("Name")).size(13.0).color(LABEL));
            let name = &mut pages[ex.page].name;
            let hint = tr("Page {n}").replace("{n}", &(ex.page + 1).to_string());
            if row.add(egui::TextEdit::singleline(name).hint_text(hint).desired_width(140.0)).changed() {
                layout_changed = true;
            }
            row.add_space(10.0);
            for kind in TileKind::ALL {
                if chip(&mut row, &format!("+ {}", kind_name(kind)), None, false).clicked() {
                    pages[ex.page].tiles.push(Tile { kind, ..Tile::default() });
                    layout_changed = true;
                }
            }
            if pages.len() > 1 {
                row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if link(ui, tr("Remove page"), RED).clicked() {
                        pages.remove(ex.page);
                        ex.page = ex.page.min(pages.len() - 1);
                        layout_changed = true;
                    }
                });
            }
        } else {
            row.label(RichText::new(tr("Filter")).size(13.0).color(LABEL));
            row.add_space(2.0);
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
        let spots = place(&page_tiles.iter().map(|t| t.size).collect::<Vec<_>>(), cols, rows);
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
            if page_tiles.is_empty() {
                ui.painter().text(
                    area.center(),
                    Align2::CENTER_CENTER,
                    tr("No tiles on this page – add some with ARRANGE."),
                    FontId::proportional(13.0),
                    MUTED,
                );
            }
            for (i, (tile, &(c, r, w, h))) in page_tiles.iter().zip(&spots).enumerate() {
                let min = grid_rect.min + vec2(c as f32 * (cell.x + GAP), r as f32 * (cell.y + GAP));
                let size = vec2(w as f32 * cell.x + (w - 1) as f32 * GAP, h as f32 * cell.y + (h - 1) as f32 * GAP);
                let rect = Rect::from_min_size(min, size);
                let ctx = TileCtx { shown: &shown, selected: &ex.selected, color_of: &color_of, units, all_time: self.all_time };
                tile_ui(ui, rect, i, tile, ex.arrange, page_tiles.len(), &ctx, focus, &mut edits, &mut clicked);
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

/// What every tile draws from.
struct TileCtx<'a> {
    shown: &'a [&'a ShotRecord],
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
        head_ui.label(RichText::new(kind_name(tile.kind).to_uppercase()).font(cond(14.0)).color(LABEL).extra_letter_spacing(1.2));
        if tile.kind == TileKind::Table {
            head_ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                for (on, label) in [(true, tr("Averages")), (false, tr("Shots"))] {
                    let color = if tile.averages == on { WHITE } else { MUTED };
                    if link(ui, label, color).clicked() && tile.averages != on {
                        edits.push(Edit::Averages(i, on));
                    }
                }
            });
        }
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
    }
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
    let columns = if ui.available_width() >= 360.0 { 2 } else { 1 };
    let per = values.len().div_ceil(columns);
    egui::ScrollArea::vertical().id_salt(("details", s.shot_number)).auto_shrink([false, false]).show(ui, |ui| {
        let width = ui.available_width();
        let col_w = (width - (columns - 1) as f32 * 18.0) / columns as f32;
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
fn page_tab(ui: &mut egui::Ui, name: &str, active: bool) -> bool {
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
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
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
        let states: [(&str, Setup); 3] = [
            ("explorer-averages.png", |app, _| app.explorer.page = 2),
            ("explorer-selection.png", |app, keys| {
                app.explorer.selected = keys.iter().cloned().collect();
                app.explorer.focus = keys.last().cloned();
            }),
            ("explorer-arrange.png", |app, _| app.explorer.arrange = true),
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
