// "BlueZ" and "btleplug" are brand names, not code identifiers.
#![allow(clippy::doc_markdown)]

#[cfg(feature = "ble")]
pub mod ble;
#[cfg(ble_backend_bluez)]
pub mod bluez;
#[cfg(ble_backend_btleplug)]
pub mod btleplug;
pub mod client;
pub mod cobs;
pub mod crc;
pub mod error;
#[cfg(feature = "frp")]
pub mod frp;
pub mod gfdi;
pub mod multilink;
pub mod proto;

#[cfg(feature = "ble")]
pub use ble::{BleError, BleTransport};
pub use client::{Client, Event, Transport};
pub use error::Error;
#[cfg(feature = "frp")]
pub use frp::FrpDevice;
pub use proto::ShotData;

/// Receiver for diagnostic messages (BLE scan, connection watchdog). r10-gspro addition:
/// lets the app show them in its log instead of stderr.
static DIAG: std::sync::OnceLock<fn(&str)> = std::sync::OnceLock::new();

/// Route diagnostic messages to `f` (once; later calls are ignored).
pub fn set_diagnostics(f: fn(&str)) {
    let _ = DIAG.set(f);
}

#[allow(dead_code)]
pub(crate) fn diag(msg: &str) {
    match DIAG.get() {
        Some(f) => f(msg),
        None => eprintln!("{msg}"),
    }
}
