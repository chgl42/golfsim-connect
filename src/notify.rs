//! macOS notifications for problems that need attention while GSPro is in front
//! (low battery, R10 errors, lost connections, undelivered shots).
//!
//! Texts are English source strings, translated with [`i18n::tr_detail`]. Each kind
//! of alert is sent at most once per [`MIN_GAP`], so a flapping connection doesn't spam.
//! macOS only shows the banner while this app is not the active one.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use crate::{i18n, log};

/// Off until the app enables it from the settings (the CLI never notifies).
pub static ENABLED: AtomicBool = AtomicBool::new(false);

/// Minimum time between two notifications of the same kind.
const MIN_GAP: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Alert {
    BatteryLow,
    DeviceError,
    R10Lost,
    GsproLost,
    ShotNotDelivered,
}

static LAST_SENT: LazyLock<Mutex<HashMap<Alert, Instant>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Post a notification (if enabled and not rate-limited). `text` is the English source text.
pub fn alert(kind: Alert, text: &str) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    {
        let mut last = LAST_SENT.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if last.get(&kind).is_some_and(|t| t.elapsed() < MIN_GAP) {
            return;
        }
        last.insert(kind, Instant::now());
    }
    log::info(&format!("Notification: {text}"));
    let body = i18n::tr_detail(text);
    // Delivery talks to the notification centre; keep it off the bridge threads.
    std::thread::spawn(move || deliver(&body));
}

/// Send a notification right away, regardless of the setting and the rate limit
/// (the "Test" button in Settings).
pub fn send_now(text: &str) {
    log::info(&format!("Notification (test): {text}"));
    let body = i18n::tr_detail(text);
    std::thread::spawn(move || deliver(&body));
}

#[cfg(all(target_os = "macos", not(test)))]
fn deliver(body: &str) {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        // Inside the app bundle, notifications carry our name and icon. Started outside
        // a bundle (cargo run), fall back to the library's default sender.
        let bundled = std::env::current_exe().is_ok_and(|p| p.to_string_lossy().contains(".app/Contents/MacOS/"));
        if bundled {
            let _ = mac_notification_sys::set_application("com.golfsimconnect.app");
        }
    });
    if let Err(e) = mac_notification_sys::send_notification("GolfSim Connect", None, body, None) {
        log::info(&format!("Notification failed: {e}"));
    }
}

#[cfg(any(not(target_os = "macos"), test))]
fn deliver(_body: &str) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_kind_is_rate_limited() {
        ENABLED.store(true, Ordering::Relaxed);
        LAST_SENT.lock().unwrap().clear();
        alert(Alert::ShotNotDelivered, "Shot was not delivered to GSPro");
        let first = LAST_SENT.lock().unwrap()[&Alert::ShotNotDelivered];
        alert(Alert::ShotNotDelivered, "Shot was not delivered to GSPro");
        assert_eq!(LAST_SENT.lock().unwrap()[&Alert::ShotNotDelivered], first, "second alert within MIN_GAP is dropped");
        ENABLED.store(false, Ordering::Relaxed);
    }
}
