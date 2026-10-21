//! Persistent app settings (~/Library/Application Support/OpenLaunch Connect/settings.json).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::engine::Config;
use crate::history::ExportFormat;
use crate::i18n::Language;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub gspro_addr: String,
    pub tee_distance: f32,
    pub launcher_path: Option<PathBuf>,
    pub auto_start_gspro: bool,
    pub device: Option<String>,
    /// Show speeds in km/h (true) or mph (false). Display only; GSPro always gets mph.
    pub metric: bool,
    /// Show distances in metres (true) or yards (false), independent of the speed unit.
    pub meters: bool,
    /// Log raw GSPro OpenConnect messages.
    pub debug: bool,
    /// Always-on-top status overlay for playing with GSPro in full screen.
    pub overlay: OverlayPos,
    /// Size, background and contents of the overlay.
    pub overlay_style: OverlayStyle,
    /// Ask GitHub for a newer release on start.
    pub check_updates: bool,
    /// Last main window size in logical points (at zoom 1), restored on start.
    pub window_size: Option<[f32; 2]>,
    /// Interface size: `None` fits the window (which keeps the base aspect ratio), otherwise a fixed
    /// zoom (1.0–1.5) and a freely sized window whose extra room the pages use.
    pub ui_scale: Option<f32>,
    /// macOS notifications for problems (battery, R10 errors, lost connections, undelivered shots).
    pub notifications: bool,
    /// UI language (the log stays English).
    pub language: Language,
    /// Last CSV layout chosen on the SHOTS page.
    pub export_format: ExportFormat,
    /// Pages and tiles of the shots explorer.
    pub explorer: Explorer,
}

/// The shots explorer: pages of tiles on a grid of square cells.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Explorer {
    pub pages: Vec<ExplorerPage>,
}

impl Default for Explorer {
    /// A page with the shots table and the details of a shot, the table on a page of its own,
    /// and the averages per club.
    fn default() -> Self {
        let tile = |kind, size, averages| Tile { kind, size, averages };
        Self {
            pages: vec![
                ExplorerPage {
                    name: String::new(),
                    tiles: vec![tile(TileKind::Table, TileSize::W4H2, false), tile(TileKind::Details, TileSize::W2H2, false)],
                },
                ExplorerPage { name: String::new(), tiles: vec![tile(TileKind::Table, TileSize::Page, false)] },
                ExplorerPage { name: String::new(), tiles: vec![tile(TileKind::Table, TileSize::Page, true)] },
            ],
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ExplorerPage {
    /// Name on the page's tab; empty shows "Page n".
    pub name: String,
    pub tiles: Vec<Tile>,
}

/// One tile: what it shows and how many cells it covers.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Tile {
    pub kind: TileKind,
    pub size: TileSize,
    /// Table: averages per club instead of single shots.
    pub averages: bool,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TileKind {
    #[default]
    Table,
    /// All values of the chosen (or latest) shot.
    Details,
}

impl TileKind {
    pub const ALL: [Self; 2] = [Self::Table, Self::Details];
}

/// Tile sizes in cells (width × height), like widgets on a phone, or the whole page.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TileSize {
    W1H1,
    W2H1,
    W1H2,
    #[default]
    W2H2,
    W3H2,
    W4H2,
    Page,
}

impl TileSize {
    pub const ALL: [Self; 7] = [Self::W1H1, Self::W2H1, Self::W1H2, Self::W2H2, Self::W3H2, Self::W4H2, Self::Page];

    /// Width and height in cells; `None` for the whole page.
    #[must_use]
    pub fn cells(self) -> Option<(usize, usize)> {
        match self {
            Self::W1H1 => Some((1, 1)),
            Self::W2H1 => Some((2, 1)),
            Self::W1H2 => Some((1, 2)),
            Self::W2H2 => Some((2, 2)),
            Self::W3H2 => Some((3, 2)),
            Self::W4H2 => Some((4, 2)),
            Self::Page => None,
        }
    }
}

/// What the overlay shows and how large it is. The status is always shown.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct OverlayStyle {
    /// Size factor on screen (0.75–1.5).
    pub scale: f32,
    /// Everything in one line instead of two.
    pub one_line: bool,
    /// Background opacity (0.7–1.0).
    pub opacity: f32,
    pub club: bool,
    pub battery: bool,
    pub tempo: bool,
    pub ball_speed: bool,
    pub vla: bool,
    pub hla: bool,
    pub spin: bool,
    pub spin_axis: bool,
    pub club_speed: bool,
    pub smash: bool,
}

impl Default for OverlayStyle {
    /// The overlay as it was before it could be configured.
    fn default() -> Self {
        Self {
            scale: 1.0,
            one_line: false,
            opacity: 0.85,
            club: true,
            battery: true,
            tempo: true,
            ball_speed: true,
            vla: true,
            hla: false,
            spin: true,
            spin_axis: false,
            club_speed: false,
            smash: true,
        }
    }
}

/// Where the overlay sits on the screen: a corner, the middle of an edge, or off.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OverlayPos {
    Off,
    TopLeft,
    Top,
    #[default]
    TopRight,
    Left,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl OverlayPos {
    /// Positions in screen order (rows top to bottom), `Off` in the middle.
    pub const GRID: [[Self; 3]; 3] = [
        [Self::TopLeft, Self::Top, Self::TopRight],
        [Self::Left, Self::Off, Self::Right],
        [Self::BottomLeft, Self::Bottom, Self::BottomRight],
    ];

    /// Placement as fractions of the free screen space (0 = left/top, 1 = right/bottom).
    #[must_use]
    pub fn anchor(self) -> Option<(f32, f32)> {
        Self::GRID.iter().enumerate().find_map(|(row, cells)| {
            let col = cells.iter().position(|&p| p == self)?;
            (self != Self::Off).then(|| (col as f32 / 2.0, row as f32 / 2.0))
        })
    }
}

impl Default for Settings {
    fn default() -> Self {
        let cfg = Config::default();
        Self {
            gspro_addr: cfg.gspro_addr,
            tee_distance: cfg.tee_distance,
            launcher_path: None,
            auto_start_gspro: false,
            device: None,
            metric: true,
            meters: true,
            debug: false,
            overlay: OverlayPos::default(),
            overlay_style: OverlayStyle::default(),
            export_format: ExportFormat::default(),
            language: Language::default(),
            notifications: true,
            check_updates: true,
            window_size: None,
            ui_scale: None,
            explorer: Explorer::default(),
        }
    }
}

impl Settings {
    fn dir(name: &str) -> Option<PathBuf> {
        let home = std::env::var_os("HOME")?;
        let mut p = PathBuf::from(home);
        if cfg!(target_os = "macos") {
            p.push("Library/Application Support");
            p.push(name);
        } else {
            p.push(".config");
            p.push(name.to_lowercase());
        }
        Some(p)
    }

