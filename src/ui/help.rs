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
    use super::super::kit::READ;
    use super::super::{App, BASE_SIZE, setup_style};
    use crate::i18n::{self, Language, tr};
    use crate::settings::{OverlayPos, Settings};
    use eframe::egui::{self, accesskit::Role};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    /// The app at its base size in the current language, on the Help page.
    fn help_harness() -> Harness<'static, App> {
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut harness = Harness::builder().with_size(BASE_SIZE).build_eframe(|cc| {
            setup_style(&cc.egui_ctx);
            App::new(Settings { overlay: OverlayPos::Off, ..Settings::default() }, tx)
        });
        harness.state_mut().remember_size = false;
        harness.step();
        harness.get_by_label(tr("HELP")).click();
        harness.step();
        harness.step();
        harness
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
            let bullets: Vec<&(String, egui::Rect)> = labels
                .iter()
                .filter(|(text, _)| text != "•" && !headings.iter().any(|h| text == tr(h)))
                .collect();
            assert_eq!(bullets.len(), 16, "{code}: every bullet was looked at");
            assert_eq!(dots.len(), 16, "{code}: every bullet has its dot");

            let line = dots[0].height();
            assert!(line >= READ, "{code}: a text line is {line} high, under the reading size {READ}");
            for heading in headings {
                let height = harness.get_by_label(tr(heading)).rect().height();
                assert!(height > line, "{code}: {heading} is {height} high, a text line {line}");
            }
            for (text, rect) in bullets {
                assert!(rect.height() >= READ, "{code}: under the reading size: {text}");
                let column = if rect.min.x < right { left } else { right };
                assert!(rect.min.x >= column, "{code}: starts left of its column: {text}");
                assert!(rect.max.x <= column + inner_width, "{code}: wider than its column ({} > {}): {text}", rect.max.x, column + inner_width);
                assert!(
                    dots.iter().any(|dot| (dot.min.y - rect.min.y).abs() < 0.5 && dot.max.x <= rect.min.x),
                    "{code}: no dot on the first line of: {text}"
                );
            }

            let link = tr("Open the issues on GitHub");
            assert!(harness.get_by_label(link).rect().max.y > BASE_SIZE.y, "{code}: the page no longer fits, it scrolls");
            scroll_to_end(&mut harness);
            let rect = harness.get_by_label(link).rect();
            assert!(rect.max.y <= BASE_SIZE.y, "{code}: the issues link ends at {} after scrolling", rect.max.y);
            assert!(rect.height() >= READ, "{code}: the issues link is under the reading size");
            assert!(rect.max.x <= right + inner_width, "{code}: the issues link is wider than its column");
        }
        i18n::set(&Language::new("en"));
    }
}
