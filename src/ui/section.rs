//! Settings of other crates on the *Settings* page: what a section implements
//! ([`SettingsSection`]), where its cards go and how SAVE reaches them (ADR-018).

use eframe::egui::{self, Rect, Sense, pos2, vec2};

use crate::settings::Settings;

use super::kit::{DISABLED, Icon, card, with_badge};

/// A card of another crate's settings on the *Settings* page, after the core's cards. A section
/// keeps its own state in its fields (e.g. a copy of its settings being edited) and reads and
/// writes only its own file: it is given the core's settings to read, never to change, and the
/// core never writes a section's data. The core's SAVE saves it with the rest (see [`save`]).
///
/// Every method is asked every frame, so a change (e.g. no longer greyed) shows at once.
/// Another crate hands its sections to [`run_with_sections`](super::run_with_sections).
///
/// [`save`]: SettingsSection::save
pub trait SettingsSection {
    /// Unique, lower case, e.g. `"example"`; keeps the card's controls apart from other cards'.
    fn id(&self) -> &'static str;
    /// The card's heading, in capitals and translated with `tr` (a table of its own crate goes in
    /// with `i18n::register`).
    fn title(&self) -> &'static str;
    /// The icon before the heading.
    fn icon(&self) -> Icon;
    /// Draws the card's body (rows with [`setting_row`](super::kit::setting_row) like the core's
    /// cards). `settings` are the core's settings in use, to read only.
    fn ui(&mut self, ui: &mut egui::Ui, settings: &Settings);
    /// Whether the section has unsaved changes: SAVE is enabled and "Unsaved changes" shows while
    /// it has. False by default.
    fn dirty(&self) -> bool {
        false
    }
    /// Writes the section's unsaved changes to its own file; called by SAVE while it is
    /// [`dirty`](SettingsSection::dirty). `Err(why)` shows as "Saving failed: why" next to SAVE
    /// (translated by the section); the edits stay, so SAVE can be pressed again. Does nothing
    /// by default.
    ///
    /// # Errors
    /// Why the section's changes could not be saved.
    fn save(&mut self) -> Result<(), String> {
        Ok(())
    }
    /// The card's height in points, heading included; 140 (two rows) by default. Two cards side by
    /// side share the taller one's height.
    fn height(&self) -> f32 {
        140.0
    }
    /// A mark drawn after the heading, one space apart and in its colour (e.g. `"★"`), translated
    /// with `tr`; the heading's accessible label is then `"<title> <badge>"`. None by default
    /// (as [`Page::badge`](super::Page::badge), ADR-016).
    fn badge(&self) -> Option<&'static str> {
        None
    }
    /// `Some(why)` draws the card greyed (the kit's [`DISABLED`] look) with `why` as the tooltip of
    /// its heading, translated with `tr`. Its controls stay usable; what they do is the section's.
    /// None by default (as [`Page::greyed`](super::Page::greyed), ADR-016).
    fn greyed(&self) -> Option<&'static str> {
        None
    }
}

/// Room between cards, across and down.
pub(super) const GAP: f32 = 10.0;

/// Where the cards of sections with these `heights` go below the core's cards: from `origin`
/// (the left of the first row, `GAP` below the core's cards), two to a row in the order given,
/// each `width` wide (two columns with `GAP` between them), a row as tall as its taller card.
pub(super) fn layout(heights: &[f32], origin: egui::Pos2, width: f32) -> Vec<Rect> {
    let col_w = (width - GAP) / 2.0;
    let mut top = origin.y;
    let mut rects = Vec::with_capacity(heights.len());
    for row in heights.chunks(2) {
        let h = row.iter().copied().fold(0.0, f32::max);
        for col in 0..row.len() {
            rects.push(Rect::from_min_size(pos2(origin.x + col as f32 * (col_w + GAP), top), vec2(col_w, h)));
        }
        top += h + GAP;
    }
    rects
}