    /// Directory for settings and shot history.
    #[must_use]
    pub fn data_dir() -> Option<PathBuf> {
        Self::dir("OpenLaunch Connect")
    }

    fn path() -> Option<PathBuf> {
        Some(Self::data_dir()?.join("settings.json"))
    }

    /// Move the data folder of OpenR10 Connect (the name until 0.7) to the new
    /// name, so settings and shot history carry over. Call once at startup.
    pub fn migrate_data_dir() {
        if let (Some(new), Some(old)) = (Self::data_dir(), Self::dir("OpenR10 Connect"))
            && !new.exists()
            && old.is_dir()
        {
            let _ = std::fs::rename(&old, &new);
        }
    }

    #[must_use]
    pub fn load() -> Self {
        let read = |p: Option<PathBuf>| {
            p.and_then(|p| std::fs::read(p).ok())
                .and_then(|b| serde_json::from_slice::<Self>(&b).ok())
        };
        // Fall back to settings of earlier builds (GSProR10Connect, R10 Bridge).
        read(Self::path())
            .or_else(|| read(Self::dir("GSProR10Connect").map(|d| d.join("settings.json"))))
            .or_else(|| read(Self::dir("R10 Bridge").map(|d| d.join("settings.json"))))
            .unwrap_or_default()
    }

    /// # Errors
    /// Returns a message if the file cannot be written.
    pub fn save(&self) -> Result<(), String> {
        let path = Self::path().ok_or("HOME not set")?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        // Write a copy and swap it in, so a crash never leaves half a settings file.
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json).and_then(|()| std::fs::rename(&tmp, &path)).map_err(|e| e.to_string())
    }

    #[must_use]
    pub fn to_config(&self) -> Config {
        Config {
            gspro_addr: self.gspro_addr.clone(),
            tee_distance: self.tee_distance,
            device: self.device.clone().filter(|d| !d.trim().is_empty()),
            demo: std::env::var_os("OPENLAUNCH_DEMO").is_some(),
            ..Config::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_anchors() {
        assert_eq!(OverlayPos::Off.anchor(), None);
        assert_eq!(OverlayPos::TopLeft.anchor(), Some((0.0, 0.0)));
        assert_eq!(OverlayPos::Right.anchor(), Some((1.0, 0.5)));
        assert_eq!(OverlayPos::Bottom.anchor(), Some((0.5, 1.0)));
    }

    #[test]
    fn old_settings_without_overlay_still_load() {
        let s: Settings = serde_json::from_str(r#"{"gspro_addr":"127.0.0.1:921","metric":false}"#).unwrap();
        assert_eq!(s.overlay, OverlayPos::TopRight);
        assert!(!s.metric);
        assert!(s.meters, "distances default to metres, whatever the speed unit");
        assert_eq!(s.explorer, Explorer::default(), "default explorer pages");
        let json = serde_json::to_string(&Settings { overlay: OverlayPos::BottomLeft, ..s }).unwrap();
        assert!(json.contains(r#""overlay":"bottom_left""#));
    }

    #[test]
    fn explorer_layout_round_trips() {
        let mut s = Settings::default();
        s.explorer.pages[0].name = "Range".into();
        s.explorer.pages[0].tiles.push(Tile { kind: TileKind::Details, size: TileSize::W1H2, averages: false });
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""size":"w1_h2""#) || json.contains(r#""size":"w1h2""#), "{json}");
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), s);
    }
}
