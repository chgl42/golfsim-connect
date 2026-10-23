//! Persistent app settings (~/Library/Application Support/OpenLaunch Connect/settings.json).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::engine::Config;
use crate::history::ExportFormat;
use crate::i18n::Language;

/// Name of the data folder (settings and shot history).
const DATA_DIR: &str = "OpenLaunch Connect";

/// Off in tests that drive the app, so they never overwrite the real settings file.
pub static SAVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

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
    #[serde(deserialize_with = "tolerant::or_default")]
    pub overlay: OverlayPos,
    /// Placed by hand: its top left corner as fractions of the room the screen leaves (overrides
    /// the place in `overlay`, which still turns it on or off).
    pub overlay_spot: Option<[f32; 2]>,
    /// Size, background and contents of the overlay.
    pub overlay_style: OverlayStyle,
    /// Overlays with one tile of the shots explorer each.
    #[serde(deserialize_with = "tolerant::list")]
    pub tile_overlays: Vec<TileOverlay>,
    /// Saved arrangements of all overlays; the one in use (`arrangement`) follows every change.
    #[serde(deserialize_with = "tolerant::list")]
    pub arrangements: Vec<Arrangement>,
    pub arrangement: usize,
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
    #[serde(deserialize_with = "tolerant::or_default")]
    pub language: Language,
    /// CSV layout chosen last (unused since the export asks each time; kept so the field still loads).
    #[serde(deserialize_with = "tolerant::or_default")]
    pub export_format: ExportFormat,
    /// Pages and tiles of the shots explorer.
    pub explorer: Explorer,
    /// Carry, total, apex and side from GSPro where it sent them (else the estimate).
    pub gspro_results: bool,
    /// Settings this version doesn't know (of a newer or extended build), written back unchanged.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// The shots explorer: pages of tiles on a grid of square cells.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Explorer {
    #[serde(deserialize_with = "tolerant::list")]
    pub pages: Vec<ExplorerPage>,
}

impl Default for Explorer {
    /// One row each: the range (table, dispersion, flights from above), the ball flight from the
    /// tee, the side and above, and the data (table and details of a shot). The names are shown
    /// translated (see `DEFAULT_PAGE_NAMES`).
    fn default() -> Self {
        let tile = |kind, size, camera| Tile { kind, size, camera, ..Tile::default() };
        let page = |name: &str, tiles| ExplorerPage { name: name.into(), tiles };
        Self {
            pages: vec![
                page(
                    DEFAULT_PAGE_NAMES[0],
                    vec![
                        tile(TileKind::Table, TileSize::W3H2, Camera::Tee),
                        tile(TileKind::Dispersion, TileSize::W2H2, Camera::Tee),
                        tile(TileKind::Flight, TileSize::W1H2, Camera::Above),
                    ],
                ),
                page(
                    DEFAULT_PAGE_NAMES[1],
                    vec![
                        tile(TileKind::Flight, TileSize::W2H2, Camera::Tee),
                        tile(TileKind::Flight, TileSize::W3H2, Camera::Side),
                        tile(TileKind::Flight, TileSize::W1H2, Camera::Above),
                    ],
                ),
                page(
                    DEFAULT_PAGE_NAMES[2],
                    vec![tile(TileKind::Table, TileSize::W4H2, Camera::Tee), tile(TileKind::Details, TileSize::W2H2, Camera::Tee)],
                ),
            ],
        }
    }
}

/// Names of the default pages, in English; the app shows them translated.
pub const DEFAULT_PAGE_NAMES: [&str; 3] = ["Range", "Ball flight", "Data"];

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ExplorerPage {
    /// Name on the page's tab; empty shows "Page n".
    pub name: String,
    #[serde(deserialize_with = "tolerant::list")]
    pub tiles: Vec<Tile>,
}

/// One tile: what it shows and how many cells it covers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Tile {
    pub kind: TileKind,
    pub size: TileSize,
    /// Table: averages per club instead of single shots.
    pub averages: bool,
    /// Dispersion: where the ball comes to rest instead of where it lands.
    pub total: bool,
    /// Scatter: values on the horizontal and vertical axis.
    pub x: Metric,
    pub y: Metric,
    /// 3D flight: where the camera looks from.
    pub camera: Camera,
}

/// Camera of a 3D flight tile.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Camera {
    /// Behind the tee, looking down the range.
    #[default]
    Tee,
    /// From the side: height over distance.
    Side,
    /// From above and behind, at an angle.
    Above,
}