/// The room the sections' cards take below the core's cards, the gap above each row included.
pub(super) fn total_height(sections: &[Box<dyn SettingsSection>]) -> f32 {
    let heights: Vec<f32> = sections.iter().map(|s| s.height()).collect();
    layout(&heights, pos2(0.0, 0.0), 2.0 * GAP + 2.0).last().map_or(0.0, |r| r.bottom() + GAP)
}

/// Draws the sections' cards, the first row from `origin` (`GAP` below the core's cards),
/// `width` wide.
pub(super) fn draw(ui: &mut egui::Ui, sections: &mut [Box<dyn SettingsSection>], origin: egui::Pos2, width: f32, settings: &Settings) {
    let heights: Vec<f32> = sections.iter().map(|s| s.height()).collect();
    let rects = layout(&heights, pos2(origin.x, origin.y + GAP), width);
    for (section, rect) in sections.iter_mut().zip(rects) {
        let title = with_badge(section.title(), section.badge());
        let greyed = section.greyed();
        ui.push_id(section.id(), |ui| {
            if greyed.is_some() {
                ui.multiply_opacity(DISABLED);
            }
            card(ui, rect, &title, section.icon(), |ui| section.ui(ui, settings));
            // The heading row, for its accessible label and the greyed card's reason.
            let head = Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + 38.0));
            let resp = ui.interact(head, ui.id().with("heading"), Sense::hover());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &title));
            if let Some(why) = greyed {
                resp.on_hover_text(why);
            }
        });
    }
}

/// Whether any section has unsaved changes.
pub(super) fn any_dirty(sections: &[Box<dyn SettingsSection>]) -> bool {
    sections.iter().any(|s| s.dirty())
}

