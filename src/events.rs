//! What happens in the bridge, as events for whoever wants to react to it (sounds, an announcement
//! of GSPro's result, …) without being called from the core.
//!
//! `state` stays the snapshot the GUI draws each frame; events say *that* something happened.
//! Every receiver from [`subscribe`] gets every event published after it subscribed.
//!
//! - **Order:** all subscribers see the events in the same order, the order [`publish`] was called
//!   in (one lock around the fan-out); from one thread, that is the order it published them.
//!   An event is published after the state it describes is written, so a subscriber that looks
//!   into `state` finds it there.
//! - **Threads:** events are published from the bridge threads (`r10`, `gspro`, `gspro-db`);
//!   a receiver can be read on any thread (std mpsc).
//! - **Never blocks:** the channels are unbounded and a dropped receiver is removed on the next
//!   publish, so a slow or gone subscriber never holds up the bridge. A receiver that is kept but
//!   never read keeps every event, so drop what you don't read.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Mutex;

use crate::gspro_db::GsproResult;
use crate::history::ShotRecord;
use crate::state::R10Status;

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// A shot was recorded: sent to GSPro or not (`shot.delivered`), in the session and the
    /// history unless it is a test shot.
    ShotRecorded { shot: Box<ShotRecord>, test: bool },
    /// GSPro's result arrived for the session shot with this time and shot number, or for a shot
    /// not in the session (`None`, e.g. a test shot).
    GsproResult { shot: Option<(String, u32)>, result: GsproResult },
    /// The R10's status or its detail text changed.
    R10Status { status: R10Status, detail: String },
    /// The GSPro connection or its detail text changed.
    GsproStatus { connected: bool, detail: String },
    /// The R10's battery dropped to the low level (in percent).
    BatteryLow { level: u8 },
    /// The R10 reported a problem (e.g. "R10 is tilted – please level it (…)").
    DeviceError { what: String },
    /// GSPro went to its menus, the range or a course (from its log), or that isn't known any
    /// more (`None`, e.g. GSPro restarted).
    GsproMode { mode: Option<crate::gspro_log::GameMode> },
}

static SUBSCRIBERS: Mutex<Vec<Sender<Event>>> = Mutex::new(Vec::new());

/// A receiver for every event published from now on.
pub fn subscribe() -> Receiver<Event> {
    let (tx, rx) = mpsc::channel();
    SUBSCRIBERS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(tx);
    rx
}

/// Send `event` to every subscriber; never blocks (see the module docs).
pub fn publish(event: Event) {
    fan_out(&mut SUBSCRIBERS.lock().unwrap_or_else(std::sync::PoisonError::into_inner), &event);
}

/// Send `event` to each of `subscribers`, dropping those whose receiver is gone.
fn fan_out(subscribers: &mut Vec<Sender<Event>>, event: &Event) {
    subscribers.retain(|tx| tx.send(event.clone()).is_ok());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Events of this test only (tests run in parallel and publish too).
    fn mine(rx: &Receiver<Event>, tag: &str) -> Vec<String> {
        rx.try_iter()
            .filter_map(|e| match e {
                Event::DeviceError { what } if what.starts_with(tag) => Some(what),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn every_subscriber_gets_the_events_in_order() {
        let (a, b) = (subscribe(), subscribe());
        let sent: Vec<String> = (0..20).map(|i| format!("order-{i}")).collect();
        for what in &sent {
            publish(Event::DeviceError { what: what.clone() });
        }
        assert_eq!(mine(&a, "order-"), sent);
        assert_eq!(mine(&b, "order-"), sent);
    }

    #[test]
    fn a_dropped_subscriber_is_removed_and_blocks_nothing() {
        let (kept, kept_rx) = mpsc::channel();
        let (gone, gone_rx) = mpsc::channel();
        let mut subscribers = vec![gone, kept];
        drop(gone_rx);
        fan_out(&mut subscribers, &Event::DeviceError { what: "first".into() });
        assert_eq!(subscribers.len(), 1, "removed on the next publish");
        // Many events with no one reading: publishing still returns at once.
        for _ in 0..1000 {
            fan_out(&mut subscribers, &Event::DeviceError { what: "more".into() });
        }
        assert_eq!(kept_rx.try_iter().count(), 1001);
        // Through the global list as well.
        drop(subscribe());
        publish(Event::DeviceError { what: "after-drop".into() });
    }
}