impl Camera {
    pub const ALL: [Self; 3] = [Self::Tee, Self::Side, Self::Above];
}

impl Default for Tile {
    fn default() -> Self {
        Self {
            kind: TileKind::Table,
            size: TileSize::W2H2,
            averages: false,
            total: false,
            x: Metric::Hla,
            y: Metric::SpinAxis,
            camera: Camera::Tee,
        }
    }
}

/// A value of a shot that a scatter tile can plot.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    BallSpeed,
    ClubSpeed,
    Smash,
    Vla,
    Hla,
    Spin,
    SpinAxis,
    AngleOfAttack,
    Path,
    FaceToTarget,
    FaceToPath,
    Carry,
    Total,
    Side,
    Apex,
    Tempo,
}

impl Metric {
    pub const ALL: [Self; 16] = [
        Self::BallSpeed,
        Self::ClubSpeed,
        Self::Smash,
        Self::Vla,
        Self::Hla,
        Self::Spin,
        Self::SpinAxis,
        Self::AngleOfAttack,
        Self::Path,
        Self::FaceToTarget,
        Self::FaceToPath,
        Self::Carry,
        Self::Total,
        Self::Side,
        Self::Apex,
        Self::Tempo,
    ];
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TileKind {
    #[default]
    Table,
    /// All values of the chosen (or latest) shot.
    Details,
    /// Landing points from above, with mean and spread per club.
    Dispersion,
    /// Two values against each other.
    Scatter,
    /// The estimated flights in 3D.
    Flight,
}

impl TileKind {
    pub const ALL: [Self; 5] = [Self::Table, Self::Details, Self::Dispersion, Self::Scatter, Self::Flight];
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
    /// The overlay's tiles: columns of two (top, bottom). The first column starts with the status,
    /// which can't be moved. One line shows them in this order.
    #[serde(deserialize_with = "tolerant::layout")]
    pub layout: Vec<[Option<OverlayItem>; 2]>,
}

impl Default for OverlayStyle {
    fn default() -> Self {
        Self {
            scale: 1.0,
            one_line: false,
            opacity: 0.85,
            layout: OverlayItem::default_layout(),
        }
    }
}

/// Something the overlay can show in one of its tiles.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OverlayItem {
    Status,
    Club,
    BallSpeed,
    ClubSpeed,
    Smash,
    Spin,
    Vla,
    Hla,
    SpinAxis,
    Tempo,
    /// Distance left to the pin after the shot (from GSPro's results).
    Pin,
    Battery,
    /// Graphic of the impact from the side: angle of attack and launch angle (a whole column).
    ImpactSide,
    /// Graphic from above: club path, start direction and the shot's shape (a whole column).
    ImpactTop,
}

impl OverlayItem {
    /// What can be put into a tile (the status has its fixed place).
    pub const CHOICES: [Self; 13] = [
        Self::Club,
        Self::BallSpeed,
        Self::ClubSpeed,
        Self::Smash,
        Self::Spin,
        Self::Vla,
        Self::Hla,
        Self::SpinAxis,
        Self::Tempo,
        Self::Pin,
        Self::Battery,
        Self::ImpactSide,
        Self::ImpactTop,
    ];

    /// The impact graphics take a whole column.
    #[must_use]
    pub fn is_graphic(self) -> bool {
        matches!(self, Self::ImpactSide | Self::ImpactTop)
    }

    /// Status and club, the speeds, smash and spin, the launch angles, pin and battery, then the
    /// impact from the side and from above.
    #[must_use]
    pub fn default_layout() -> Vec<[Option<Self>; 2]> {
        vec![
            [Some(Self::Status), Some(Self::Club)],
            [Some(Self::BallSpeed), Some(Self::ClubSpeed)],
            [Some(Self::Smash), Some(Self::Spin)],
            [Some(Self::Vla), Some(Self::Hla)],
            [Some(Self::Pin), Some(Self::Battery)],
            [Some(Self::ImpactSide), None],
            [Some(Self::ImpactTop), None],
        ]
    }
}

/// The overlays as they were arranged, under a name: the status overlay and the tiles.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Arrangement {
    pub name: String,
    pub overlay: OverlayPos,
    pub overlay_spot: Option<[f32; 2]>,
    pub overlay_style: OverlayStyle,
    #[serde(deserialize_with = "tolerant::list")]
    pub tiles: Vec<TileOverlay>,
}

