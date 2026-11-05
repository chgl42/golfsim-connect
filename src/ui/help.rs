//! *Help*: the common fixes (the README's *Troubleshooting* is the longer reference) and how to
//! ask for help with the copied log.

use eframe::egui::{self, Margin, RichText};

use crate::i18n::tr;

use super::APP_NAME;
use super::kit::{ICON, LABEL, MUTED, READ, READ_HEADING, SOFT, TILE, cond};
use super::page::{AppCtx, Page};

/// Where to ask for help: the public repository's issues.
pub(super) const ISSUES_URL: &str = concat!(env!("CARGO_PKG_REPOSITORY"), "/issues");

/// *Help*: the four common fixes and how to ask for help. It has no state of its own.
pub(super) struct HelpPage;

impl Page for HelpPage {
    fn id(&self) -> &'static str {
        "help"
    }

    fn label(&self) -> &'static str {
        tr("HELP")
    }

    fn ui(&mut self, ui: &mut egui::Ui, _app: &mut AppCtx<'_>) {
        let app = |text: &str| text.replace("{app}", APP_NAME);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.columns(2, |cols| {
                section(&mut cols[0], tr("R10 NOT FOUND OR LOST"), &[
                    app(tr("Is the R10 on? Turn off Bluetooth on your iPhone (or quit the Garmin Golf app) – otherwise the phone grabs the R10.")),
                    app(tr("Allow {app} in System Settings → Privacy & Security → Bluetooth.")),
                    app(tr("“Device error”: usually the R10 is not level. Put it on a flat surface.")),
                    app(tr("“No handshake”: a macOS pairing dialog may be hidden; otherwise switch the R10 off and on.")),
                    app(tr("macOS doesn't list the R10 in its Bluetooth settings – that's normal; the app connects on its own and reconnects when it is lost.")),
                ]);
                section(&mut cols[0], tr("GSPRO NOT CONNECTED"), &[
                    app(tr("GSPro must be running with its connect window (OpenConnect) open.")),
                    app(tr("Only one OpenConnect client can be connected – quit other connectors (e.g. gspro-r10).")),
                    app(tr("The address is in the settings (default 127.0.0.1:921) and applies after a restart.")),
                    app(tr("Shot sent but no ball flight: GSPro must be the active window with a ball teed up.")),
                ]);
                section(&mut cols[1], tr("CROSSOVER SETUP"), &[
                    app(tr("GSPro runs in CrossOver and needs a licence for the R10. {app} does the Bluetooth part on the Mac and hands the shots to GSPro over the GSPro address (default 127.0.0.1:921).")),
                    app(tr("To start GSPro from the app, set its launcher in the settings: Find searches ~/Applications/CrossOver/…, Choose … picks it by hand.")),
                ]);
                section(&mut cols[1], tr("OVERLAY NOT VISIBLE"), &[
                    app(tr("On the Overlay page, give the overlay a place – OFF hides it.")),
                    app(tr("Tile overlays step aside while GSPro shows its menus and come back on the range or a course; the status overlay stays.")),
                    app(tr("Moved by hand and lost? Choosing one of the nine places brings it back.")),
                ]);
                let ui = &mut cols[1];
                tile(ui, tr("ASKING FOR HELP"), |ui| {
                    bullet(ui, &app(tr("Copy the log (COPY LOG on the Log page) and open an issue on GitHub; paste the log into it.")));
                    bullet(ui, &app(tr("The copied log shows ~ for your home folder, <email> for email addresses and <device> for other Bluetooth devices nearby. Nothing else is masked – look the log over before you post it.")));
                    let link = ui.link(RichText::new(tr("Open the issues on GitHub")).size(READ).color(ICON).underline());
                    if link.on_hover_text(ISSUES_URL).clicked() {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(ISSUES_URL));
                    }
                });
            });
        });
    }
}

/// A tile with a heading and one bullet per text.
fn section(ui: &mut egui::Ui, title: &str, texts: &[String]) {
    tile(ui, title, |ui| {
        for text in texts {
            bullet(ui, text);
        }
    });
}

/// A tile with a condensed heading; `add` fills it.
fn tile(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new().fill(TILE).corner_radius(6).inner_margin(Margin::symmetric(12, 8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.label(RichText::new(title).font(cond(READ_HEADING)).color(LABEL).extra_letter_spacing(1.5));
        add(ui);
    });
    ui.add_space(6.0);
}

/// One fix: a muted dot, then the wrapped text.
fn bullet(ui: &mut egui::Ui, text: &str) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(RichText::new("•").size(READ).color(MUTED));
        ui.add(egui::Label::new(RichText::new(text).size(READ).color(SOFT)).wrap());
    });
}

