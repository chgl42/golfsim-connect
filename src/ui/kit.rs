//! Building blocks of the app's look (GSPro's in-game style): the palette, the condensed font,
//! and controls drawn with the painter. Pages added to the app use these to look the same.
//!
//! The controls are sized for the app's 34 px rows. Texts are passed in as shown; translate
//! them with [`crate::i18n::tr`] first.

use std::sync::Arc;

use crate::i18n::tr;

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontFamily, FontId, Rect, RichText, Sense, Stroke, StrokeKind, pos2, vec2,
};

// GSPro in-game palette: deep navy chrome, slightly lighter stat tiles, blue accent.
/// Page background.
pub const PAGE: Color32 = Color32::from_rgb(0x0b, 0x12, 0x20);
/// Top bar and windows.
pub const BAR: Color32 = Color32::from_rgb(0x10, 0x1a, 0x2d);
/// Tiles and cards.
pub const TILE: Color32 = Color32::from_rgb(0x16, 0x21, 0x3a);
/// A tile under the mouse.
pub const TILE_HOVER: Color32 = Color32::from_rgb(0x1d, 0x2b, 0x4a);
/// Borders and dividers.
pub const LINE: Color32 = Color32::from_rgb(0x22, 0x31, 0x4f);
/// Figures and headings.
pub const WHITE: Color32 = Color32::from_rgb(0xff, 0xff, 0xff);
/// Body text.
pub const SOFT: Color32 = Color32::from_rgb(0xd6, 0xde, 0xee);
/// Secondary text, units, empty values.
pub const MUTED: Color32 = Color32::from_rgb(0x7f, 0x8f, 0xb0);
/// Blue accent: chosen segments, primary buttons, card icons.
pub const ACCENT: Color32 = Color32::from_rgb(0x3d, 0x6f, 0xe6);
/// Ready, good.
pub const GREEN: Color32 = Color32::from_rgb(0x3d, 0xdc, 0x6a);
/// Waiting, a warning.
pub const AMBER: Color32 = Color32::from_rgb(0xf2, 0xb3, 0x3d);
/// An error, missing.
pub const RED: Color32 = Color32::from_rgb(0xef, 0x5b, 0x5b);
// Controls: raised buttons, dark input fields, row labels.
/// A raised button.
pub const BUTTON: Color32 = Color32::from_rgb(0x22, 0x33, 0x5a);
/// A raised button under the mouse.
pub const BUTTON_HOVER: Color32 = Color32::from_rgb(0x2a, 0x3f, 0x6c);
/// A raised button's border.
pub const BUTTON_LINE: Color32 = Color32::from_rgb(0x3d, 0x55, 0x85);
/// A primary button under the mouse.
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0x52, 0x81, 0xf0);
/// Icons on secondary buttons, links.
pub const ICON: Color32 = Color32::from_rgb(0x7f, 0xa6, 0xff);
/// Input fields' borders.
pub const FIELD_LINE: Color32 = Color32::from_rgb(0x2b, 0x3d, 0x63);
/// Row labels and card headings.
pub const LABEL: Color32 = Color32::from_rgb(0x9f, 0xb0, 0xd0);

/// The condensed bold font (DIN Condensed) at `size`, for figures and headings; set up by
/// [`setup_style`]. Draw it in boxes with [`caps_text`].
#[must_use]
pub fn cond(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("cond".into()))
}

/// DIN Condensed's capitals sit about a quarter of the font size above the middle of the
/// text box (measured in the rendered screenshots).
pub(crate) const CAPS_OFFSET: f32 = 0.25;

/// Draw condensed `text` with its capitals vertically centred on `pos.y`
/// (`h`: horizontal alignment). Use this whenever a figure or label sits in a box.
pub fn caps_text(p: &egui::Painter, pos: egui::Pos2, h: egui::Align, text: impl ToString, size: f32, color: Color32) -> Rect {
    p.text(pos + vec2(0.0, size * CAPS_OFFSET), Align2([h, egui::Align::Center]), text, cond(size), color)
}

