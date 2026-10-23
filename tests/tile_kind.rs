//! A tile kind as another crate adds it: only the library's public interface (ADR-009).
#![cfg(feature = "gui")]

use openlaunch_connect::settings::Tile;
use openlaunch_connect::ui::egui;
use openlaunch_connect::ui::explorer::{self, TileCtx, TileKind, Units};
use serde::{Deserialize, Serialize};

/// Carry per club, longest first.
struct Gapping;

#[derive(Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Config {
    total: bool,
}

impl TileKind for Gapping {
    fn id(&self) -> &'static str {
        "gapping"
    }

    fn name(&self) -> &'static str {
        "Gapping"
    }

    fn options(&self, ui: &mut egui::Ui, tile: &mut Tile, _units: Units) {
        tile.change_config(|cfg: &mut Config| {
            ui.checkbox(&mut cfg.total, "Total");
        });
    }

    fn body(&self, ui: &mut egui::Ui, tile: &mut Tile, ctx: &TileCtx) {
        let total = tile.config::<Config>().total;
        let u = ctx.units();
        for (shot, f) in ctx.shots().iter().zip(ctx.flights()) {
            let d = if total { f.total } else { f.carry };
            let text = format!("{} {:.0} {}", shot.club_code.as_deref().unwrap_or("–"), u.dist(d), u.dist_unit());
            let label = ui.colored_label(ctx.club_color(&shot.club_code), text);
            if label.interact(egui::Sense::click()).clicked() {
                ctx.click(shot);
            }
        }
        *ctx.state::<usize>() += 1;
    }
}

#[test]
fn another_crate_adds_a_tile_kind() {
    explorer::register_tile_kind(&Gapping);
    // Once is enough; a second registration of the id is left out.
    explorer::register_tile_kind(&Gapping);
    let tile = Tile::new("gapping", openlaunch_connect::settings::TileSize::W2H1).with(&Config { total: true });
    assert!(tile.config::<Config>().total);
}