impl Default for Arrangement {
    fn default() -> Self {
        Self {
            name: DEFAULT_ARRANGEMENT.into(),
            overlay: OverlayPos::default(),
            overlay_spot: None,
            overlay_style: OverlayStyle::default(),
            tiles: Vec::new(),
        }
    }
}

/// Name of the first arrangement, in English; the app shows it translated.
pub const DEFAULT_ARRANGEMENT: &str = "Standard";

impl Settings {
    /// The overlays as they are now, as the arrangement in use (the first one if there is none).
    pub fn keep_arrangement(&mut self) {
        let name = self.arrangements.get(self.arrangement).map_or_else(|| DEFAULT_ARRANGEMENT.to_string(), |a| a.name.clone());
        let now = Arrangement {
            name,
            overlay: self.overlay,
            overlay_spot: self.overlay_spot,
            overlay_style: self.overlay_style.clone(),
            tiles: self.tile_overlays.clone(),
        };
        if self.arrangements.is_empty() {
            self.arrangement = 0;
            self.arrangements.push(now);
        } else {
            self.arrangement = self.arrangement.min(self.arrangements.len() - 1);
            self.arrangements[self.arrangement] = now;
        }
    }

    /// Shows the overlays as arrangement `i` has them.
    pub fn use_arrangement(&mut self, i: usize) {
        let Some(a) = self.arrangements.get(i).cloned() else { return };
        self.arrangement = i;
        self.overlay = a.overlay;
        self.overlay_spot = a.overlay_spot;
        self.overlay_style = a.overlay_style;
        self.tile_overlays = a.tiles;
    }
}

/// An overlay with one tile of the shots explorer, e.g. the dispersion or the flights in 3D.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct TileOverlay {
    /// Where it sits; `Off` hides it (the tile is kept).
    pub pos: OverlayPos,
    /// Placed by hand (like `Settings::overlay_spot`).
    pub spot: Option<[f32; 2]>,
    /// What it shows (its size in cells is unused).
    pub tile: Tile,
    /// Width in points; the height is three quarters of it.
    pub width: f32,
    /// Background opacity (0.7–1.0).
    pub opacity: f32,
    /// Only the shots with the club chosen in GSPro, else all of the session.
    pub club_only: bool,
}

impl Default for TileOverlay {
    fn default() -> Self {
        Self {
            pos: OverlayPos::Left,
            spot: None,
            tile: Tile { kind: TileKind::Dispersion, ..Tile::default() },
            width: 360.0,
            opacity: 0.85,
            club_only: false,
        }
    }
}

impl TileOverlay {
    /// The widths to choose from (small, medium, large).
    pub const WIDTHS: [f32; 3] = [280.0, 360.0, 460.0];

    #[must_use]
    pub fn height(&self) -> f32 {
        (self.width * 0.75).round()
    }
}

/// Where the overlay sits on the screen: a corner, the middle of an edge, or off.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
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
            overlay_spot: None,
            overlay_style: OverlayStyle::default(),
            tile_overlays: Vec::new(),
            arrangements: Vec::new(),
            arrangement: 0,
            export_format: ExportFormat::default(),
            language: Language::default(),
            notifications: true,
            check_updates: true,
            window_size: None,
            ui_scale: None,
            explorer: Explorer::default(),
            gspro_results: true,
            extra: Map::new(),
        }
    }
}

impl Settings {
    fn dir(name: &str) -> Option<PathBuf> {
        Some(Self::dir_in(Path::new(&std::env::var_os("HOME")?), name))
    }

    /// The data folder `name` under the home folder `home` (a parameter so tests can use a temp dir).
    fn dir_in(home: &Path, name: &str) -> PathBuf {
        let mut p = home.to_path_buf();
        if cfg!(target_os = "macos") {
            p.push("Library/Application Support");
            p.push(name);
        } else {
            p.push(".config");
            p.push(name.to_lowercase());
        }
        p
    }

    /// Directory for settings and shot history.
    #[must_use]
    pub fn data_dir() -> Option<PathBuf> {
        Self::dir(DATA_DIR)
    }

    /// Move the data folder of OpenR10 Connect (the name until 0.7) to the new
    /// name, so settings and shot history carry over. Call once at startup.
    pub fn migrate_data_dir() {
        if let Some(home) = std::env::var_os("HOME") {
            Self::migrate_data_dir_in(Path::new(&home));
        }
    }

