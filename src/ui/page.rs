//! The pages of the app: what a page implements ([`Page`]), what it is given to reach what pages
//! share ([`AppCtx`]), and the list the navigation is built from (ADR-010).

use std::sync::mpsc::Sender;

use crate::gspro::Cmd;
use crate::log;
use crate::settings::Settings;

use super::Moving;
use super::egui;
use super::explorer::ExplorerState;

/// A page of the app: an entry in the navigation and what is drawn below the top bar when it is
/// chosen. A page keeps its own state in its fields; what pages share comes through [`AppCtx`].
///
/// The core pages are *Play*, *Shots*, *Log*, *Overlay* and *Settings*; another crate adds its
/// own with [`run_with`](super::run_with).
pub trait Page: std::any::Any {
    /// Unique, lower case, e.g. `"sound"`; `OPENLAUNCH_PAGE=<id>` opens the page first.
    fn id(&self) -> &'static str;
    /// The entry in the navigation, in capitals and translated with `tr` (a table of its own
    /// crate goes in with `i18n::register`).
    fn label(&self) -> &'static str;
    /// Draws the page; `ui` is the room below the top bar, inside the page margins.
    fn ui(&mut self, ui: &mut egui::Ui, app: &mut AppCtx<'_>);
}

/// What the pages share, lent to the page drawn: the settings and the way to change them, the
/// commands to the bridge, and for the core pages the shots explorer's state and the overlay
/// being moved by hand (the overlay windows use both).
pub struct AppCtx<'a> {
    saved: &'a mut Settings,
    /// The Settings page's copy: every change made through [`AppCtx::change_settings`] is made
    /// to it, too, so its unsaved edits stay.
    pub(super) edit: &'a mut Settings,
    tx: &'a Sender<Cmd>,
    pub(super) explorer: &'a mut ExplorerState,
    pub(super) moving: &'a mut Option<Moving>,
}

impl<'a> AppCtx<'a> {
    pub(super) fn new(
        saved: &'a mut Settings,
        edit: &'a mut Settings,
        tx: &'a Sender<Cmd>,
        explorer: &'a mut ExplorerState,
        moving: &'a mut Option<Moving>,
    ) -> Self {
        Self { saved, edit, tx, explorer, moving }
    }

    /// The settings in use (as saved).
    #[must_use]
    pub fn settings(&self) -> &Settings {
        self.saved
    }

    /// The one way settings change outside the Settings page's SAVE: `change` is applied to the
    /// settings in use and to the Settings page's copy (so its unsaved edits stay, and stay
    /// visible as such), then the settings are saved. A failure is logged.
    ///
    /// # Errors
    /// Returns why the settings could not be saved (they are changed in the app all the same).
    pub fn change_settings(&mut self, change: impl Fn(&mut Settings)) -> Result<(), String> {
        change(self.saved);
        change(self.edit);
        self.saved.save().inspect_err(|e| log::info(&format!("Could not save the settings: {e}")))
    }

    /// Where commands to the bridge go, e.g. a test shot (`Cmd::Shot`).
    #[must_use]
    pub fn commands(&self) -> &Sender<Cmd> {
        self.tx
    }
}

/// The core pages in the order of the navigation, left to right, with `extra` (pages of other
/// crates) before *Settings*, which stays last.
pub(super) fn pages(extra: Vec<Box<dyn Page>>) -> Vec<Box<dyn Page>> {
    let mut pages: Vec<Box<dyn Page>> = vec![
        Box::new(super::PlayPage),
        Box::new(super::explorer::ShotsPage::default()),
        Box::new(super::LogPage),
        Box::new(super::overlay::OverlayPage::default()),
    ];
    pages.extend(extra);
    pages.push(Box::new(super::SettingsPage::default()));
    pages
}

/// The page shown first: the one named `wanted` (`OPENLAUNCH_PAGE`), else the first.
pub(super) fn start_page(pages: &[Box<dyn Page>], wanted: Option<&str>) -> usize {
    wanted.and_then(|id| pages.iter().position(|p| p.id() == id)).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{App, BASE_SIZE, setup_style};
    use std::cell::Cell;
    use std::rc::Rc;

    /// A page as another crate adds it: counts how often it is drawn and changes a setting.
    struct SoundPage {
        drawn: Rc<Cell<u32>>,
    }

    impl Page for SoundPage {
        fn id(&self) -> &'static str {
            "sound"
        }

        fn label(&self) -> &'static str {
            "SOUND"
        }

        fn ui(&mut self, ui: &mut egui::Ui, app: &mut AppCtx<'_>) {
            self.drawn.set(self.drawn.get() + 1);
            ui.label("Sound page");
            if app.settings().metric {
                let _ = app.change_settings(|s| s.metric = false);
            }
        }
    }

    #[test]
    fn extra_pages_come_before_settings() {
        let drawn = Rc::new(Cell::new(0));
        let pages = pages(vec![Box::new(SoundPage { drawn })]);
        let ids: Vec<&str> = pages.iter().map(|p| p.id()).collect();
        assert_eq!(ids, ["play", "shots", "log", "overlay", "sound", "settings"]);
        assert_eq!(start_page(&pages, Some("sound")), 4);
        assert_eq!(start_page(&pages, Some("settings")), 5);
        assert_eq!(start_page(&pages, Some("nope")), 0, "an unknown page: the first");
        assert_eq!(start_page(&pages, None), 0);
    }

    /// A page given to the app shows in the navigation, opens from there and reaches the settings.
    #[test]
    fn an_extra_page_is_in_the_navigation_and_renders() {
        use egui_kittest::Harness;
        use egui_kittest::kittest::Queryable;

        let _state = crate::test_util::state_lock();
        crate::settings::SAVE.store(false, std::sync::atomic::Ordering::Relaxed);
        crate::i18n::set(crate::i18n::Language::English);
        let drawn = Rc::new(Cell::new(0));
        let page = SoundPage { drawn: drawn.clone() };
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut harness = Harness::builder().with_size(BASE_SIZE).build_eframe(|cc| {
            setup_style(&cc.egui_ctx);
            App::with_pages(Settings { overlay: crate::settings::OverlayPos::Off, ..Settings::default() }, tx, vec![Box::new(page)])
        });
        harness.state_mut().remember_size = false;
        harness.step();
        assert_eq!(drawn.get(), 0, "the Play page is shown first");
        harness.get_by_label("SOUND").click();
        harness.step();
        harness.step();
        assert!(drawn.get() > 0);
        assert_eq!(harness.state().pages[harness.state().page].id(), "sound");
        harness.get_by_label("Sound page");
        let app = harness.state();
        assert!(!app.saved.metric && !app.edit.metric, "changed through AppCtx");
        // The core pages are still there.
        harness.get_by_label("SETTINGS").click();
        harness.step();
        assert_eq!(harness.state().pages[harness.state().page].id(), "settings");
    }
}
