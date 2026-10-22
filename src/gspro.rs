//! GSPro OpenConnect v1 client (JSON over TCP, default 127.0.0.1:921).
//!
//! Runs in its own thread, reconnects on its own, and keeps GSPro informed of
//! the launch monitor's readiness with heartbeats.

use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

use crate::history;
use crate::log;
use crate::notify::{self, Alert};
use crate::state;
use crate::mapping::{GsBall, GsClub, Tempo};

const RECONNECT_INTERVAL: Duration = Duration::from_secs(3);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// GSPro parses one JSON message per TCP read and rejects concatenated
/// messages ("501 Bad format"), so messages must never be sent back to back.
const MIN_SEND_GAP: Duration = Duration::from_millis(300);
/// How long to wait for GSPro's reply before sending the next message.
const REPLY_TIMEOUT: Duration = Duration::from_millis(1500);

/// Commands from the R10 side to the GSPro side.
#[derive(Debug, Clone)]
pub enum Cmd {
    /// Launch monitor readiness changed.
    Ready(bool),
    /// A shot to forward. Test shots are sent to GSPro but not added to the history.
    Shot { ball: GsBall, club: Option<GsClub>, tempo: Option<Tempo>, test: bool },
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct ShotDataOptions {
    contains_ball_data: bool,
    contains_club_data: bool,
    launch_monitor_is_ready: bool,
    launch_monitor_ball_detected: bool,
    is_heart_beat: bool,
}

#[derive(Serialize)]
struct Message<'a> {
    #[serde(rename = "DeviceID")]
    device_id: &'a str,
    #[serde(rename = "Units")]
    units: &'a str,
    #[serde(rename = "ShotNumber")]
    shot_number: u32,
    #[serde(rename = "APIversion")]
    api_version: &'a str,
    #[serde(rename = "BallData", skip_serializing_if = "Option::is_none")]
    ball_data: Option<GsBall>,
    #[serde(rename = "ClubData", skip_serializing_if = "Option::is_none")]
    club_data: Option<GsClub>,
    #[serde(rename = "ShotDataOptions")]
    options: ShotDataOptions,
}

const DEVICE_ID: &str = "OpenLaunchConnect";

/// Log raw OpenConnect traffic; can be toggled at runtime (e.g. from the app).
pub static DEBUG: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn debug_on(local: bool) -> bool {
    local || DEBUG.load(std::sync::atomic::Ordering::Relaxed)
}

#[must_use]
pub fn heartbeat_json(ready: bool) -> String {
    serde_json::to_string(&Message {
        device_id: DEVICE_ID,
        units: "Yards",
        shot_number: 0,
        api_version: "1",
        ball_data: None,
        club_data: None,
        options: ShotDataOptions {
            contains_ball_data: false,
            contains_club_data: false,
            launch_monitor_is_ready: ready,
            launch_monitor_ball_detected: ready,
            is_heart_beat: true,
        },
    })
    .expect("serializable")
}

#[must_use]
pub fn shot_json(shot_number: u32, ball: GsBall, club: Option<GsClub>) -> String {
    serde_json::to_string(&Message {
        device_id: DEVICE_ID,
        units: "Yards",
        shot_number,
        api_version: "1",
        ball_data: Some(ball),
        club_data: club,
        options: ShotDataOptions {
            contains_ball_data: true,
            contains_club_data: club.is_some(),
            launch_monitor_is_ready: true,
            launch_monitor_ball_detected: true,
            is_heart_beat: false,
        },
    })
    .expect("serializable")
}

pub struct Link {
    addr: String,
    debug: bool,
    stream: Option<TcpStream>,
    rx_buf: Vec<u8>,
    ready: bool,
    shot_number: u32,
    last_heartbeat: Instant,
    last_attempt: Option<Instant>,
    last_send: Option<Instant>,
    warned_unreachable: bool,
    /// A shot was sent and GSPro has not confirmed it yet.
    awaiting_shot_ack: bool,
}

