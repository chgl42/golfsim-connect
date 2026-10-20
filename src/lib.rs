//! OpenLaunch Connect — open-source macOS bridge from a Garmin Approach R10 (Bluetooth LE)
//! to GSPro's OpenConnect API (e.g. GSPro running under CrossOver).
//!
//! R10 ──BLE──> bridge ──TCP 127.0.0.1:921──> GSPro

pub mod engine;
pub mod gspro;
pub mod history;
pub mod i18n;
pub mod launcher;
pub mod log;
pub mod mapping;
pub mod notify;
pub mod settings;
pub mod state;
pub mod update;
