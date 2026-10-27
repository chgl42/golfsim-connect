//! GolfSim Connect — open-source macOS bridge from a Garmin Approach R10 (Bluetooth LE)
//! to GSPro's OpenConnect API (e.g. GSPro running under CrossOver).
//!
//! R10 ──BLE──> bridge ──TCP 127.0.0.1:921──> GSPro

pub mod engine;
pub mod events;
pub mod flight;
pub mod gspro;
pub mod gspro_db;
pub mod gspro_log;
pub mod history;
pub mod i18n;
pub mod instance;
pub mod launcher;
pub mod log;
pub mod mapping;
pub mod misread;
pub mod notify;
pub mod recorder;
pub mod settings;
pub mod shape;
pub mod state;
#[cfg(feature = "gui")]
pub mod ui;
pub mod update;

#[cfg(test)]
mod test_util;

/// `v` with `digits` decimals, without the minus sign of values that round to zero ("-0.0" → "0.0").
#[must_use]
pub fn num(v: f64, digits: usize) -> String {
    let s = format!("{v:.digits$}");
    match s.strip_prefix('-') {
        Some(rest) if rest.bytes().all(|b| b == b'0' || b == b'.') => rest.to_string(),
        _ => s,
    }
}

#[cfg(test)]
mod tests {
    use super::num;

    #[test]
    fn num_drops_the_sign_of_negative_zero() {
        assert_eq!(num(-0.04, 1), "0.0");
        assert_eq!(num(-0.0, 0), "0");
        assert_eq!(num(-0.4, 0), "0");
        assert_eq!(num(-0.05, 1), "-0.1");
        assert_eq!(num(-12.34, 1), "-12.3");
        assert_eq!(num(0.04, 1), "0.0");
    }
}
