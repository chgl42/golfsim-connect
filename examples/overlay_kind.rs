//! A sample overlay kind as another crate adds it (ADR-016): a panel with the number of shots of
//! the session and the last shot's club, in a window of its own over GSPro. Turn it on in the
//! Overlay page's *SAMPLE* tab.
//!
//! `OPENLAUNCH_DEMO=range cargo run --example overlay_kind`

use openlaunch_connect::state;
use openlaunch_connect::ui::{self, eframe, egui};
use openlaunch_connect::ui::overlay::{self, OverlayKind};

struct Sample;

impl OverlayKind for Sample {
    fn id(&self) -> &'static str {
        "sample"
    }

    fn name(&self) -> &'static str {
        "Sample"
    }

    /// Nothing to show (no window) before the session's first shot.
    fn size(&self, _ctx: &egui::Context) -> Option<egui::Vec2> {
        state::with(|s| !s.session.is_empty()).then_some(egui::vec2(220.0, 64.0))
    }

    fn ui(&self, ui: &mut egui::Ui, rect: egui::Rect) {
        let (shots, club) = state::with(|s| (s.session.len(), s.session.last().and_then(|r| r.club_code.clone())));
        let p = ui.painter();
        p.rect_filled(rect, 8.0, egui::Color32::from_rgba_unmultiplied(0x0b, 0x16, 0x2a, 220));
        let text = format!("{shots} shots · last {}", club.as_deref().unwrap_or("–"));
        p.text(rect.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(18.0), egui::Color32::WHITE);
    }
}

fn main() -> eframe::Result {
    overlay::register_overlay_kind(&Sample);
    ui::run_with(Vec::new())
}
