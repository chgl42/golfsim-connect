//! Overlay kinds of other crates (ADR-017): a window of their own over GSPro that sits, moves and
//! stacks like the tile overlays. A kind is registered once, before `ui::run_with`; its entry in
//! the settings (`settings::ExtensionOverlay`) says where it sits.

use std::sync::{PoisonError, RwLock};

use super::super::egui;

/// A kind of overlay window another crate adds: the core makes the window (borderless,
/// transparent, click-through, always on top, placed like its own overlays) and the Overlay page's
/// tab for it; the kind says how large it is and draws it. Its entry in the settings
/// (`settings::ExtensionOverlay`) has the keys `id`, `pos` and `spot`; fields a kind keeps there
/// use other names.
pub trait OverlayKind: Sync {
    /// Unique, lower case, e.g. `"clock"`: the id of its entry in the settings and the end of
    /// its window's title.
    fn id(&self) -> &'static str;
    /// Translated with `tr` (a table of its own crate goes in with `i18n::register`); its tab on
    /// the Overlay page shows it in capitals.
    fn name(&self) -> &'static str;
    /// The tab's label when the tabs don't fit beside the arrangement's name (in capitals, too).
    fn short_label(&self) -> &'static str {
        self.name()
    }
    /// The room it draws in now, in points (drawn at zoom 1, whatever the main window's zoom);
    /// `None` when there is nothing to show: then there is no window.
    fn size(&self, ctx: &egui::Context) -> Option<egui::Vec2>;
    /// Draws it into `rect` (of the size `size` gave) in its window, and in the Overlay page's
    /// preview.
    fn ui(&self, ui: &mut egui::Ui, rect: egui::Rect);
}

/// Kinds added by other crates, in the order they were registered.
static REGISTERED: RwLock<Vec<&'static dyn OverlayKind>> = RwLock::new(Vec::new());

/// Adds a kind of overlay: it gets a tab on the Overlay page, and its entry in the settings is
/// drawn by it. Call it before `ui::run_with`. A kind with an id that is already there is left
/// out.
pub fn register_overlay_kind(kind: &'static dyn OverlayKind) {
    let mut kinds = REGISTERED.write().unwrap_or_else(PoisonError::into_inner);
    if !kinds.iter().any(|k| k.id() == kind.id()) {
        kinds.push(kind);
    }
}

/// The registered kind with the id `id`, if there is one.
#[must_use]
pub fn overlay_kind(id: &str) -> Option<&'static dyn OverlayKind> {
    kinds().into_iter().find(|k| k.id() == id)
}

/// Every registered kind, in the order they were registered (the app takes them once, at start).
pub(in super::super) fn kinds() -> Vec<&'static dyn OverlayKind> {
    REGISTERED.read().unwrap_or_else(PoisonError::into_inner).clone()
}