fn load_first(paths: &[&str]) -> Option<Vec<u8>> {
    paths.iter().find_map(|p| std::fs::read(p).ok())
}

/// Fonts (body text and [`cond`]) and visuals of the app. The app sets them up on start; tests
/// that draw a page offscreen call this first (fonts set in one frame apply from the next).
pub fn setup_style(ctx: &egui::Context) {
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

/// Width of the label column on the settings page.
const SETTING_LABEL_W: f32 = 92.0;

/// A settings card: tile with border, icon + condensed heading, divider; `add` fills the body.
pub fn card(ui: &mut egui::Ui, rect: Rect, title: &str, icon: Icon, add: impl FnOnce(&mut egui::Ui)) {
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
pub(crate) fn setting_label(ui: &mut egui::Ui, label: &str, h: f32) {
    let (r, _) = ui.allocate_exact_size(vec2(SETTING_LABEL_W, h), Sense::hover());
    ui.painter().text(r.left_center(), Align2::LEFT_CENTER, label, FontId::proportional(13.0), LABEL);
}

/// A row in a card: `label` in a fixed-width column, then what `add` puts there (left to right).
pub fn setting_row(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    // Its own id, so equal options in two rows (e.g. "Mittel") don't clash.
    ui.push_id(label, |ui| {
        ui.horizontal(|ui| {
            ui.set_min_height(34.0);
            setting_label(ui, label, 34.0);
            add(ui);
        });
    });
}

/// Secondary button inside a card (34 px, raised like the main buttons), greyed out unless
/// `enabled`.
pub fn secondary_button(ui: &mut egui::Ui, label: &str, enabled: bool) -> egui::Response {
    let button = egui::Button::new(RichText::new(label).size(13.0).strong().color(SOFT)).min_size(vec2(0.0, 34.0));
    ui.add_enabled(enabled, button)
}

/// Read-only value in an input-like field (e.g. the launcher path), truncated with a tooltip.
pub(crate) fn read_only_field(ui: &mut egui::Ui, width: f32, text: &str) {
    let (r, resp) = ui.allocate_exact_size(vec2(width, 34.0), Sense::hover());
    ui.painter().rect(r, 5.0, PAGE, Stroke::new(1.0, FIELD_LINE), StrokeKind::Inside);
    let inner = r.shrink2(vec2(10.0, 0.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
        ui.add(egui::Label::new(RichText::new(text).size(13.0).color(SOFT)).truncate());
    });
    resp.on_hover_text(text);
}

/// Switch with a label to its right; clicking either toggles. Returns the response (`changed()`).
pub fn toggle(ui: &mut egui::Ui, on: &mut bool, label: &str) -> egui::Response {
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

/// Equal segments, one per option (e.g. `&[(true, "km/h"), (false, "mph")]`); the chosen one is
/// filled. Returns true when it changed.
pub fn segments<T: PartialEq + Copy>(ui: &mut egui::Ui, value: &mut T, options: &[(T, &str)]) -> bool {
    segments_where(ui, value, options, |_| true, "")
}

/// Like [`segments`]; options that aren't `enabled` are greyed out and explain why on hover.
pub fn segments_where<T: PartialEq + Copy>(
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
pub fn stepper(ui: &mut egui::Ui, value: &mut f32, range: std::ops::RangeInclusive<f32>, step: f32, unit: &str) -> egui::Response {
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
pub enum Icon {
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
pub fn button(ui: &mut egui::Ui, rect: Rect, label: &str, icon: Icon, primary: bool, enabled: bool, disabled_hint: &str) -> bool {
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
    let x0 = r.center().x - f32::midpoint(icon_w, text_w);
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

/// A modal dialog in the app's look, `width` wide; `add` fills it. Escape or a click outside
/// closes it, as does `ui.close()` (see `should_close()`).
pub(crate) fn dialog<R>(ctx: &egui::Context, id: &str, width: f32, add: impl FnOnce(&mut egui::Ui) -> R) -> egui::ModalResponse<R> {
    egui::Modal::new(egui::Id::new(id))
        .frame(egui::Frame::new().fill(TILE).stroke(Stroke::new(1.0, BUTTON_LINE)).corner_radius(8).inner_margin(20))
        .show(ctx, |ui| {
            ui.set_width(width);
            add(ui)
        })
}

/// Asks before something that can't be undone: `title`, `text` below it, *Cancel* and the
/// primary button `ok`. Show it each frame while asking (`id` tells dialogs apart). Returns
/// `Some(true)` once `ok` is clicked, `Some(false)` once it is cancelled (also with Escape or a
/// click outside), else `None`.
#[must_use]
pub fn confirm(ctx: &egui::Context, id: &str, title: &str, text: &str, ok: &str) -> Option<bool> {
    let mut confirmed = false;
    let modal = dialog(ctx, id, 360.0, |ui| {
        ui.label(RichText::new(title).size(16.0).color(WHITE));
        ui.add_space(6.0);
        ui.label(RichText::new(text).size(13.0).color(SOFT));
        ui.add_space(16.0);
        let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover());
        let ok_r = Rect::from_min_max(pos2(row.right() - 130.0, row.top()), row.max);
        let cancel_r = Rect::from_min_max(pos2(ok_r.left() - 140.0, row.top()), pos2(ok_r.left() - 10.0, row.bottom()));
        if button(ui, cancel_r, tr("CANCEL"), Icon::None, false, true, "") {
            ui.close();
        }
        if button(ui, ok_r, ok, Icon::None, true, true, "") {
            confirmed = true;
            ui.close();
        }
    });
    modal.should_close().then_some(confirmed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use std::cell::RefCell;
    use std::rc::Rc;

    type Answers = Rc<RefCell<Vec<Option<bool>>>>;

    /// A harness asking every frame, with the answers of each frame.
    fn asking() -> (Harness<'static>, Answers) {
        let answers = Answers::default();
        let out = answers.clone();
        let mut harness = Harness::builder().with_size(vec2(600.0, 300.0)).build_ui(move |ui| {
            // Fonts set in one frame apply from the next.
            if ui.ctx().fonts(|f| f.families().contains(&FontFamily::Name("cond".into()))) {
                out.borrow_mut().push(confirm(ui.ctx(), "test", "Delete it?", "It goes for good.", "DELETE"));
            } else {
                setup_style(ui.ctx());
            }
        });
        harness.run_steps(3);
        (harness, answers)
    }

    /// The dialog's buttons (Cancel, OK), left to right.
    fn buttons(harness: &Harness) -> Vec<Rect> {
        let mut rects: Vec<Rect> = harness.ctx.viewport(|v| {
            v.prev_pass.widgets.layers().flat_map(|(_, w)| w).filter(|w| w.sense.senses_click() && (w.rect.height() - 34.0).abs() < 0.5).map(|w| w.rect).collect()
        });
        rects.sort_by(|a, b| a.left().total_cmp(&b.left()));
        rects
    }

    fn click(harness: &mut Harness, pos: egui::Pos2) {
        let press = |pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        for event in [egui::Event::PointerMoved(pos), press(true), press(false)] {
            harness.input_mut().events.push(event);
            harness.step();
        }
    }

    /// No answer while it is open; then one: OK, or cancelled by the button or Escape.
    #[test]
    fn confirm_answers_once_when_closed() {
        for (how, want) in [("ok", true), ("cancel", false), ("escape", false)] {
            let (mut harness, answers) = asking();
            assert!(answers.borrow().iter().all(Option::is_none), "{how}: no answer while open");
            let buttons = buttons(&harness);
            assert_eq!(buttons.len(), 2, "{how}: Cancel and OK");
            match how {
                "ok" => click(&mut harness, buttons[1].center()),
                "cancel" => click(&mut harness, buttons[0].center()),
                _ => {
                    harness.key_press(egui::Key::Escape);
                    harness.step();
                }
            }
            let given: Vec<bool> = answers.borrow().iter().filter_map(|a| *a).collect();
            assert_eq!(given, [want], "{how}");
        }
    }
}