/// Saves every section with unsaved changes, in the order given, all of them even after one
/// failed; returns the first failure.
pub(super) fn save_all(sections: &mut [Box<dyn SettingsSection>]) -> Option<String> {
    let mut first = None;
    for section in sections.iter_mut().filter(|s| s.dirty()) {
        if let Err(e) = section.save() {
            crate::log::info(&format!("Could not save the settings of \"{}\": {e}", section.id()));
            first.get_or_insert(e);
        }
    }
    first
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{App, BASE_SIZE, setup_style};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    /// What a test section records and is told: unsaved, how its save goes, greyed.
    #[derive(Default)]
    struct Probe {
        dirty: Cell<bool>,
        saves: Cell<u32>,
        fail: RefCell<Option<String>>,
        greyed: Cell<Option<&'static str>>,
        drawn: Cell<u32>,
    }

    /// A section as another crate adds it.
    struct TestSection {
        id: &'static str,
        title: &'static str,
        height: f32,
        badge: Option<&'static str>,
        probe: Rc<Probe>,
    }

    impl TestSection {
        fn new(id: &'static str, title: &'static str, height: f32) -> (Self, Rc<Probe>) {
            let probe = Rc::new(Probe::default());
            (Self { id, title, height, badge: None, probe: probe.clone() }, probe)
        }
    }

    impl SettingsSection for TestSection {
        fn id(&self) -> &'static str {
            self.id
        }

        fn title(&self) -> &'static str {
            self.title
        }

        fn icon(&self) -> Icon {
            Icon::Bell
        }

        fn ui(&mut self, ui: &mut egui::Ui, settings: &Settings) {
            self.probe.drawn.set(self.probe.drawn.get() + 1);
            ui.label(format!("{} body{}", self.id, if settings.metric { "" } else { " (imperial)" }));
            if ui.button(format!("{} edit", self.id)).clicked() {
                self.probe.dirty.set(true);
            }
        }

        fn dirty(&self) -> bool {
            self.probe.dirty.get()
        }

        fn save(&mut self) -> Result<(), String> {
            self.probe.saves.set(self.probe.saves.get() + 1);
            match self.probe.fail.borrow().clone() {
                Some(e) => Err(e),
                None => {
                    self.probe.dirty.set(false);
                    Ok(())
                }
            }
        }

        fn height(&self) -> f32 {
            self.height
        }

        fn badge(&self) -> Option<&'static str> {
            self.badge
        }

        fn greyed(&self) -> Option<&'static str> {
            self.probe.greyed.get()
        }
    }

    /// The colour the glyphs of `text` are painted in: a painter's opacity (`Ui::multiply_opacity`)
    /// dims the glyphs' vertices, not the text's format.
    fn glyph_color(output: &egui::FullOutput, text: &str) -> Option<egui::Color32> {
        fn find(shape: &egui::Shape, text: &str) -> Option<egui::Color32> {
            match shape {
                egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| find(s, text)),
                egui::Shape::Text(t) if t.galley.text() == text => {
                    t.galley.rows.iter().find_map(|r| r.row.visuals.mesh.vertices.first()).map(|v| v.color)
                }
                _ => None,
            }
        }
        output.shapes.iter().find_map(|c| find(&c.shape, text))
    }

    /// Tall enough to show every card without scrolling, so the tests can click them.
    const TALL: egui::Vec2 = vec2(1000.0, 900.0);

    /// The app on the Settings page with these sections, in a window of `size`.
    fn harness(size: egui::Vec2, sections: Vec<Box<dyn SettingsSection>>) -> egui_kittest::Harness<'static, App> {
        crate::settings::SAVE.store(false, std::sync::atomic::Ordering::Relaxed);
        crate::i18n::set(&crate::i18n::Language::new("en"));
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut harness = egui_kittest::Harness::builder().with_size(size).build_eframe(|cc| {
            setup_style(&cc.egui_ctx);
            cc.egui_ctx.all_styles_mut(|s| s.interaction.tooltip_delay = 0.0);
            // A fixed interface size gives the page the room of a larger window (no zoom).
            let ui_scale = (size != BASE_SIZE).then_some(1.0);
            App::with_pages(Settings {
                overlay: crate::settings::OverlayPos::Off,
                ui_scale,
                language: crate::i18n::Language::new("en"),
                // A launcher of its own, so the app doesn't put one it finds into the copy.
                launcher_path: Some("/Applications/GSPro.app".into()),
                ..Settings::default()
            }, tx, Vec::new(), sections)
        });
        harness.state_mut().remember_size = false;
        harness.state_mut().show_page("settings");
        harness.run_steps(2);
        harness
    }

    #[test]
    fn three_sections_lay_out_in_two_columns_rows_as_tall_as_the_taller_card() {
        let rects = layout(&[140.0, 184.0, 140.0], pos2(20.0, 344.0), 960.0);
        let col_w = (960.0 - GAP) / 2.0;
        assert_eq!(rects[0], Rect::from_min_size(pos2(20.0, 344.0), vec2(col_w, 184.0)));
        assert_eq!(rects[1], Rect::from_min_size(pos2(20.0 + col_w + GAP, 344.0), vec2(col_w, 184.0)));
        assert_eq!(rects[2], Rect::from_min_size(pos2(20.0, 344.0 + 184.0 + GAP), vec2(col_w, 140.0)), "alone, left");
        assert!(layout(&[], pos2(0.0, 0.0), 960.0).is_empty());
    }

    #[test]
    fn the_sections_take_their_rows_and_the_gaps_above_them() {
        let (a, _) = TestSection::new("a", "A", 140.0);
        let (b, _) = TestSection::new("b", "B", 184.0);
        let (c, _) = TestSection::new("c", "C", 140.0);
        assert_eq!(total_height(&[]), 0.0);
        assert_eq!(total_height(&[Box::new(a)]), GAP + 140.0);
        let (a, _) = TestSection::new("a", "A", 140.0);
        assert_eq!(total_height(&[Box::new(a), Box::new(b), Box::new(c)]), GAP + 184.0 + GAP + 140.0);
    }

    /// Three sections show as cards after the core's, two to a row, and the page scrolls.
    #[test]
    fn sections_show_as_cards_after_the_core_cards_and_the_page_scrolls() {
        use egui_kittest::kittest::Queryable;

        let _state = crate::test_util::state_lock();
        let (a, _) = TestSection::new("one", "FIRST", 140.0);
        let (b, _) = TestSection::new("two", "SECOND", 184.0);
        let (c, probe_c) = TestSection::new("three", "THIRD", 140.0);
        let mut harness = harness(BASE_SIZE, vec![Box::new(a), Box::new(b), Box::new(c)]);
        let rect = |h: &egui_kittest::Harness<'_, App>, label: &str| h.get_by_label(label).rect();
        let (first, second, save) = (rect(&harness, "FIRST"), rect(&harness, "SECOND"), rect(&harness, "SAVE"));
        assert_eq!(first.top(), second.top(), "side by side");
        assert!(first.right() < second.left());
        assert!(first.top() > harness.get_by_label("Test").rect().bottom() + 40.0, "after the core cards");
        // The third card's row lies below the window: scrolled into view with the wheel; the
        // footer stays where it was.
        assert!(probe_c.drawn.get() > 0, "drawn, if clipped");
        harness.get_by_label("Find").hover();
        harness.step();
        harness.event(egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: vec2(0.0, -2000.0), phase: egui::TouchPhase::Move, modifiers: egui::Modifiers::NONE });
        harness.run_steps(8);
        let third = rect(&harness, "THIRD");
        let first_after = rect(&harness, "FIRST");
        assert!(first_after.top() < first.top(), "scrolled");
        assert_eq!(third.left(), first_after.left(), "alone in its row, left");
        assert!((third.top() - (first_after.top() + 184.0 + GAP)).abs() < 0.5, "below a row as tall as the taller card");
        assert_eq!(rect(&harness, "SAVE"), save, "the footer stays");
    }

    /// An edited section enables SAVE, which saves only it; settings.json's copy stays as it was.
    #[test]
    fn save_saves_a_dirty_section_once_and_leaves_the_core_settings() {
        use egui_kittest::kittest::Queryable;

        let _state = crate::test_util::state_lock();
        let (a, probe_a) = TestSection::new("one", "FIRST", 140.0);
        let (b, probe_b) = TestSection::new("two", "SECOND", 140.0);
        let mut harness = harness(TALL, vec![Box::new(a), Box::new(b)]);
        assert!(crate::test_util::painted_text_color(harness.output(), "Unsaved changes").is_none());
        harness.get_by_label("SAVE").click();
        harness.run_steps(2);
        assert_eq!(probe_a.saves.get(), 0, "SAVE is disabled with nothing to save");

        harness.get_by_label("one edit").click();
        harness.run_steps(2);
        assert!(crate::test_util::painted_text_color(harness.output(), "Unsaved changes").is_some());
        let before = harness.state().saved.clone();
        harness.get_by_label("SAVE").click();
        harness.run_steps(2);
        assert_eq!(probe_a.saves.get(), 1, "its save, once");
        assert_eq!(probe_b.saves.get(), 0, "a clean section is not saved");
        assert!(harness.state().saved == before && harness.state().edit == before, "the core's settings untouched");
        assert!(crate::test_util::painted_text_color(harness.output(), "Saved.").is_some());
    }

    /// A failing section shows its reason, stays unsaved, and the other one is saved all the same.
    #[test]
    fn a_failing_save_shows_its_reason_and_the_others_are_saved() {
        use egui_kittest::kittest::Queryable;

        let _state = crate::test_util::state_lock();
        let (a, probe_a) = TestSection::new("one", "FIRST", 140.0);
        let (b, probe_b) = TestSection::new("two", "SECOND", 140.0);
        *probe_a.fail.borrow_mut() = Some("disk full".into());
        probe_a.dirty.set(true);
        probe_b.dirty.set(true);
        let mut harness = harness(TALL, vec![Box::new(a), Box::new(b)]);
        harness.get_by_label("SAVE").click();
        harness.run_steps(2);
        assert_eq!((probe_a.saves.get(), probe_b.saves.get()), (1, 1));
        assert!(probe_a.dirty.get() && !probe_b.dirty.get());
        // The failure shows next to SAVE, not "Unsaved changes", though one is still unsaved.
        assert!(crate::test_util::painted_text_color(harness.output(), "Saving failed: disk full").is_some());
        assert!(crate::test_util::painted_text_color(harness.output(), "Unsaved changes").is_none());

        // Another note (the shots view reset) replaces the failure: "Unsaved changes" shows again.
        harness.state_mut().page_mut::<super::super::SettingsPage>().confirm_reset = true;
        harness.run_steps(2);
        harness.get_by_label("RESET").click();
        harness.run_steps(2);
        assert!(crate::test_util::painted_text_color(harness.output(), "Unsaved changes").is_some());

        // Fixed: SAVE again saves it.
        *probe_a.fail.borrow_mut() = None;
        harness.get_by_label("SAVE").click();
        harness.run_steps(2);
        assert_eq!((probe_a.saves.get(), probe_b.saves.get()), (2, 1));
        assert!(crate::test_util::painted_text_color(harness.output(), "Saved.").is_some());
    }

    /// The core's changes and a section's save together: settings.json's copy is saved too.
    #[test]
    fn save_saves_the_core_copy_and_the_sections() {
        use egui_kittest::kittest::Queryable;

        let _state = crate::test_util::state_lock();
        let (a, probe_a) = TestSection::new("one", "FIRST", 140.0);
        probe_a.dirty.set(true);
        let mut harness = harness(TALL, vec![Box::new(a)]);
        harness.state_mut().edit.auto_start_gspro = !harness.state().saved.auto_start_gspro;
        harness.run_steps(1);
        harness.get_by_label("SAVE").click();
        harness.run_steps(2);
        assert_eq!(probe_a.saves.get(), 1);
        assert!(harness.state().saved == harness.state().edit, "the core copy saved");
        assert!(crate::test_util::painted_text_color(harness.output(), "Saved.").is_some());
    }

    /// A greyed card with a badge: "<TITLE> ★", dimmed, its reason on the heading, the body usable.
    #[test]
    fn a_greyed_section_shows_its_badge_and_reason_and_stays_usable() {
        use egui_kittest::kittest::Queryable;

        let _state = crate::test_util::state_lock();
        let (mut a, probe) = TestSection::new("one", "FIRST", 140.0);
        a.badge = Some("★");
        probe.greyed.set(Some("Not available yet"));
        let mut harness = harness(TALL, vec![Box::new(a)]);
        assert!(harness.query_by_label("FIRST").is_none(), "the heading carries the badge");
        let painted = |h: &egui_kittest::Harness<'_, App>| glyph_color(h.output(), "FIRST ★");
        let normal = super::super::LABEL;
        assert_eq!(painted(&harness), Some(normal.gamma_multiply(DISABLED)), "greyed: dimmed");
        harness.get_by_label("FIRST ★").hover();
        harness.run_steps(3);
        harness.get_by_label("Not available yet");
        harness.get_by_label("one edit").click();
        harness.run_steps(2);
        assert!(probe.dirty.get(), "its controls stay usable");

        probe.greyed.set(None);
        harness.get_by_label("SAVE").hover();
        harness.run_steps(3);
        assert_eq!(painted(&harness), Some(normal), "no longer greyed: as usual");
        assert!(harness.query_by_label("Not available yet").is_none());
    }

    /// Without sections the core cards are where they were, and the footer right below them,
    /// inside the window (456 = 470 less the page margin): 66 (top bar and margin) + 334 (the
    /// cards) + 6 (spacing) + 12. Before, the footer followed the last card's content and, without
    /// an update source, covered the cards' last row.
    #[test]
    fn without_sections_the_core_cards_stay_and_the_footer_follows_them() {
        use egui_kittest::kittest::Queryable;

        let _state = crate::test_util::state_lock();
        let harness = harness(BASE_SIZE, Vec::new());
        assert_eq!(harness.get_by_label("SAVE").rect(), Rect::from_min_max(pos2(822.0, 418.0), pos2(982.0, 456.0)));
        assert_eq!(harness.get_by_label("RESET SHOTS VIEW").rect().min, pos2(18.0, 418.0));
        let find = harness.get_by_label("Find").rect().min;
        assert!((find.x - 324.3).abs() < 0.1 && (find.y - 114.0).abs() < 0.1, "the first card's controls where they were: {find:?}");
    }
}
