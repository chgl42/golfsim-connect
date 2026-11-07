//! The path to GSPro, end to end: the real `openlaunch` binary and `Link::run` against
//! `tools/mock_gspro.py`, checked by what the mock received. Needs `python3`.

use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use openlaunch_connect::gspro::{Cmd, Link};
use serde_json::{Value, json};

/// Far longer than a healthy run needs (about 4 s for the test shot, 10 s for the periodic
/// heartbeat), far shorter than a minute.
const DEADLINE: Duration = Duration::from_secs(30);
/// The time a periodic heartbeat has to be apart from the one before. Its interval is private to
/// `gspro.rs`; half of it is enough to tell it from an answer to a command.
const MIN_PERIOD: Duration = Duration::from_secs(5);

/// A child process, killed on every path out of a test.
struct Killed(Child);

impl Drop for Killed {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// `tools/mock_gspro.py` on a free port, with the lines it prints.
struct Mock {
    _child: Killed,
    lines: Receiver<String>,
    port: u16,
}

impl Mock {
    fn start() -> Self {
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/tools/mock_gspro.py");
        let mut child = Command::new("python3")
            .args([script, "0"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()
            .map_or_else(|e| panic!("this test needs python3 on the PATH: {e}"), Killed);
        let stdout = child.0.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let first = lines.recv_timeout(DEADLINE).unwrap_or_else(|e| panic!("the mock reported no port: {e}"));
        let port = first.rsplit_once(':').and_then(|(_, p)| p.parse().ok()).filter(|p| *p != 0);
        let port = port.unwrap_or_else(|| panic!("no port in the mock's first line: {first}"));
        Self { _child: child, lines, port }
    }

    fn addr(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }

    /// The next line the mock printed, panicking at `deadline`.
    fn next_line(&self, deadline: Instant) -> String {
        let left = deadline.saturating_duration_since(Instant::now());
        self.lines.recv_timeout(left).unwrap_or_else(|e| panic!("the mock received nothing more: {e}"))
    }

    /// What the mock has printed by now. For use once the client has finished: the mock prints a
    /// message before it answers it, so only the pipe is left to wait for.
    fn rest(&self) -> Vec<String> {
        std::iter::from_fn(|| self.lines.recv_timeout(Duration::from_millis(300)).ok()).collect()
    }
}

/// A fresh home folder for the binary, removed at the end.
struct TempHome(PathBuf);

impl TempHome {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("openlaunch-it-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A line of the mock as its kind and message; a `BAD` line (merged messages) fails the test.
fn parse(line: &str) -> (&str, Value) {
    assert!(!line.starts_with("BAD"), "the mock couldn't read a message: {line}");
    let (kind, msg) = line.split_once(' ').unwrap_or_else(|| panic!("unexpected line from the mock: {line}"));
    (kind, serde_json::from_str(msg).unwrap_or_else(|e| panic!("{e} in the line from the mock: {line}")))
}

/// Each field of `msg` has the value it should have.
fn assert_fields(msg: &Value, want: &[(&str, Value)]) {
    for (pointer, value) in want {
        assert_eq!(msg.pointer(pointer), Some(value), "field {pointer} in {msg}");
    }
}

/// What every message and its options say; `ready` for both ready flags, as `gspro.rs` sends them.
fn assert_common(msg: &Value, heartbeat: bool, ready: bool) {
    assert_fields(msg, &[
        ("/DeviceID", json!("OpenLaunchConnect")),
        ("/Units", json!("Yards")),
        ("/APIversion", json!("1")),
        ("/ShotDataOptions/ContainsBallData", json!(!heartbeat)),
        ("/ShotDataOptions/ContainsClubData", json!(!heartbeat)),
        ("/ShotDataOptions/LaunchMonitorIsReady", json!(ready)),
        ("/ShotDataOptions/LaunchMonitorBallDetected", json!(ready)),
        ("/ShotDataOptions/IsHeartBeat", json!(heartbeat)),
    ]);
}

fn assert_heartbeat(msg: &Value, ready: bool) {
    assert_common(msg, true, ready);
    assert_fields(msg, &[("/ShotNumber", json!(0))]);
    for absent in ["BallData", "ClubData"] {
        assert!(msg.get(absent).is_none(), "{absent} in a heartbeat: {msg}");
    }
}

fn assert_shot(msg: &Value) {
    assert_common(msg, false, true);
    assert!(msg["ShotNumber"].as_u64().is_some_and(|n| n > 0), "field /ShotNumber in {msg}");
    let ball = ["Speed", "SpinAxis", "TotalSpin", "BackSpin", "SideSpin", "HLA", "VLA"].map(|f| ("BallData", f));
    let club = ["Speed", "SpeedAtImpact", "AngleOfAttack", "FaceToTarget", "Path"].map(|f| ("ClubData", f));
    for (group, field) in ball.into_iter().chain(club) {
        assert!(msg[group][field].is_number(), "field /{group}/{field} in {msg}");
    }
    assert!(msg["BallData"]["Speed"].as_f64().is_some_and(|s| s > 0.0), "field /BallData/Speed in {msg}");
}

/// `--test-shot`: the mock gets one ready heartbeat and one well-formed shot, nothing else, and the
/// bridge understands the replies. Its exit code alone says nothing: it is 0 even after a 501.
#[test]
fn test_shot_of_the_binary_reaches_the_mock() {
    let mock = Mock::start();
    let home = TempHome::new("test-shot");
    let mut bridge = Command::new(env!("CARGO_BIN_EXE_openlaunch"))
        .args(["--test-shot", "--gspro", &mock.addr()])
        .env("HOME", &home.0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map(Killed)
        .unwrap();
    let mut stderr = bridge.0.stderr.take().unwrap();
    let log = std::thread::spawn(move || {
        let mut log = String::new();
        stderr.read_to_string(&mut log).unwrap();
        log
    });

    let start = Instant::now();
    let status = loop {
        if let Some(status) = bridge.0.try_wait().unwrap() {
            break status;
        }
        assert!(start.elapsed() < DEADLINE, "the bridge didn't finish within {DEADLINE:?}");
        std::thread::sleep(Duration::from_millis(50));
    };
    let log = log.join().unwrap();
    let lines = mock.rest();

    for line in &lines {
        parse(line);
    }
    assert_eq!(lines.len(), 2, "the mock should get one heartbeat and one shot: {lines:?}\nbridge: {log}");
    let (first, heartbeat) = parse(&lines[0]);
    let (second, shot) = parse(&lines[1]);
    assert_heartbeat(&heartbeat, true);
    assert_shot(&shot);
    assert_eq!((first, second), ("heartbeat", "SHOT"), "the mock's lines: {lines:?}");

    assert!(status.success(), "{status}\nbridge: {log}");
    assert!(log.contains("GSPro acknowledged the shot (Ball Data received)"), "bridge: {log}");
    assert!(log.contains("Player: RH, club: I7"), "bridge: {log}");
    assert!(!log.contains('⚠'), "bridge: {log}");
}

/// `Link::run`: after the heartbeat of the connection and the one for "ready", one more comes on its
/// own a heartbeat interval later. Dropping the sender ends the thread.
#[test]
fn link_sends_the_periodic_heartbeat() {
    let mock = Mock::start();
    let (tx, rx) = mpsc::channel();
    let link = Link::new(mock.addr(), false);
    let thread = std::thread::spawn(move || link.run(&rx));
    tx.send(Cmd::Ready(true)).unwrap();

    let deadline = Instant::now() + DEADLINE;
    let heartbeat = |ready| {
        let line = mock.next_line(deadline);
        let (kind, msg) = parse(&line);
        assert_heartbeat(&msg, ready);
        assert_eq!(kind, "heartbeat", "the mock's line: {line}");
        Instant::now()
    };
    heartbeat(false);
    let ready_at = heartbeat(true);
    let periodic_at = heartbeat(true);
    let gap = periodic_at - ready_at;
    assert!(gap >= MIN_PERIOD, "the third heartbeat came only {gap:?} after the second");

    drop(tx);
    while !thread.is_finished() {
        assert!(Instant::now() < deadline, "dropping the sender didn't end the link's thread");
        std::thread::sleep(Duration::from_millis(50));
    }
    thread.join().unwrap();
    let rest = mock.rest();
    assert!(rest.is_empty(), "the mock got more than the three heartbeats: {rest:?}");
}
