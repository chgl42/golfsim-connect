//! Persistent app settings (~/Library/Application Support/OpenLaunch Connect/settings.json).

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::engine::Config;
use crate::history::ExportFormat;
use crate::i18n::Language;

/// Name of the data folder (settings and shot history).
pub(crate) const DATA_DIR: &str = "OpenLaunch Connect";

/// Earlier names of the data folder, in the order they are tried (see `migrate_data_dir_in`).
const OLD_DATA_DIRS: [&str; 1] = ["OpenR10 Connect"];

/// The shot history in the data folder; moved last, so the data folder only has it once the rest is there.
const HISTORY_FILE: &str = "shots.jsonl";

/// In the data folder while a move from an earlier one is under way (see `migrate_data_dir_in`).
const PENDING_FILE: &str = "migration.pending";

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
    /// UI language: `"system"` or a language code (the log stays English). Older builds' names
    /// read as codes; a code this build doesn't have is kept and follows the system.
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
        let flight = |size, camera: &str| Tile::new("flight", size).with(&serde_json::json!({ "camera": camera }));
        let page = |name: &str, tiles| ExplorerPage { name: name.into(), tiles };
        Self {
            pages: vec![
                page(
                    DEFAULT_PAGE_NAMES[0],
                    vec![Tile::new("table", TileSize::W3H2), Tile::new("dispersion", TileSize::W2H2), flight(TileSize::W1H2, "above")],
                ),
                page(
                    DEFAULT_PAGE_NAMES[1],
                    vec![flight(TileSize::W2H2, "tee"), flight(TileSize::W3H2, "side"), flight(TileSize::W1H2, "above")],
                ),
                page(DEFAULT_PAGE_NAMES[2], vec![Tile::new("table", TileSize::W4H2), Tile::new("details", TileSize::W2H2)]),
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

/// One tile: its kind, how many cells it covers and the kind's own settings. The kinds are in
/// the UI (`ui::explorer::TileKind`); here a tile is data, so a kind this build doesn't have is
/// kept as it is.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Tile {
    /// Id of the tile's kind, e.g. `"table"`, `"dispersion"` or `"flight"`.
    pub kind: String,
    pub size: TileSize,
    /// The kind's settings, next to `kind` and `size` in the file (e.g. `"averages": true`). Kept
    /// as they are, with those of the kinds the tile was before and of kinds this build doesn't
    /// have; read and changed with [`Tile::config`] and [`Tile::change_config`].
    #[serde(flatten)]
    pub config: Map<String, Value>,
}

impl Default for Tile {
    fn default() -> Self {
        Self::new("table", TileSize::W2H2)
    }
}

impl Tile {
    /// A tile of the kind `kind` with its kind's default settings.
    #[must_use]
    pub fn new(kind: &str, size: TileSize) -> Self {
        Self { kind: kind.into(), size, config: Map::new() }
    }

    /// The kind's settings as its own type `T` (e.g. a struct with `#[serde(default)]`): what the
    /// tile doesn't have is `T`'s default, and settings it can't read are all of `T`'s defaults.
    #[must_use]
    pub fn config<T: DeserializeOwned + Default>(&self) -> T {
        serde_json::from_value(Value::Object(self.config.clone())).unwrap_or_default()
    }

    /// Changes the kind's settings, read as `T` (see [`Tile::config`]), with `change`; writes them
    /// back only if they changed, keeping the tile's other settings.
    pub fn change_config<T: DeserializeOwned + Serialize + Default + PartialEq>(&mut self, change: impl FnOnce(&mut T)) {
        let mut config: T = self.config();
        change(&mut config);
        if config != self.config() {
            self.set_config(&config);
        }
    }

    /// The tile with the fields of `config` (anything that serializes to an object) as settings.
    #[must_use]
    pub fn with(mut self, config: &impl Serialize) -> Self {
        self.set_config(config);
        self
    }

    fn set_config(&mut self, config: &impl Serialize) {
        if let Ok(Value::Object(fields)) = serde_json::to_value(config) {
            self.config.extend(fields);
        }
    }
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
            tile: Tile::new("dispersion", TileSize::W2H2),
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
    pub(crate) fn dir_in(home: &Path, name: &str) -> PathBuf {
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

    /// Moves the data folder of an earlier name into the data folder, so settings and shot history
    /// carry over. The first earlier folder that exists moves while the data folder has no
    /// `shots.jsonl` or a move into it is pending (`migration.pending`, written before the move and
    /// removed once every entry is in); otherwise the data folder isn't changed. Entries move one by
    /// one with `shots.jsonl` last. Where both folders hold an entry of the same name, the earlier
    /// one wins and the other is kept as `<name>.before-move` – except when a pending move goes on:
    /// then an entry of the data folder changed after the move began (later than the marker) stays
    /// and the earlier one is kept as `<name>.before-move`, and two histories are joined, the
    /// earlier lines first, once. Lock files are never moved. Only call it with the data folder claimed ([`Settings::claim_data_dir`]), so no
    /// other instance writes meanwhile.
    pub(crate) fn migrate_data_dir_in(home: &Path) {
        let new = Self::dir_in(home, DATA_DIR);
        let pending = new.join(PENDING_FILE);
        let Some((name, old)) =
            OLD_DATA_DIRS.iter().map(|name| (*name, Self::dir_in(home, name))).find(|(_, dir)| dir.is_dir())
        else {
            // The earlier folder is gone (moved, or removed by hand): nothing is pending any more.
            let _ = std::fs::remove_file(&pending);
            return;
        };
        // When a pending move began; what the data folder changed after that is the player's (a
        // marker without a time: all of it).
        let since = pending.exists().then(|| {
            std::fs::metadata(&pending).and_then(|m| m.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        });
        let resume = since.is_some();
        if !resume && new.join(HISTORY_FILE).exists() {
            crate::log::info(&format!("Data folder: \"{DATA_DIR}\" has a shot history, \"{name}\" left as it is"));
            return;
        }
        let moved = std::fs::create_dir_all(&new)
            .and_then(|()| if resume { Ok(()) } else { std::fs::write(&pending, "") })
            .and_then(|()| move_entries(&old, &new, since));
        match moved {
            Ok(n) => {
                // A lock file was never ours to move; with it gone the old folder can go too.
                let _ = std::fs::remove_file(old.join(crate::instance::LOCK_FILE));
                let gone = std::fs::remove_dir(&old).is_ok();
                let _ = std::fs::remove_file(&pending);
                let rest = if gone { "" } else { ", the old folder is not empty" };
                let again = if resume { " (a move cut short, finished)" } else { "" };
                crate::log::info(&format!("Data folder: moved {n} entries from \"{name}\" to \"{DATA_DIR}\"{again}{rest}"));
            }
            Err(e) => crate::log::info(&format!(
                "Data folder: moving \"{name}\" to \"{DATA_DIR}\" stopped ({e}), it goes on at the next start"
            )),
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

/// Moves every entry of `old` into `new` by name, `shots.jsonl` last, lock and marker files not at
/// all. An entry of the same name in `new` is kept as `<name>.before-move`. When a pending move
/// goes on (`since`: when it began), an entry of `new` changed after that stays and the old one is
/// kept as `<name>.before-move` instead, and the histories are joined. The number moved.
fn move_entries(old: &Path, new: &Path, since: Option<std::time::SystemTime>) -> std::io::Result<usize> {
    let mut names: Vec<std::ffi::OsString> = std::fs::read_dir(old)?
        .map(|entry| entry.map(|e| e.file_name()))
        .collect::<std::io::Result<_>>()?;
    names.retain(|name| name != crate::instance::LOCK_FILE && name != PENDING_FILE);
    names.sort_by(|a, b| (a == HISTORY_FILE, a).cmp(&(b == HISTORY_FILE, b)));
    for name in &names {
        let (from, target) = (old.join(name), new.join(name));
        let mut kept = name.clone();
        kept.push(".before-move");
        if target.symlink_metadata().is_err() {
            std::fs::rename(&from, &target)?;
        } else if since.is_some() && name == HISTORY_FILE {
            join_history(&from, &target)?;
        } else if since.is_some_and(|since| changed_after(&target, since)) {
            std::fs::rename(&from, new.join(kept))?;
        } else {
            std::fs::rename(&target, new.join(kept))?;
            std::fs::rename(&from, &target)?;
        }
    }
    Ok(names.len())
}

/// Whether `path` was changed after `since`; `false` if its time is unknown.
fn changed_after(path: &Path, since: std::time::SystemTime) -> bool {
    std::fs::symlink_metadata(path).and_then(|m| m.modified()).is_ok_and(|t| t > since)
}

/// Joins the earlier history `old` and the data folder's `target` into `target`, the earlier lines
/// first, then removes `old`. Written next to `target`, synced and renamed over it, so `target` is
/// never half written. A `target` that already starts with `old` was joined before (a stop between
/// the rename and the removal): then only `old` goes, so the lines are never there twice.
fn join_history(old: &Path, target: &Path) -> std::io::Result<()> {
    use std::io::Write as _;
    let mut joined = std::fs::read(old)?;
    if !joined.is_empty() && !joined.ends_with(b"\n") {
        joined.push(b'\n');
    }
    let ours = std::fs::read(target)?;
    if !joined.is_empty() && ours.starts_with(&joined) {
        return std::fs::remove_file(old);
    }
    joined.extend(ours);
    let tmp = target.with_extension("jsonl.joining");
    let mut file = std::fs::File::create(&tmp)?;
    file.write_all(&joined)?;
    file.sync_all()?;
    std::fs::rename(&tmp, target)?;
    std::fs::remove_file(old)
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
        s.explorer.pages[0].tiles.push(Tile::new("details", TileSize::W1H2));
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

    /// A file `name` with `text` in the data folder `dir` of `home`.
    fn put(home: &Path, dir: &str, name: &str, text: &str) {
        let dir = Settings::dir_in(home, dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(name), text).unwrap();
    }

    fn read(home: &Path, dir: &str, name: &str) -> Option<String> {
        std::fs::read_to_string(Settings::dir_in(home, dir).join(name)).ok()
    }

    const OLD: &str = "OpenR10 Connect";
    const SHOTS: &str = "{\"shot\":1}\n{\"shot\":2}\n{\"shot\":3}\n";

    /// Fresh move: the OpenR10 Connect folder (the name until 0.7) moves into a missing data
    /// folder, the old folder is gone and the history keeps every line.
    #[test]
    fn data_folder_of_openr10_connect_is_moved() {
        let home = crate::test_util::TempDir::new("settings-migrate");
        write_in(home.path(), OLD, r#"{"metric":false}"#);
        put(home.path(), OLD, "shots.jsonl", SHOTS);
        Settings::migrate_data_dir_in(home.path());
        assert!(!Settings::dir_in(home.path(), OLD).exists());
        assert!(!Settings::load_in(home.path()).metric);
        assert_eq!(read(home.path(), DATA_DIR, "shots.jsonl").unwrap().lines().count(), 3);
    }

    /// A data folder with only settings (e.g. written by another build) doesn't strand the history
    /// in the old folder: the old entries move in, the old one wins a clash, the new one is kept.
    #[test]
    fn data_folder_without_history_takes_the_old_one() {
        let home = crate::test_util::TempDir::new("settings-migrate-partial");
        write_in(home.path(), OLD, r#"{"metric":false}"#);
        put(home.path(), OLD, "shots.jsonl", SHOTS);
        write_in(home.path(), DATA_DIR, r#"{"metric":true}"#);
        put(home.path(), DATA_DIR, crate::instance::LOCK_FILE, "");
        Settings::migrate_data_dir_in(home.path());
        assert!(!Settings::dir_in(home.path(), OLD).exists());
        assert!(!Settings::load_in(home.path()).metric, "the old settings win");
        assert_eq!(read(home.path(), DATA_DIR, "settings.json.before-move").as_deref(), Some(r#"{"metric":true}"#));
        assert_eq!(read(home.path(), DATA_DIR, "shots.jsonl").as_deref(), Some(SHOTS));
        assert_eq!(read(home.path(), DATA_DIR, crate::instance::LOCK_FILE).as_deref(), Some(""));
    }

    /// A data folder with a history is never changed, and the old folder is left as it is.
    #[test]
    fn data_folder_with_history_is_left_alone() {
        let home = crate::test_util::TempDir::new("settings-migrate-has-history");
        write_in(home.path(), OLD, r#"{"metric":true}"#);
        put(home.path(), OLD, "shots.jsonl", SHOTS);
        write_in(home.path(), DATA_DIR, r#"{"metric":false}"#);
        put(home.path(), DATA_DIR, "shots.jsonl", "{\"shot\":9}\n");
        Settings::migrate_data_dir_in(home.path());
        assert!(!Settings::load_in(home.path()).metric);
        assert_eq!(read(home.path(), DATA_DIR, "shots.jsonl").as_deref(), Some("{\"shot\":9}\n"));
        assert!(read(home.path(), DATA_DIR, "settings.json.before-move").is_none());
        assert_eq!(read(home.path(), OLD, "shots.jsonl").as_deref(), Some(SHOTS));
        assert_eq!(read(home.path(), OLD, "settings.json").as_deref(), Some(r#"{"metric":true}"#));
    }

    /// A move cut short (some entries in the data folder, `shots.jsonl` still in the old one) goes
    /// on at the next start; lock files stay where they are.
    #[test]
    fn interrupted_move_goes_on() {
        let home = crate::test_util::TempDir::new("settings-migrate-interrupted");
        // Moved already: the settings. Still in the old folder: another file and the history.
        write_in(home.path(), DATA_DIR, r#"{"metric":false}"#);
        put(home.path(), OLD, "notes.txt", "x");
        put(home.path(), OLD, "shots.jsonl", SHOTS);
        put(home.path(), OLD, crate::instance::LOCK_FILE, "");
        Settings::migrate_data_dir_in(home.path());
        assert!(!Settings::dir_in(home.path(), OLD).exists());
        assert!(!Settings::load_in(home.path()).metric);
        assert_eq!(read(home.path(), DATA_DIR, "notes.txt").as_deref(), Some("x"));
        assert_eq!(read(home.path(), DATA_DIR, "shots.jsonl").unwrap().lines().count(), 3);
        assert!(read(home.path(), DATA_DIR, crate::instance::LOCK_FILE).is_none(), "lock files never move");
        assert!(read(home.path(), DATA_DIR, "settings.json.before-move").is_none());
    }

    /// `shots.jsonl` moves last, so a data folder holding it holds the rest too; a move stopped by
    /// an error leaves the marker, and the next start goes on with it.
    #[test]
    fn history_moves_last() {
        let home = crate::test_util::TempDir::new("settings-migrate-order");
        for name in ["a", "shots.jsonl", "z", "settings.json"] {
            put(home.path(), OLD, name, name);
        }
        // A data folder that can't take entries ("z" is a folder there with content) stops the move
        // before the history: the history is still in the old folder, the move is pending.
        for sub in ["z.before-move", "z"] {
            std::fs::create_dir_all(Settings::dir_in(home.path(), DATA_DIR).join(sub)).unwrap();
            put(home.path(), DATA_DIR, &format!("{sub}/keep"), "");
        }
        Settings::migrate_data_dir_in(home.path());
        assert_eq!(read(home.path(), OLD, "shots.jsonl").as_deref(), Some("shots.jsonl"));
        assert!(read(home.path(), DATA_DIR, "shots.jsonl").is_none());
        assert!(read(home.path(), DATA_DIR, PENDING_FILE).is_some(), "the move is marked pending");
        // Cleared, the next start finishes it. The data folder's "z" was there before the move began,
        // so the old one wins as on a move that never stopped.
        std::fs::remove_dir_all(Settings::dir_in(home.path(), DATA_DIR).join("z.before-move")).unwrap();
        Settings::migrate_data_dir_in(home.path());
        assert_eq!(read(home.path(), DATA_DIR, "shots.jsonl").as_deref(), Some("shots.jsonl"));
        assert_eq!(read(home.path(), DATA_DIR, "z").as_deref(), Some("z"));
        assert!(Settings::dir_in(home.path(), DATA_DIR).join("z.before-move/keep").is_file());
        assert!(read(home.path(), DATA_DIR, PENDING_FILE).is_none(), "nothing pending any more");
        assert!(!Settings::dir_in(home.path(), OLD).exists());
    }

    /// Sets the time `name` in the data folder `dir` was last changed to an hour ago.
    fn age(home: &Path, dir: &str, name: &str) {
        let file = std::fs::File::options().write(true).open(Settings::dir_in(home, dir).join(name)).unwrap();
        file.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600)).unwrap();
    }

    /// A move stopped by an error, then a run that changed the settings and recorded a shot: the
    /// next start goes on (the history in the data folder doesn't stop it), keeps the newer settings
    /// with the old ones as `settings.json.before-move`, and joins the histories, the old lines first.
    #[test]
    fn a_stopped_move_goes_on_and_keeps_newer_entries() {
        let home = crate::test_util::TempDir::new("settings-migrate-resume");
        for (name, text) in [("a", "a"), ("settings.json", r#"{"metric":true}"#), ("shots.jsonl", SHOTS)] {
            put(home.path(), OLD, name, text);
            age(home.path(), OLD, name);
        }
        // "a" can't move in: the data folder has a full "a" and "a.before-move".
        for sub in ["a.before-move", "a"] {
            std::fs::create_dir_all(Settings::dir_in(home.path(), DATA_DIR).join(sub)).unwrap();
            put(home.path(), DATA_DIR, &format!("{sub}/keep"), "");
        }
        Settings::migrate_data_dir_in(home.path());
        assert!(read(home.path(), DATA_DIR, PENDING_FILE).is_some(), "the move is marked pending");
        assert_eq!(read(home.path(), OLD, "settings.json").as_deref(), Some(r#"{"metric":true}"#));

        // The app ran on: the player changed the settings and a shot was recorded (after the move
        // began, so after the marker).
        age(home.path(), DATA_DIR, PENDING_FILE);
        write_in(home.path(), DATA_DIR, r#"{"metric":false}"#);
        put(home.path(), DATA_DIR, "shots.jsonl", "{\"shot\":4}\n");
        std::fs::remove_dir_all(Settings::dir_in(home.path(), DATA_DIR).join("a.before-move")).unwrap();
        Settings::migrate_data_dir_in(home.path());

        assert!(!Settings::load_in(home.path()).metric, "the newer settings stay");
        assert_eq!(read(home.path(), DATA_DIR, "settings.json.before-move").as_deref(), Some(r#"{"metric":true}"#));
        let history = read(home.path(), DATA_DIR, "shots.jsonl").unwrap();
        assert_eq!(history, format!("{SHOTS}{{\"shot\":4}}\n"), "old lines first, nothing lost");
        assert!(read(home.path(), DATA_DIR, PENDING_FILE).is_none());
        assert!(!Settings::dir_in(home.path(), OLD).exists());
    }

    /// Without a pending move, a data folder with a history stays as it is (the marker is what lets
    /// a move go on, not the history's absence alone).
    #[test]
    fn a_finished_move_leaves_no_marker() {
        let home = crate::test_util::TempDir::new("settings-migrate-done");
        put(home.path(), OLD, "shots.jsonl", SHOTS);
        Settings::migrate_data_dir_in(home.path());
        assert!(read(home.path(), DATA_DIR, PENDING_FILE).is_none());
        // An earlier folder that turns up again later is left alone.
        put(home.path(), OLD, "shots.jsonl", "{\"shot\":9}\n");
        Settings::migrate_data_dir_in(home.path());
        assert_eq!(read(home.path(), DATA_DIR, "shots.jsonl").as_deref(), Some(SHOTS));
        assert_eq!(read(home.path(), OLD, "shots.jsonl").as_deref(), Some("{\"shot\":9}\n"));
    }

    /// A join cut short after the joined history was in place (the old file still there) only
    /// removes the old file: the lines are never there twice, however often it goes on.
    #[test]
    fn a_join_cut_short_is_not_joined_twice() {
        let home = crate::test_util::TempDir::new("settings-migrate-join-twice");
        let joined = format!("{SHOTS}{{\"shot\":4}}\n");
        put(home.path(), OLD, "shots.jsonl", SHOTS);
        put(home.path(), DATA_DIR, "shots.jsonl", &joined);
        put(home.path(), DATA_DIR, PENDING_FILE, "");
        Settings::migrate_data_dir_in(home.path());
        assert_eq!(read(home.path(), DATA_DIR, "shots.jsonl").as_deref(), Some(joined.as_str()));
        assert_eq!(read(home.path(), DATA_DIR, "shots.jsonl").unwrap().lines().count(), 4);
        assert!(!Settings::dir_in(home.path(), OLD).exists());
        assert!(read(home.path(), DATA_DIR, PENDING_FILE).is_none());
    }

    /// An earlier history without a last line break still joins line by line.
    #[test]
    fn a_history_without_a_last_line_break_joins_cleanly() {
        let home = crate::test_util::TempDir::new("settings-migrate-join-newline");
        put(home.path(), OLD, "shots.jsonl", "{\"shot\":1}");
        put(home.path(), DATA_DIR, "shots.jsonl", "{\"shot\":2}\n");
        put(home.path(), DATA_DIR, PENDING_FILE, "");
        Settings::migrate_data_dir_in(home.path());
        assert_eq!(read(home.path(), DATA_DIR, "shots.jsonl").as_deref(), Some("{\"shot\":1}\n{\"shot\":2}\n"));
        // Going on once more (the old file back, as if not removed) doesn't add it again.
        put(home.path(), OLD, "shots.jsonl", "{\"shot\":1}");
        put(home.path(), DATA_DIR, PENDING_FILE, "");
        Settings::migrate_data_dir_in(home.path());
        assert_eq!(read(home.path(), DATA_DIR, "shots.jsonl").unwrap().lines().count(), 2);
    }

    /// A marker left when the earlier folder is gone (removed by hand) is cleared, so an earlier
    /// folder that turns up later doesn't go into a data folder with a history.
    #[test]
    fn a_stale_marker_is_cleared() {
        let home = crate::test_util::TempDir::new("settings-migrate-stale-marker");
        put(home.path(), DATA_DIR, "shots.jsonl", SHOTS);
        put(home.path(), DATA_DIR, PENDING_FILE, "");
        Settings::migrate_data_dir_in(home.path());
        assert!(read(home.path(), DATA_DIR, PENDING_FILE).is_none());
        put(home.path(), OLD, "shots.jsonl", "{\"shot\":9}\n");
        Settings::migrate_data_dir_in(home.path());
        assert_eq!(read(home.path(), DATA_DIR, "shots.jsonl").as_deref(), Some(SHOTS));
        assert_eq!(read(home.path(), OLD, "shots.jsonl").as_deref(), Some("{\"shot\":9}\n"));
    }

    /// One value this version doesn't know (a tile kind or overlay item of a newer version, or a
    /// typo) costs at most what holds it; the rest survives (bug #70: all fell back to the defaults).
    /// A tile of an unknown kind is kept (ADR-009).
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
        // The tile with the unknown kind is kept, only the unknown item's place is empty.
        assert_eq!(loaded[0].explorer.pages[0].tiles[0].kind, "gapping");
        assert_eq!(loaded[0].explorer.pages[0].tiles[1..], s.explorer.pages[0].tiles[1..]);
        assert_eq!(loaded[0].overlay_style, s.overlay_style);
        assert_eq!(loaded[1].overlay_style.layout[1], [None, Some(OverlayItem::ClubSpeed)]);
        assert_eq!(loaded[1].explorer, s.explorer);
    }

    /// An unknown tile size takes only its tile with it, in the explorer and in the tile overlays
    /// (of the arrangements, too); an unknown overlay position, or a language that isn't text, is the default.
    #[test]
    fn unknown_values_drop_only_their_entry() {
        let mut s = Settings { tile_overlays: vec![TileOverlay::default(), TileOverlay::default()], ..Settings::default() };
        s.keep_arrangement();
        let mut v = serde_json::to_value(&s).unwrap();
        v["explorer"]["pages"][0]["tiles"][1]["size"] = "w9_h9".into();
        v["tile_overlays"][0]["tile"]["size"] = "w9_h9".into();
        v["arrangements"][0]["tiles"][1]["pos"] = "center".into();
        v["overlay"] = "center".into();
        v["language"] = 7.into();
        let loaded: Settings = serde_json::from_value(v).unwrap();
        let kinds: Vec<&str> = loaded.explorer.pages[0].tiles.iter().map(|t| t.kind.as_str()).collect();
        assert_eq!(kinds, ["table", "flight"]);
        assert_eq!(loaded.explorer.pages[1..], s.explorer.pages[1..]);
        assert_eq!((loaded.tile_overlays.len(), loaded.arrangements[0].tiles.len()), (1, 1));
        assert_eq!((loaded.overlay, loaded.language), (OverlayPos::default(), Language::default()));
    }

    /// Older builds' language names read as codes and are saved as codes; a code this build
    /// doesn't have is kept (it resolves like System, see `i18n`).
    #[test]
    fn language_by_code_with_old_names() {
        for (stored, code) in [("system", "system"), ("english", "en"), ("german", "de"), ("de", "de"), ("fr", "fr")] {
            let loaded: Settings = serde_json::from_str(&format!(r#"{{"language":"{stored}"}}"#)).unwrap();
            assert_eq!(loaded.language, Language::new(code), "{stored}");
            assert_eq!(serde_json::to_value(&loaded).unwrap()["language"], code, "{stored} is saved as {code}");
        }
        assert_eq!(serde_json::to_value(Settings::default()).unwrap()["language"], "system");
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

    /// Tiles as the builds before tile kinds were modules wrote them (every kind's switches in
    /// every tile) load unchanged: each kind reads its own, and the file it saves is the same.
    #[test]
    fn tiles_of_the_earlier_format_load() {
        #[derive(Default, PartialEq, Debug, Deserialize)]
        #[serde(default)]
        struct Flight {
            camera: String,
            averages: bool,
        }
        let home = crate::test_util::TempDir::new("settings-tiles");
        write_in(home.path(), DATA_DIR, crate::fixture!("settings-tiles.json"));
        let s = Settings::load_in(home.path());
        let pages = &s.explorer.pages;
        let kinds: Vec<Vec<&str>> = pages.iter().map(|p| p.tiles.iter().map(|t| t.kind.as_str()).collect()).collect();
        assert_eq!(kinds, [vec!["table", "dispersion", "flight"], vec!["scatter", "details"]]);
        let sizes: Vec<TileSize> = pages.iter().flat_map(|p| p.tiles.iter().map(|t| t.size)).collect();
        assert_eq!(sizes, [TileSize::W3H2, TileSize::W2H2, TileSize::W1H2, TileSize::W2H2, TileSize::Page]);
        assert_eq!(pages[0].tiles[0].config["averages"], Value::Bool(true));
        assert_eq!(pages[0].tiles[1].config["total"], Value::Bool(true));
        assert_eq!(pages[0].tiles[2].config::<Flight>(), Flight { camera: "above".into(), averages: true });
        assert_eq!((&pages[1].tiles[0].config["x"], &pages[1].tiles[0].config["y"]), (&"ball_speed".into(), &"carry".into()));
        assert_eq!(s.tile_overlays[0].tile.config::<Flight>(), Flight { camera: "side".into(), averages: false });
        assert_eq!((s.tile_overlays[0].club_only, s.tile_overlays[0].width), (true, 460.0));
        assert!(s.extra.is_empty(), "{:?}", s.extra);
        // Saved and read again: the same tiles, every switch where the earlier builds look for it.
        s.save_in(home.path()).unwrap();
        let file = Settings::dir_in(home.path(), DATA_DIR).join("settings.json");
        let saved: Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
        let fixture: Value = serde_json::from_str(crate::fixture!("settings-tiles.json")).unwrap();
        assert_eq!(saved["explorer"], fixture["explorer"]);
        assert_eq!(saved["tile_overlays"][0]["tile"], fixture["tile_overlays"][0]["tile"]);
        assert_eq!(Settings::load_in(home.path()), s);
    }

    /// A tile of a kind this build doesn't have (an extension's, or a newer build's) is kept with all
    /// of its settings when the settings are saved.
    #[test]
    fn tiles_of_an_unknown_kind_survive_a_save() {
        let gapping = r#"{"kind":"gapping","size":"w2_h1","clubs":["DR","I7"],"bins":{"width":5}}"#;
        let json = format!(r#"{{"explorer":{{"pages":[{{"name":"Bag","tiles":[{gapping}]}}]}},"tile_overlays":[{{"tile":{gapping}}}]}}"#);
        let home = crate::test_util::TempDir::new("settings-unknown-kind");
        write_in(home.path(), DATA_DIR, &json);
        let s = Settings::load_in(home.path());
        let tile = &s.explorer.pages[0].tiles[0];
        assert_eq!((tile.kind.as_str(), tile.size), ("gapping", TileSize::W2H1));
        assert_eq!(s.tile_overlays[0].tile, *tile);
        s.save_in(home.path()).unwrap();
        let file = Settings::dir_in(home.path(), DATA_DIR).join("settings.json");
        let saved: Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
        let expected: Value = serde_json::from_str(gapping).unwrap();
        assert_eq!(saved["explorer"]["pages"][0]["tiles"][0], expected);
        assert_eq!(saved["tile_overlays"][0]["tile"], expected);
    }

    /// A kind's settings: read as its own type with defaults for what is missing or unreadable,
    /// changed without losing the settings of other kinds.
    #[test]
    fn tile_settings_are_read_and_changed_by_their_kind() {
        #[derive(Default, PartialEq, Debug, Serialize, Deserialize)]
        #[serde(default)]
        struct Chart {
            total: bool,
            bins: u32,
        }
        let mut tile = Tile::new("chart", TileSize::W1H1).with(&serde_json::json!({ "averages": true, "bins": 3 }));
        assert_eq!(tile.config::<Chart>(), Chart { total: false, bins: 3 });
        tile.change_config(|c: &mut Chart| c.total = true);
        assert_eq!(Value::Object(tile.config.clone()), serde_json::json!({ "averages": true, "bins": 3, "total": true }));
        // Nothing changed: nothing written (a missing default stays missing).
        let mut plain = Tile::new("chart", TileSize::W1H1);
        plain.change_config(|c: &mut Chart| c.bins = 0);
        assert!(plain.config.is_empty());
        // Unreadable: all defaults; the value itself is kept.
        let odd = Tile::new("chart", TileSize::W1H1).with(&serde_json::json!({ "bins": "many" }));
        assert_eq!(odd.config::<Chart>(), Chart::default());
        assert_eq!(odd.config["bins"], "many");
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