impl Link {
    #[must_use]
    pub fn new(addr: String, debug: bool) -> Self {
        Self {
            addr,
            debug,
            stream: None,
            rx_buf: Vec::new(),
            ready: false,
            shot_number: initial_shot_number(),
            last_heartbeat: Instant::now(),
            last_attempt: None,
            last_send: None,
            warned_unreachable: false,
            awaiting_shot_ack: false,
        }
    }

    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.stream.is_some()
    }

    /// Thread body: process commands until the sender is dropped.
    pub fn run(mut self, rx: &Receiver<Cmd>) {
        loop {
            self.ensure_connected();
            self.read_responses();
            if self.is_connected() && self.last_heartbeat.elapsed() >= HEARTBEAT_INTERVAL {
                self.send_heartbeat();
            }
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(cmd) => self.handle(cmd),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    pub fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Ready(ready) => {
                if ready != self.ready {
                    self.ready = ready;
                    if self.is_connected() {
                        self.send_heartbeat();
                    }
                }
            }
            Cmd::Shot { ball, club, tempo, test } => {
                let was_ready = self.ready;
                self.ready = true;
                if !self.is_connected() {
                    // One immediate attempt, so a shot right after GSPro starts is not lost.
                    self.last_attempt = None;
                    self.ensure_connected();
                }
                if !self.is_connected() {
                    log::gspro("⚠️  Shot dropped – not connected to GSPro.");
                    record_shot(ball, club, tempo, false, 0, test);
                    if test {
                        self.ready = was_ready;
                    }
                    return;
                }
                if !was_ready {
                    // Tell GSPro the launch monitor is ready before the first shot.
                    self.send_heartbeat();
                }
                self.shot_number += 1;
                let json = shot_json(self.shot_number, ball, club);
                // Drain pending heartbeat replies so they aren't taken as the shot's ack.
                self.read_responses();
                self.awaiting_shot_ack = true;
                log::gspro(&format!("Sending shot to GSPro (ShotNumber {}) …", self.shot_number));
                let delivered = self.send(&json);
                if delivered {
                    if self.awaiting_shot_ack {
                        log::gspro("No direct reply from GSPro to the shot.");
                    }
                } else {
                    self.awaiting_shot_ack = false;
                }
                record_shot(ball, club, tempo, delivered, self.shot_number, test);
                // A test shot doesn't make the launch monitor ready (e.g. no R10 connected).
                if test && !was_ready {
                    self.ready = false;
                    if self.is_connected() {
                        self.send_heartbeat();
                    }
                }
            }
        }
    }

    pub fn ensure_connected(&mut self) {
        if self.stream.is_some() {
            return;
        }
        if let Some(t) = self.last_attempt
            && t.elapsed() < RECONNECT_INTERVAL {
                return;
            }
        self.last_attempt = Some(Instant::now());

        let addrs: Vec<SocketAddr> = match self.addr.to_socket_addrs() {
            Ok(a) => a.collect(),
            Err(e) => {
                log::gspro(&format!("Invalid address {}: {e}", self.addr));
                return;
            }
        };
        for addr in addrs {
            match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
                Ok(stream) => {
                    let _ = stream.set_nodelay(true);
                    let _ = stream.set_nonblocking(true);
                    self.stream = Some(stream);
                    self.rx_buf.clear();
                    self.warned_unreachable = false;
                    log::gspro(&format!("✅ Connected to GSPro OpenConnect ({addr})"));
                    state::set_gspro(true, format!("Connected ({addr})")); // "(…)" suffix: see i18n::tr_detail
                    self.send_heartbeat();
                    return;
                }
                Err(e) => {
                    state::set_gspro(false, "Waiting for GSPro (Connect window open?)");
                    if !self.warned_unreachable {
                        log::gspro(&format!(
                            "Waiting for GSPro OpenConnect at {addr} ({e}). \
                             Is GSPro running with its Connect window open?"
                        ));
                        self.warned_unreachable = true;
                    }
                }
            }
        }
    }

    fn send_heartbeat(&mut self) {
        let json = heartbeat_json(self.ready);
        self.send(&json);
        self.last_heartbeat = Instant::now();
    }

    /// Send one message, then wait for GSPro's reply so that the next message
    /// always arrives in a separate TCP read on GSPro's side.
    fn send(&mut self, json: &str) -> bool {
        if self.stream.is_none() {
            return false;
        }
        // Let any pending replies arrive and keep a minimum gap between messages.
        self.read_responses();
        if let Some(t) = self.last_send {
            let elapsed = t.elapsed();
            if elapsed < MIN_SEND_GAP {
                std::thread::sleep(MIN_SEND_GAP - elapsed);
            }
        }
        let Some(stream) = self.stream.as_mut() else {
            return false;
        };
        if debug_on(self.debug) {
            log::gspro(&format!("OUT {json}"));
        }
        // Brief blocking write: messages are tiny.
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
        let result = stream.write_all(json.as_bytes()).and_then(|()| stream.flush());
        let _ = stream.set_nonblocking(true);
        self.last_send = Some(Instant::now());
        match result {
            Ok(()) => {
                self.wait_for_reply();
                // Writing to a socket GSPro just closed succeeds; the close only shows up while
                // reading the reply, which drops the stream.
                self.stream.is_some()
            }
            Err(e) => {
                log::gspro(&format!("Lost connection to GSPro ({e}) – reconnecting …"));
                state::set_gspro(false, "Connection lost – reconnecting …");
                notify::alert(Alert::GsproLost, "GSPro connection lost");
                self.stream = None;
                false
            }
        }
    }

    /// Block until GSPro answers (or the timeout passes).
    fn wait_for_reply(&mut self) {
        let deadline = Instant::now() + REPLY_TIMEOUT;
        while self.stream.is_some() && Instant::now() < deadline {
            if self.read_responses() > 0 {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Read and handle all available replies; returns how many were handled.
    pub fn read_responses(&mut self) -> usize {
        let Some(stream) = self.stream.as_mut() else {
            return 0;
        };
        let mut buf = [0u8; 4096];
        loop {
            match stream.read(&mut buf) {
                Ok(0) => {
                    log::gspro("GSPro closed the connection – reconnecting …");
                    state::set_gspro(false, "GSPro disconnected – reconnecting …");
                    notify::alert(Alert::GsproLost, "GSPro connection lost");
                    self.stream = None;
                    return 0;
                }
                Ok(n) => self.rx_buf.extend_from_slice(&buf[..n]),
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => {
                    log::gspro(&format!("Read error from GSPro ({e}) – reconnecting …"));
                    state::set_gspro(false, "Connection error – reconnecting …");
                    notify::alert(Alert::GsproLost, "GSPro connection lost");
                    self.stream = None;
                    return 0;
                }
            }
        }
        let responses = drain_json_values(&mut self.rx_buf);
        for resp in &responses {
            self.handle_response(resp);
        }
        responses.len()
    }

    fn handle_response(&mut self, resp: &Value) {
        if debug_on(self.debug) {
            log::gspro(&format!("IN  {resp}"));
        }
        let code = resp.get("Code").and_then(Value::as_i64).unwrap_or(0);
        let msg = resp.get("Message").and_then(Value::as_str).unwrap_or("");
        match code {
            200 => {
                if self.awaiting_shot_ack && !msg.to_lowercase().contains("heartbeat") {
                    self.awaiting_shot_ack = false;
                    log::gspro(&format!("GSPro acknowledged the shot ({msg})"));
                }
            }
            201 => {
                if let Some(p) = resp.get("Player") {
                    let club = p.get("Club").and_then(Value::as_str).unwrap_or("?");
                    let handed = p.get("Handed").and_then(Value::as_str).unwrap_or("?");
                    log::gspro(&format!("Player: {handed}, club: {club}"));
                    let (club, handed) = (club.to_string(), handed.to_string());
                    state::with(|s| {
                        s.gspro_club = Some(club);
                        s.gspro_handed = Some(handed);
                    });
                }
            }
            c if c >= 500 => log::gspro(&format!("⚠️  GSPro reported error {c}: {msg}")),
            _ => {}
        }
    }
}

fn record_shot(ball: GsBall, club: Option<GsClub>, tempo: Option<Tempo>, delivered: bool, shot_number: u32, test: bool) {
    let rec = state::with(|s| {
        if delivered {
            s.shots_sent += 1;
        }
        s.last_shot = Some(state::LastShot { ball, club, tempo, at: Instant::now(), delivered, shot_number });
        // GSPro's result of the previous shot no longer applies (the new one follows when it lands).
        s.gspro_result = None;
        if test {
            return None;
        }
        let mut rec = history::ShotRecord::new(ball, club, s.gspro_club.clone(), delivered, shot_number);
        rec.tempo = tempo;
        s.session.push(rec.clone());
        Some(rec)
    });
    if !delivered && !test {
        notify::alert(Alert::ShotNotDelivered, "Shot was not delivered to GSPro");
    }
    if let Some(rec) = rec
        && let Err(e) = history::append(&rec)
    {
        log::gspro(&format!("⚠️  Could not save shot history: {e}"));
    }
}

/// GSPro ignores a shot whose ShotNumber it has already seen, so numbering must
/// keep increasing across restarts. Seconds since 2025-01-01 does that and fits in i32.
fn initial_shot_number() -> u32 {
    const EPOCH_2025: u64 = 1_735_689_600;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(EPOCH_2025, |d| d.as_secs());
    u32::try_from(now.saturating_sub(EPOCH_2025)).unwrap_or(1)
}

/// Pull every complete JSON value out of `buf` (GSPro concatenates
/// responses without delimiters), leaving any trailing partial value.
pub fn drain_json_values(buf: &mut Vec<u8>) -> Vec<Value> {
    let mut out = Vec::new();
    let mut stream = serde_json::Deserializer::from_slice(buf).into_iter::<Value>();
    let mut consumed = 0;
    loop {
        match stream.next() {
            Some(Ok(v)) => {
                consumed = stream.byte_offset();
                out.push(v);
            }
            Some(Err(e)) if e.is_eof() => break,
            Some(Err(_)) => {
                // Garbage: drop everything we have and resync on the next read.
                consumed = buf.len();
                break;
            }
            None => {
                consumed = stream.byte_offset();
                break;
            }
        }
    }
    buf.drain(..consumed);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A test shot – here to a connection GSPro has just closed – is not delivered and
    /// leaves the launch monitor's readiness as it was.
    #[test]
    fn test_shot_keeps_the_readiness() {
        crate::notify::ENABLED.store(false, std::sync::atomic::Ordering::Relaxed);
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let mut link = Link::new(listener.local_addr().unwrap().to_string(), false);
        link.ensure_connected();
        assert!(link.is_connected());
        drop(listener.accept().unwrap());
        let (ball, club) = crate::engine::test_shot();
        link.handle(Cmd::Shot { ball, club: Some(club), tempo: None, test: true });
        assert_eq!(state::with(|s| s.last_shot.as_ref().map(|s| s.delivered)), Some(false));
        assert!(!link.ready);
    }

    #[test]
    fn drains_concatenated_and_partial_responses() {
        let mut buf = br#"{"Code":200,"Message":"ok"}{"Code":201,"Player":{"Club":"DR"}}{"Code":2"#.to_vec();
        let vals = drain_json_values(&mut buf);
        assert_eq!(vals.len(), 2);
        assert_eq!(vals[1]["Player"]["Club"], "DR");
        assert_eq!(buf, br#"{"Code":2"#);
        buf.extend_from_slice(br#"00}"#);
        assert_eq!(drain_json_values(&mut buf).len(), 1);
        assert!(buf.is_empty());
    }

    #[test]
    fn heartbeat_has_required_fields() {
        let v: Value = serde_json::from_str(&heartbeat_json(true)).unwrap();
        assert_eq!(v["APIversion"], "1");
        assert_eq!(v["Units"], "Yards");
        assert_eq!(v["ShotDataOptions"]["IsHeartBeat"], true);
        assert_eq!(v["ShotDataOptions"]["LaunchMonitorIsReady"], true);
        assert!(v.get("BallData").is_none());
    }
}
