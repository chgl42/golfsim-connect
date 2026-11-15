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
/// The core pages are *Play*, *Shots*, *Log*, *Overlay*, *Help* and *Settings*; another crate
/// adds its own with [`run_with`](super::run_with).
pub trait Page: std::any::Any {
    /// Unique, lower case, e.g. `"sound"`; `OPENLAUNCH_PAGE=<id>` opens the page first.
    fn id(&self) -> &'static str;
    /// The entry in the navigation, in capitals and translated with `tr` (a table of its own
    /// crate goes in with `i18n::register`, one per language).
    fn label(&self) -> &'static str;
    /// A mark drawn after the label in the navigation, one space apart and in its colour (e.g.
    /// `"★"`), translated with `tr`; the entry's accessible label is then `"<label> <badge>"`.
    /// Asked every frame, so a change shows at once. None by default (ADR-016).
    fn badge(&self) -> Option<&'static str> {
        None
    }
    /// `Some(why)` draws the navigation entry greyed (the kit's [`DISABLED`](super::kit::DISABLED)
    /// look) with `why` as its tooltip, translated with `tr`. The entry stays clickable and opens
    /// the page as always; what the page shows then is its own. Asked every frame. None by
    /// default (ADR-016).
    fn greyed(&self) -> Option<&'static str> {
        None
    }
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
/// crates) before *Help* and *Settings*, which stay last.
pub(super) fn pages(extra: Vec<Box<dyn Page>>) -> Vec<Box<dyn Page>> {
    let mut pages: Vec<Box<dyn Page>> = vec![
        Box::new(super::PlayPage),
        Box::new(super::explorer::ShotsPage::default()),
        Box::new(super::LogPage::default()),
        Box::new(super::overlay::OverlayPage::default()),
    ];
    pages.extend(extra);
    pages.push(Box::new(super::help::HelpPage));
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

    /// A page as an extension marks it: a badge, and greyed while `greyed` holds a reason.
    struct MarkedPage {
        greyed: Rc<Cell<Option<&'static str>>>,
        drawn: Rc<Cell<u32>>,
    }

    impl Page for MarkedPage {
        fn id(&self) -> &'static str {
            "sound"
        }

        fn label(&self) -> &'static str {
            "SOUND"
        }

        fn badge(&self) -> Option<&'static str> {
            Some("★")
        }

        fn greyed(&self) -> Option<&'static str> {
            self.greyed.get()
        }

        fn ui(&mut self, ui: &mut egui::Ui, _app: &mut AppCtx<'_>) {
            self.drawn.set(self.drawn.get() + 1);
            ui.label("Sound page");
        }
    }

    /// A marked page shows its badge in the navigation, greyed with its reason as tooltip, opens
    /// on a click all the same, and looks as usual the frame after it is no longer greyed.
    #[test]
    fn a_marked_page_shows_its_badge_and_reason_and_still_opens() {
        use egui_kittest::Harness;
        use egui_kittest::kittest::Queryable;

        let _state = crate::test_util::state_lock();
        crate::settings::SAVE.store(false, std::sync::atomic::Ordering::Relaxed);
        crate::i18n::set(&crate::i18n::Language::new("en"));
        let greyed = Rc::new(Cell::new(Some("Not available yet")));
        let drawn = Rc::new(Cell::new(0));
        let page = MarkedPage { greyed: greyed.clone(), drawn: drawn.clone() };
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut harness = Harness::builder().with_size(BASE_SIZE).build_eframe(|cc| {
            setup_style(&cc.egui_ctx);
            cc.egui_ctx.all_styles_mut(|s| s.interaction.tooltip_delay = 0.0);
            App::with_pages(Settings { overlay: crate::settings::OverlayPos::Off, ..Settings::default() }, tx, vec![Box::new(page)])
        });
        harness.state_mut().remember_size = false;
        harness.run_steps(2);
        assert!(harness.query_by_label("SOUND").is_none(), "the label carries the badge");
        let painted = |harness: &Harness<'_, App>| crate::test_util::painted_text_color(harness.output(), "SOUND ★");
        assert_eq!(painted(&harness), Some(super::super::MUTED.gamma_multiply(super::super::DISABLED)), "greyed: dimmed");
        harness.get_by_label("SOUND ★").hover();
        harness.run_steps(3);
        harness.get_by_label("Not available yet");

        harness.get_by_label("SOUND ★").click();
        harness.run_steps(2);
        assert!(drawn.get() > 0, "a greyed page opens");
        assert_eq!(harness.state().pages[harness.state().page].id(), "sound");

        greyed.set(None);
        harness.get_by_label("SOUND ★").hover();
        harness.run_steps(3);
        assert!(harness.query_by_label("Not available yet").is_none(), "no reason once it is not greyed");
        harness.get_by_label("PLAY").click();
        harness.run_steps(2);
        assert_eq!(painted(&harness), Some(super::super::MUTED), "no longer greyed: as usual");
        // The core entries are as they were.
        harness.get_by_label("SETTINGS");
        harness.get_by_label("PLAY");
    }

    /// The entry's colour: active or hovered white, else muted; while the page is greyed at the
    /// kit's disabled look, and as usual once it is not.
    #[test]
    fn a_greyed_entry_is_dimmed() {
        use super::super::{DISABLED, MUTED, WHITE, entry_color};
        let greyed = Rc::new(Cell::new(Some("Not available yet")));
        let page = MarkedPage { greyed: greyed.clone(), drawn: Rc::new(Cell::new(0)) };
        assert_eq!(entry_color(&page, false, false), MUTED.gamma_multiply(DISABLED));
        assert_eq!(entry_color(&page, false, true), WHITE.gamma_multiply(DISABLED));
        assert_eq!(entry_color(&page, true, false), WHITE.gamma_multiply(DISABLED), "the page shown, greyed: dimmed too");
        greyed.set(None);
        assert_eq!(entry_color(&page, false, false), MUTED);
        assert_eq!(entry_color(&page, true, false), WHITE);
        assert_eq!(entry_color(&page, false, true), WHITE);
        let core = super::super::PlayPage;
        assert_eq!(entry_color(&core, false, false), MUTED, "a core page as before");
    }

    #[test]
    fn extra_pages_come_before_help_and_settings() {
        let drawn = Rc::new(Cell::new(0));
        let pages = pages(vec![Box::new(SoundPage { drawn })]);
        let ids: Vec<&str> = pages.iter().map(|p| p.id()).collect();
        assert_eq!(ids, ["play", "shots", "log", "overlay", "sound", "help", "settings"]);
        assert_eq!(start_page(&pages, Some("sound")), 4);
        assert_eq!(start_page(&pages, Some("settings")), 6);
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
        crate::i18n::set(&crate::i18n::Language::new("en"));
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
