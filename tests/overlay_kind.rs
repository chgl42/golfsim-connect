//! An overlay kind as another crate adds it: only the library's public interface (ADR-017). Its
//! own test binary, so registering the process-wide kind leaves the lib tests' Overlay page as it
//! is, and setting `HOME` touches no other test.
#![cfg(feature = "gui")]

use openlaunch_connect::settings::{ExtensionOverlay, OverlayPos, Settings};
use openlaunch_connect::ui::egui;
use openlaunch_connect::ui::overlay::{self, OverlayKind};

/// The time, in a panel of its own.
struct Clock;

impl OverlayKind for Clock {
    fn id(&self) -> &'static str {
        "clock"
    }

    fn name(&self) -> &'static str {
        "Clock"
    }

    fn size(&self, _ctx: &egui::Context) -> Option<egui::Vec2> {
        Some(egui::vec2(240.0, 96.0))
    }

    fn ui(&self, ui: &mut egui::Ui, rect: egui::Rect) {
        ui.painter().rect_filled(rect, 6.0, egui::Color32::from_black_alpha(200));
    }
}

/// Another kind with the same id.
struct Impostor;

impl OverlayKind for Impostor {
    fn id(&self) -> &'static str {
        "clock"
    }

    fn name(&self) -> &'static str {
        "Impostor"
    }

    fn size(&self, _ctx: &egui::Context) -> Option<egui::Vec2> {
        None
    }

    fn ui(&self, _ui: &mut egui::Ui, _rect: egui::Rect) {}
}

#[test]
fn another_crate_adds_an_overlay_kind() {
    assert!(overlay::overlay_kind("clock").is_none());
    overlay::register_overlay_kind(&Clock);
    // An id that is already there is left out.
    overlay::register_overlay_kind(&Impostor);
    let kind = overlay::overlay_kind("clock").expect("registered");
    assert_eq!((kind.name(), kind.short_label()), ("Clock", "Clock"), "the short label defaults to the name");
    assert!(overlay::overlay_kind("nope").is_none());

    // Its entry survives a load and save, with the kind's own fields.
    let home = std::env::temp_dir().join(format!("openlaunch-overlay-kind-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    // SAFETY: the only test of this binary, before any other thread reads the environment.
    unsafe { std::env::set_var("HOME", &home) };
    let dir = Settings::data_dir().expect("HOME is set");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("settings.json"), r#"{"extension_overlays":[{"id":"clock","pos":"right","spot":[0.5,0.25],"rows":4}]}"#).unwrap();
    let mut settings = Settings::load();
    let entry = &settings.extension_overlays[0];
    assert_eq!((entry.id.as_str(), entry.pos, entry.spot), ("clock", OverlayPos::Right, Some([0.5, 0.25])));
    settings.extension_overlays.push(ExtensionOverlay { pos: OverlayPos::Top, ..ExtensionOverlay::new("other") });
    settings.save().unwrap();
    let again = Settings::load();
    assert_eq!(again, settings);
    assert_eq!(again.extension_overlays[0].config["rows"], 4);
    let _ = std::fs::remove_dir_all(&home);
}