    fn migrate_data_dir_in(home: &Path) {
        let (new, old) = (Self::dir_in(home, DATA_DIR), Self::dir_in(home, "OpenR10 Connect"));
        if !new.exists() && old.is_dir() {
            let _ = std::fs::rename(&old, &new);
        }
    }

    #[must_use]
    pub fn load() -> Self {
        std::env::var_os("HOME").map_or_else(Self::default, |home| Self::load_in(Path::new(&home)))
    }

    /// The settings under the home folder `home`; the defaults if there are none or they can't be read.
    /// Values this version doesn't know are left out (see `tolerant`); a file that can't be read at
    /// all is kept as `settings.json.bad`, so the next save doesn't lose it.
    fn load_in(home: &Path) -> Self {
        let own = Self::dir_in(home, DATA_DIR).join("settings.json");
        if let Ok(bytes) = std::fs::read(&own) {
            match serde_json::from_slice::<Self>(&bytes) {
                Ok(s) => return s,
                Err(e) => {
                    let bad = own.with_extension("json.bad");
                    let kept = match std::fs::write(&bad, &bytes) {
                        Ok(()) => format!("kept as {}", bad.display()),
                        Err(e) => format!("no copy kept ({e})"),
                    };
                    crate::log::info(&format!("Settings can't be read ({e}), {kept}; using the defaults"));
                }
            }
        }
        // Fall back to settings of earlier builds (GSProR10Connect, R10 Bridge).
        let read = |name: &str| {
            std::fs::read(Self::dir_in(home, name).join("settings.json"))
                .ok()
                .and_then(|b| serde_json::from_slice::<Self>(&b).ok())
        };
        read("GSProR10Connect").or_else(|| read("R10 Bridge")).unwrap_or_default()
    }

