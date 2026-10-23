//! Shared, process-wide status that the GUI (or anything else) can read.

use std::collections::VecDeque;
use std::sync::{LazyLock, Mutex};
use std::time::Instant;

use crate::events::{self, Event};
use crate::history::ShotRecord;
use crate::mapping::{GsBall, GsClub, Tempo};

const LOG_CAPACITY: usize = 400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum R10Status {
    /// Scanning / waiting for the device.
    Searching,
    /// Bluetooth connected, handshake in progress or device not armed.
    Connected,
    /// Armed and waiting for a shot.
    Ready,
}

#[derive(Debug, Clone)]
pub struct LastShot {
    pub ball: GsBall,
    pub club: Option<GsClub>,
    pub tempo: Option<Tempo>,
    pub at: Instant,
    pub delivered: bool,
    pub shot_number: u32,
    /// The reading that looks wrong, if any (see `misread`).
    pub misread: Option<crate::misread::Misread>,
}

#[derive(Debug)]
pub struct State {
    pub r10: R10Status,
    pub r10_detail: String,
    /// Bluetooth id of the connected R10 (macOS: peripheral UUID), if connected.
    pub r10_device: Option<String>,
    /// R10 battery level in percent, if known.
    pub r10_battery: Option<u8>,
    pub gspro_connected: bool,
    pub gspro_detail: String,
    /// Club currently selected in GSPro (from the 201 player info).
    pub gspro_club: Option<String>,
    /// Handedness reported by GSPro ("RH"/"LH").
    pub gspro_handed: Option<String>,
    pub last_shot: Option<LastShot>,
    /// GSPro's result of the latest shot (from its database), e.g. the distance left to the pin.
    pub gspro_result: Option<crate::gspro_db::GsproResult>,
    pub shots_sent: u32,
    /// Newer release found by the update check.
    pub update: Option<crate::update::Release>,
    /// Outcome of the last update check (shown in the settings).
    pub update_check: Option<crate::update::Check>,
    /// Real shots of this session (test shots excluded), oldest first.
    pub session: Vec<ShotRecord>,
    pub log: VecDeque<String>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            r10: R10Status::Searching,
            r10_detail: "Searching for R10 …".into(),
            r10_device: None,
            r10_battery: None,
            gspro_connected: false,
            gspro_detail: "Waiting for GSPro …".into(),
            gspro_club: None,
            gspro_handed: None,
            last_shot: None,
            gspro_result: None,
            shots_sent: 0,
            update: None,
            update_check: None,
            session: Vec::new(),
            log: VecDeque::with_capacity(LOG_CAPACITY),
        }
    }
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| Mutex::new(State::default()));

/// Run `f` with exclusive access to the shared state.
pub fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    f(&mut guard)
}

/// Set the R10's status; a change is published as [`Event::R10Status`](crate::events::Event::R10Status).
pub fn set_r10(status: R10Status, detail: impl Into<String>) {
    let detail = detail.into();
    let changed = with(|s| {
        let changed = s.r10 != status || s.r10_detail != detail;
        s.r10 = status;
        s.r10_detail.clone_from(&detail);
        changed
    });
    if changed {
        events::publish(Event::R10Status { status, detail });
    }
}

/// Set the GSPro connection's status; a change is published as
/// [`Event::GsproStatus`](crate::events::Event::GsproStatus).
pub fn set_gspro(connected: bool, detail: impl Into<String>) {
    let detail = detail.into();
    let changed = with(|s| {
        let changed = s.gspro_connected != connected || s.gspro_detail != detail;
        s.gspro_connected = connected;
        s.gspro_detail.clone_from(&detail);
        changed
    });
    if changed {
        events::publish(Event::GsproStatus { connected, detail });
    }
}

pub fn push_log(line: String) {
    with(|s| {
        if s.log.len() >= LOG_CAPACITY {
            s.log.pop_front();
        }
        s.log.push_back(line);
    });
}