#[cfg(test)]
mod tests {
    use super::super::kit::{READ, READ_HEADING, cond};
    use super::super::{App, BASE_SIZE, fit_zoom, setup_style};
    use crate::i18n::{self, Language, tr};
    use crate::settings::{OverlayPos, Settings};
    use eframe::egui::{self, accesskit::Role};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    /// The app in a window of `size`, interface size Auto, in the current language.
    fn app_harness(size: egui::Vec2) -> Harness<'static, App> {
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut harness = Harness::builder().with_size(size).build_eframe(|cc| {
            setup_style(&cc.egui_ctx);
            App::new(Settings { overlay: OverlayPos::Off, ..Settings::default() }, tx)
        });
        harness.state_mut().remember_size = false;
        harness
    }

    /// The app at its base size in the current language, on the Help page.
    fn help_harness() -> Harness<'static, App> {
        let mut harness = app_harness(BASE_SIZE);
        harness.step();
        harness.get_by_label(tr("HELP")).click();
        harness.step();
        harness.step();
        harness
    }

    /// The height of one line of `font` as a node's rect shows it: in window pixels, as egui
    /// rounds a row to whole pixels when it lays the text out.
    fn line_height(harness: &Harness<'static, App>, font: &egui::FontId) -> f32 {
        (harness.ctx.fonts_mut(|fonts| fonts.row_height(font)) * harness.ctx.pixels_per_point()).round()
    }

    /// `height` is one or more whole lines of `row` high.
    fn whole_lines(height: f32, row: f32) -> bool {
        let lines = height / row;
        lines.round() >= 1.0 && (lines - lines.round()).abs() < 0.01
    }

    /// The mouse wheel, far enough for the end of the page.
    fn scroll_to_end(harness: &mut Harness<'static, App>) {
        harness.hover_at(egui::pos2(BASE_SIZE.x / 2.0, BASE_SIZE.y / 2.0));
        harness.step();
        harness.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -2000.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run_steps(8);
    }

    /// The Help page is in the navigation before Settings and shows the four fixes and how to
    /// ask for help, in every shipped language.
    #[test]
    fn help_lists_the_fixes_and_how_to_ask() {
        let _state = crate::test_util::state_lock();
        crate::settings::SAVE.store(false, std::sync::atomic::Ordering::Relaxed);
        for code in ["en", "de"] {
            i18n::set(&Language::new(code));
            let mut harness = help_harness();
            assert_eq!(harness.state().pages[harness.state().page].id(), "help");
            for heading in [
                tr("R10 NOT FOUND OR LOST"),
                tr("GSPRO NOT CONNECTED"),
                tr("CROSSOVER SETUP"),
                tr("OVERLAY NOT VISIBLE"),
                tr("ASKING FOR HELP"),
                tr("Open the issues on GitHub"),
            ] {
                harness.get_by_label(heading);
            }
            if code == "en" {
                scroll_to_end(&mut harness);
                harness.get_by_label("Open the issues on GitHub").click();
                harness.step();
                let opened = harness.output().platform_output.commands.iter().any(|c| {
                    matches!(c, egui::OutputCommand::OpenUrl(open) if open.url == super::ISSUES_URL)
                });
                assert!(opened, "the link opens the issues");
            }
            let ids: Vec<&str> = harness.state().pages.iter().map(|p| p.id()).collect();
            assert_eq!(ids[ids.len() - 2..], ["help", "settings"]);
        }
        i18n::set(&Language::new("en"));
    }

    /// The fixes are read from the simulator: at the base size, in every shipped language, the
    /// text has the reading size, headings stand above it, the bullets wrap inside their column
    /// with the dot on the first line, and the page scrolls down to the issues link.
    #[test]
    fn help_text_is_large_stays_in_its_columns_and_scrolls() {
        const TILE_MARGIN: f32 = 12.0;
        /// The page's margin below its room (`mod.rs`).
        const PAGE_MARGIN_BOTTOM: f32 = 14.0;
        assert_eq!(READ, 16.0, "the reading size");
        assert_eq!(READ_HEADING, 19.0, "the reading heading size");
        let _state = crate::test_util::state_lock();
        crate::settings::SAVE.store(false, std::sync::atomic::Ordering::Relaxed);
        let headings = ["R10 NOT FOUND OR LOST", "GSPRO NOT CONNECTED", "CROSSOVER SETUP", "OVERLAY NOT VISIBLE", "ASKING FOR HELP"];
        for code in ["en", "de"] {
            i18n::set(&Language::new(code));
            let mut harness = help_harness();
            let spacing = harness.ctx.global_style().spacing.item_spacing.x;
            let left = harness.get_by_label(tr("R10 NOT FOUND OR LOST")).rect().min.x;
            let right = harness.get_by_label(tr("CROSSOVER SETUP")).rect().min.x;
            let inner_width = right - left - spacing - 2.0 * TILE_MARGIN;
            assert_eq!(harness.get_by_label(tr("GSPRO NOT CONNECTED")).rect().min.x, left, "{code}: two columns");
            assert_eq!(harness.get_by_label(tr("OVERLAY NOT VISIBLE")).rect().min.x, right, "{code}: two columns");

            let labels: Vec<(String, egui::Rect)> = harness
                .get_all_by_role(Role::Label)
                .filter(|n| n.rect().min.y >= harness.get_by_label(tr("R10 NOT FOUND OR LOST")).rect().min.y)
                .map(|n| (n.value().unwrap_or_default(), n.rect()))
                .collect();
            let dots: Vec<egui::Rect> = labels.iter().filter(|(text, _)| text == "•").map(|(_, r)| *r).collect();
            // Not the link: egui makes a link's node a label once the link is on screen.
            let link = tr("Open the issues on GitHub");
            let bullets: Vec<&(String, egui::Rect)> = labels
                .iter()
                .filter(|(text, _)| text != "•" && text != link && !headings.iter().any(|h| text == tr(h)))
                .collect();

            let line = line_height(&harness, &egui::FontId::proportional(READ));
            let heading_line = line_height(&harness, &cond(READ_HEADING));
            assert!(heading_line > line, "{code}: a heading's line {heading_line} is not above a text line {line}");
            for dot in &dots {
                assert!((dot.height() - line).abs() < 0.01, "{code}: a dot is {} high, a line at the reading size {line}", dot.height());
            }
            for heading in headings {
                let height = harness.get_by_label(tr(heading)).rect().height();
                assert!((height - heading_line).abs() < 0.01, "{code}: {heading} is {height} high, a heading line {heading_line}");
            }
            let height = harness.get_by_label(link).rect().height();
            assert!((height - line).abs() < 0.01, "{code}: the issues link is {height} high, a line at the reading size {line}");
            for (text, rect) in &bullets {
                assert!(whole_lines(rect.height(), line), "{code}: {} high, not whole lines of {line}: {text}", rect.height());
                let column = if rect.min.x < right { left } else { right };
                assert!(rect.min.x >= column, "{code}: starts left of its column: {text}");
                assert!(rect.max.x <= column + inner_width, "{code}: wider than its column ({} > {}): {text}", rect.max.x, column + inner_width);
                assert!(
                    dots.iter().any(|dot| dot.min.x >= column && (dot.min.y - rect.min.y).abs() < 0.5 && dot.max.x <= rect.min.x),
                    "{code}: no dot on the first line of: {text}"
                );
            }
            assert_eq!(bullets.len(), 16, "{code}: every bullet was looked at");
            assert_eq!(dots.len(), 16, "{code}: every bullet has its dot");

            assert!(harness.get_by_label(link).rect().max.y > BASE_SIZE.y, "{code}: the page no longer fits, it scrolls");
            scroll_to_end(&mut harness);
            let rect = harness.get_by_label(link).rect();
            let room_end = BASE_SIZE.y - PAGE_MARGIN_BOTTOM;
            assert!(rect.max.y <= room_end, "{code}: the issues link ends at {} after scrolling, the room at {room_end}", rect.max.y);
            assert!(rect.max.x <= right + inner_width, "{code}: the issues link is wider than its column");
        }
        i18n::set(&Language::new("en"));
    }

    /// A window twice the base size with the interface size Auto zooms the page by 2: its layout
    /// and sizes in points stay those of the base size, so it shows twice as large.
    #[test]
    fn help_at_a_large_window_is_zoomed_by_two() {
        let _state = crate::test_util::state_lock();
        crate::settings::SAVE.store(false, std::sync::atomic::Ordering::Relaxed);
        let size = egui::vec2(2000.0, 940.0);
        for code in ["en", "de"] {
            i18n::set(&Language::new(code));
            let mut harness = app_harness(size);
            harness.state_mut().show_page("help");
            harness.run_steps(8);
            assert!((harness.ctx.zoom_factor() - fit_zoom(size)).abs() < 0.01);
            let room = harness.ctx.content_rect().size();
            assert!((room - BASE_SIZE).length() < 0.01, "{code}: the page is laid out in the base size, not {room:?}");
            let line = line_height(&harness, &egui::FontId::proportional(READ));
            let heading_line = line_height(&harness, &cond(READ_HEADING));
            let link = harness.get_by_label(tr("Open the issues on GitHub")).rect().height();
            assert!((link - line).abs() < 0.01, "{code}: the issues link is {link} high, a zoomed line {line}");
            let heading = harness.get_by_label(tr("CROSSOVER SETUP")).rect().height();
            assert!((heading - heading_line).abs() < 0.01, "{code}: the heading is {heading} high, a zoomed line {heading_line}");
        }
        i18n::set(&Language::new("en"));
    }

    /// The reading sizes stay on the Help page: every other page keeps its own sizes.
    #[test]
    fn only_help_uses_the_reading_sizes() {
        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    out.push(path);
                }
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui");
        let mut files = Vec::new();
        walk(&root, &mut files);
        assert!(files.len() >= 6, "the UI's files are found");
        for path in files {
            if path == root.join("kit.rs") || path == root.join("help.rs") {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap();
            let used = source.split(|c: char| !c.is_alphanumeric() && c != '_').any(|word| word == "READ" || word == "READ_HEADING");
            assert!(!used, "{} uses a reading size", path.display());
        }
    }
}