    /// # Errors
    /// Returns a message if the file cannot be written.
    pub fn save(&self) -> Result<(), String> {
        if !SAVE.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(());
        }
        let home = std::env::var_os("HOME").ok_or("HOME not set")?;
        self.save_in(Path::new(&home))
    }

    /// Writes the settings under the home folder `home` (see [`Settings::save`]).
    fn save_in(&self, home: &Path) -> Result<(), String> {
        let path = Self::dir_in(home, DATA_DIR).join("settings.json");
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

/// Reading settings that a newer or extended build wrote: a value this version doesn't know (e.g.
/// a tile kind) leaves out only what holds it, with a log line, instead of failing the whole file.
mod tolerant {
    use serde::de::DeserializeOwned;
    use serde::{Deserialize, Deserializer};
    use serde_json::Value;

    /// `value` as a `T`, or `None` (logged) if this version can't read it.
    fn read<T: DeserializeOwned>(value: Value) -> Option<T> {
        match T::deserialize(&value) {
            Ok(v) => Some(v),
            Err(e) => {
                crate::log::info(&format!("Settings: left out {value} ({e})"));
                None
            }
        }
    }

    /// A list without the entries this version can't read.
    pub fn list<'de, D: Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<Vec<T>, D::Error> {
        Ok(Vec::<Value>::deserialize(d)?.into_iter().filter_map(read).collect())
    }

    /// The value, or the default if this version can't read it.
    pub fn or_default<'de, D: Deserializer<'de>, T: DeserializeOwned + Default>(d: D) -> Result<T, D::Error> {
        Ok(read(Value::deserialize(d)?).unwrap_or_default())
    }

    /// The overlay's columns: an item this version doesn't know leaves its place empty.
    pub fn layout<'de, D: Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<Vec<[Option<T>; 2]>, D::Error> {
        let columns: Vec<[Value; 2]> = list(d)?;
        Ok(columns.into_iter().map(|c| c.map(|v| read(v).flatten())).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_layout_defaults_and_old_switches_are_ignored() {
        let old = r#"{"scale":1.25,"club":true,"battery":false,"smash":true,"to_pin":true}"#;
        let style: OverlayStyle = serde_json::from_str(old).unwrap();
        assert_eq!(style.scale, 1.25);
        assert_eq!(style.layout, OverlayItem::default_layout());
        assert_eq!(style.layout[0][0], Some(OverlayItem::Status));
        let json = serde_json::to_string(&style).unwrap();
        assert!(json.contains(r#"["ball_speed","club_speed"]"#), "{json}");
    }

    #[test]
    fn arrangements_follow_the_overlays() {
        let mut s = Settings::default();
        s.keep_arrangement();
        assert_eq!(s.arrangements.len(), 1);
        assert_eq!(s.arrangements[0].name, DEFAULT_ARRANGEMENT);
        // A second one with a tile; changes go to the one in use.
        s.arrangements.push(Arrangement { name: "Range".into(), ..Arrangement::default() });
        s.use_arrangement(1);
        s.tile_overlays.push(TileOverlay::default());
        s.overlay = OverlayPos::Bottom;
        s.keep_arrangement();
        assert_eq!(s.arrangements[1].tiles.len(), 1);
        assert_eq!(s.arrangements[1].name, "Range");
        // Back to the first: its overlays again.
        s.use_arrangement(0);
        assert!(s.tile_overlays.is_empty());
        assert_eq!(s.overlay, OverlayPos::TopRight);
        s.use_arrangement(1);
        assert_eq!((s.overlay, s.tile_overlays.len()), (OverlayPos::Bottom, 1));
    }

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
        s.explorer.pages[0].tiles.push(Tile { kind: TileKind::Details, size: TileSize::W1H2, ..Tile::default() });
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""size":"w1_h2""#) || json.contains(r#""size":"w1h2""#), "{json}");
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), s);
    }

    /// Writes `json` as the settings of the data folder `name` under `home`.
    fn write_in(home: &Path, name: &str, json: &str) {
        let dir = Settings::dir_in(home, name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.json"), json).unwrap();
    }

    #[test]
    fn settings_round_trip_through_the_file() {
        let home = crate::test_util::TempDir::new("settings-round-trip");
        let mut s = Settings { metric: false, gspro_addr: "127.0.0.1:9210".into(), ui_scale: Some(1.25), ..Settings::default() };
        s.tile_overlays.push(TileOverlay { pos: OverlayPos::BottomRight, club_only: true, ..TileOverlay::default() });
        s.keep_arrangement();
        s.save_in(home.path()).unwrap();
        let file = Settings::dir_in(home.path(), DATA_DIR).join("settings.json");
        assert!(file.is_file(), "written to {}", file.display());
        assert!(!file.with_extension("json.tmp").exists(), "the temporary copy is renamed");
        assert_eq!(Settings::load_in(home.path()), s);
    }

    #[test]
    fn nothing_saved_loads_the_defaults() {
        let home = crate::test_util::TempDir::new("settings-none");
        assert_eq!(Settings::load_in(home.path()), Settings::default());
    }

    /// Settings of an early 1.x: no explorer, tile overlays, arrangements, language, interface size
    /// or notifications yet, and the overlay's old on/off switches.
    #[test]
    fn settings_of_an_older_version_load() {
        let home = crate::test_util::TempDir::new("settings-old");
        write_in(home.path(), DATA_DIR, crate::fixture!("settings-old.json"));
        let s = Settings::load_in(home.path());
        assert_ne!(s, Settings::default(), "the file was read");
        assert_eq!(s.gspro_addr, "127.0.0.1:9210");
        assert_eq!(s.tee_distance, 2.0);
        assert!(s.auto_start_gspro && s.debug && !s.metric && !s.meters && !s.check_updates);
        assert_eq!(s.device.as_deref(), Some("Approach R10"));
        assert_eq!(s.overlay, OverlayPos::BottomLeft);
        assert_eq!(s.window_size, Some([1200.0, 564.0]));
        assert_eq!(s.export_format, ExportFormat::GarminGolf);
        assert_eq!((s.overlay_style.scale, s.overlay_style.one_line), (1.25, true));
        // What the file doesn't have yet comes from the defaults.
        let d = Settings::default();
        assert_eq!(s.overlay_style.layout, OverlayItem::default_layout());
        assert_eq!(s.overlay_style.opacity, d.overlay_style.opacity);
        assert_eq!(s.explorer, d.explorer);
        assert!(s.tile_overlays.is_empty() && s.arrangements.is_empty());
        assert_eq!((s.ui_scale, s.language, s.notifications, s.gspro_results), (None, d.language, true, true));
        assert!(s.extra.is_empty(), "every key of the old file is known: {:?}", s.extra);
    }

    /// Fields of a newer version are kept aside at the top level and ignored further in; the rest loads.
    #[test]
    fn unknown_fields_are_ignored() {
        let s: Settings = serde_json::from_str(r#"{"metric":false,"future_switch":true,"overlay_style":{"glow":3}}"#).unwrap();
        assert!(!s.metric);
        assert_eq!(s.overlay_style, OverlayStyle::default());
        assert_eq!(s.extra.get("future_switch"), Some(&Value::Bool(true)));
    }

    /// Without settings of its own, the app takes those of the earlier names: GSProR10Connect
    /// first, then R10 Bridge. Its own win once there are any.
    #[test]
    fn settings_of_the_earlier_names_are_a_fallback() {
        let home = crate::test_util::TempDir::new("settings-legacy");
        write_in(home.path(), "R10 Bridge", r#"{"gspro_addr":"bridge:921"}"#);
        assert_eq!(Settings::load_in(home.path()).gspro_addr, "bridge:921");
        write_in(home.path(), "GSProR10Connect", r#"{"gspro_addr":"gspror10:921"}"#);
        assert_eq!(Settings::load_in(home.path()).gspro_addr, "gspror10:921");
        write_in(home.path(), DATA_DIR, r#"{"gspro_addr":"own:921"}"#);
        assert_eq!(Settings::load_in(home.path()).gspro_addr, "own:921");
        // An unreadable file of its own falls back to the earlier ones too (today's behaviour).
        write_in(home.path(), DATA_DIR, "{ not json");
        assert_eq!(Settings::load_in(home.path()).gspro_addr, "gspror10:921");
    }

    /// The OpenR10 Connect folder (the name until 0.7) is moved to the new name, unless that exists.
    #[test]
    fn data_folder_of_openr10_connect_is_moved() {
        let home = crate::test_util::TempDir::new("settings-migrate");
        write_in(home.path(), "OpenR10 Connect", r#"{"metric":false}"#);
        Settings::migrate_data_dir_in(home.path());
        assert!(!Settings::dir_in(home.path(), "OpenR10 Connect").exists());
        assert!(!Settings::load_in(home.path()).metric);
        // Once the new folder is there, an old one is left alone.
        write_in(home.path(), "OpenR10 Connect", r#"{"metric":true}"#);
        Settings::migrate_data_dir_in(home.path());
        assert!(Settings::dir_in(home.path(), "OpenR10 Connect").exists());
        assert!(!Settings::load_in(home.path()).metric);
    }

    /// One value this version doesn't know (a tile kind or overlay item of a newer version, or a
    /// typo) drops only what holds it; the rest survives (bug #70: all fell back to the defaults).
    #[test]
    fn one_unknown_value_keeps_the_other_settings() {
        let mut s = Settings { metric: false, gspro_addr: "127.0.0.1:9210".into(), ..Settings::default() };
        s.overlay_style.layout[1][0] = Some(OverlayItem::Tempo);
        let json = serde_json::to_string(&s).unwrap();
        let tile = json.replacen(r#""kind":"table""#, r#""kind":"gapping""#, 1);
        let item = json.replacen(r#""tempo""#, r#""launch_direction""#, 1);
        assert_ne!(tile, json);
        assert_ne!(item, json);
        let mut loaded = Vec::new();
        for json in [tile, item] {
            let home = crate::test_util::TempDir::new("settings-unknown");
            write_in(home.path(), DATA_DIR, &json);
            let l = Settings::load_in(home.path());
            assert_eq!(l.gspro_addr, "127.0.0.1:9210", "{json}");
            assert!(!l.metric, "{json}");
            assert!(!Settings::dir_in(home.path(), DATA_DIR).join("settings.json.bad").exists(), "no copy needed");
            loaded.push(l);
        }
        // Only the tile with the unknown kind is gone, only the unknown item's place is empty.
        assert_eq!(loaded[0].explorer.pages[0].tiles, s.explorer.pages[0].tiles[1..]);
        assert_eq!(loaded[0].overlay_style, s.overlay_style);
        assert_eq!(loaded[1].overlay_style.layout[1], [None, Some(OverlayItem::ClubSpeed)]);
        assert_eq!(loaded[1].explorer, s.explorer);
    }

    /// An unknown tile kind takes only its tile with it, in the explorer and in the tile overlays
    /// (of the arrangements, too); an unknown overlay position or language is the default.
    #[test]
    fn unknown_values_drop_only_their_entry() {
        let mut s = Settings { tile_overlays: vec![TileOverlay::default(), TileOverlay::default()], ..Settings::default() };
        s.keep_arrangement();
        let mut v = serde_json::to_value(&s).unwrap();
        v["explorer"]["pages"][0]["tiles"][1]["kind"] = "gapping".into();
        v["tile_overlays"][0]["tile"]["kind"] = "gapping".into();
        v["arrangements"][0]["tiles"][1]["pos"] = "center".into();
        v["overlay"] = "center".into();
        v["language"] = "klingon".into();
        let loaded: Settings = serde_json::from_value(v).unwrap();
        let kinds: Vec<TileKind> = loaded.explorer.pages[0].tiles.iter().map(|t| t.kind).collect();
        assert_eq!(kinds, [TileKind::Table, TileKind::Flight]);
        assert_eq!(loaded.explorer.pages[1..], s.explorer.pages[1..]);
        assert_eq!((loaded.tile_overlays.len(), loaded.arrangements[0].tiles.len()), (1, 1));
        assert_eq!((loaded.overlay, loaded.language), (OverlayPos::default(), Language::default()));
    }

    /// An overlay item this version doesn't know leaves its place empty; the columns stay.
    #[test]
    fn unknown_overlay_item_leaves_its_place_empty() {
        let json = r#"{"layout":[["status","club"],["launch_direction","ball_speed"],["hologram"]]}"#;
        let style: OverlayStyle = serde_json::from_str(json).unwrap();
        let (status, club, ball) = (OverlayItem::Status, OverlayItem::Club, OverlayItem::BallSpeed);
        assert_eq!(style.layout, [[Some(status), Some(club)], [None, Some(ball)]], "a column that isn't one is left out");
    }

    /// Settings of a newer or extended build keep what this version doesn't know when it saves.
    #[test]
    fn unknown_settings_survive_a_save() {
        let home = crate::test_util::TempDir::new("settings-extra");
        write_in(home.path(), DATA_DIR, r#"{"metric":false,"swing_video":{"camera":2,"fps":120},"putting":true}"#);
        let mut s = Settings::load_in(home.path());
        assert!(!s.metric);
        assert_eq!(s.extra.len(), 2);
        s.meters = false;
        s.save_in(home.path()).unwrap();
        let file = Settings::dir_in(home.path(), DATA_DIR).join("settings.json");
        let saved: Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
        assert_eq!(saved["swing_video"], serde_json::json!({"camera":2,"fps":120}));
        assert_eq!((&saved["putting"], &saved["meters"]), (&Value::Bool(true), &Value::Bool(false)));
        assert_eq!(Settings::load_in(home.path()), s);
    }

    /// Today's settings are written as they were before unknown values were kept.
    #[test]
    fn valid_settings_are_written_unchanged() {
        let mut s = Settings { metric: false, ..Settings::default() };
        s.tile_overlays.push(TileOverlay::default());
        s.keep_arrangement();
        let json = serde_json::to_string_pretty(&s).unwrap();
        let again: Settings = serde_json::from_str(&json).unwrap();
        assert!(again.extra.is_empty());
        assert_eq!(serde_json::to_string_pretty(&again).unwrap(), json);
    }

    /// A settings file that can't be read at all (cut off) is kept as settings.json.bad, replacing
    /// an older copy, before the app starts with the defaults.
    #[test]
    fn unreadable_settings_are_kept_as_bad() {
        let home = crate::test_util::TempDir::new("settings-bad");
        let dir = Settings::dir_in(home.path(), DATA_DIR);
        let bad = dir.join("settings.json.bad");
        write_in(home.path(), DATA_DIR, r#"{"metric":false,"gspro_ad"#);
        assert_eq!(Settings::load_in(home.path()), Settings::default());
        assert_eq!(std::fs::read_to_string(&bad).unwrap(), r#"{"metric":false,"gspro_ad"#);
        write_in(home.path(), DATA_DIR, "{ not json");
        assert_eq!(Settings::load_in(home.path()), Settings::default());
        assert_eq!(std::fs::read_to_string(&bad).unwrap(), "{ not json", "the older copy is replaced");
        // Saving writes the settings anew; the copy stays.
        Settings::default().save_in(home.path()).unwrap();
        assert_eq!(Settings::load_in(home.path()), Settings::default());
        assert!(bad.is_file());
    }
}
