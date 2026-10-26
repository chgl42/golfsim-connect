//! *Help*: the common fixes (the README's *Troubleshooting* is the longer reference) and how to
//! ask for help with the copied log.

use eframe::egui::{self, Margin, RichText};

use crate::i18n::tr;

use super::APP_NAME;
use super::kit::{ICON, LABEL, MUTED, SOFT, TILE, cond};
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
                    app(tr("GSPro runs in CrossOver and needs a licence for the R10. {app} does the Bluetooth part on the Mac and hands the shots to GSPro over 127.0.0.1:921.")),
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
                    bullet(ui, &app(tr("The copied log shows ~ for your home folder and <email> for email addresses.")));
                    let link = ui.link(RichText::new(tr("Open the issues on GitHub")).size(12.5).color(ICON).underline());
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
        ui.spacing_mut().item_spacing.y = 3.0;
        ui.label(RichText::new(title).font(cond(15.0)).color(LABEL).extra_letter_spacing(1.5));
        add(ui);
    });
    ui.add_space(6.0);
}

/// One fix: a muted dot, then the wrapped text.
fn bullet(ui: &mut egui::Ui, text: &str) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(RichText::new("•").size(12.5).color(MUTED));
        ui.add(egui::Label::new(RichText::new(text).size(12.5).color(SOFT)).wrap());
    });
}

#[cfg(test)]
mod tests {
    use super::super::{App, BASE_SIZE, setup_style};
    use crate::i18n::{self, Language, tr};
    use crate::settings::{OverlayPos, Settings};
    use eframe::egui;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    /// The Help page is in the navigation before Settings and shows the four fixes and how to
    /// ask for help, in every shipped language.
    #[test]
    fn help_lists_the_fixes_and_how_to_ask() {
        let _state = crate::test_util::state_lock();
        crate::settings::SAVE.store(false, std::sync::atomic::Ordering::Relaxed);
        for code in ["en", "de"] {
            i18n::set(&Language::new(code));
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
}
