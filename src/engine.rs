//! The bridge itself: R10 thread (Bluetooth) + GSPro thread (OpenConnect).

use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant};

use tenover::ble::BleTransport;
use tenover::proto::{DeviceState, ErrorCode, ShotConfig};
use tenover::{Client, Event};

use crate::gspro::{Cmd, Link};
use crate::state::{self, R10Status};
use crate::notify::{self, Alert};
use crate::{log, mapping};

const BLE_RETRY: Duration = Duration::from_secs(5);
/// Battery level (percent) at which the app warns; matches the red battery glyph.
const LOW_BATTERY: u8 = 15;

#[derive(Debug, Clone)]
pub struct Config {
    /// GSPro OpenConnect address, e.g. `127.0.0.1:921`.
    pub gspro_addr: String,
    /// Distance R10 → ball in metres (0 = don't send).
    pub tee_distance: f32,
    /// Only connect to this BLE peripheral id.
    pub device: Option<String>,
    /// Forward swings without ball speed.
    pub send_empty: bool,
    /// Log raw OpenConnect JSON.
    pub debug: bool,
    /// Simulate an R10 instead of using Bluetooth (for screenshots and UI work).
    pub demo: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            gspro_addr: "127.0.0.1:921".into(),
            tee_distance: 2.1,
            device: None,
            send_empty: false,
            debug: false,
            demo: false,
        }
    }
}

/// Start both bridge threads. The returned sender can inject commands
/// (e.g. test shots); the bridge runs for the lifetime of the process.
pub fn start(cfg: Config) -> Sender<Cmd> {
    tenover::set_diagnostics(|msg| log::r10(&format!("BLE: {msg}")));
    crate::history::PERSIST.store(!cfg.demo, std::sync::atomic::Ordering::Relaxed);
    let (tx, rx) = mpsc::channel::<Cmd>();
    let link = Link::new(cfg.gspro_addr.clone(), cfg.debug);
    // GSPro's own results of the shots, from its database (not in the demo: nothing to match).
    if !cfg.demo {
        crate::gspro_db::watch();
    }
    thread::Builder::new()
        .name("gspro".into())
        .spawn(move || link.run(&rx))
        .expect("spawn gspro thread");

    let r10_tx = tx.clone();
    thread::Builder::new()
        .name("r10".into())
        .spawn(move || if cfg.demo { demo_loop(&r10_tx) } else { r10_loop(&cfg, &r10_tx) })
        .expect("spawn r10 thread");

    tx
}

/// A realistic 7-iron for testing the GSPro side without an R10.
///
/// Every call varies the values slightly: GSPro ignores a shot that is an
/// exact repeat of the previous one, so identical test shots would not fly.
#[must_use]
pub fn test_shot() -> (mapping::GsBall, mapping::GsClub) {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1, |d| d.subsec_nanos() ^ (d.as_secs() as u32).rotate_left(16))
        | 1;
    let mut state = seed;
    // xorshift32 → value in [-1, 1]
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        f64::from(state) / f64::from(u32::MAX) * 2.0 - 1.0
    };
    let r1 = |v: f64| (v * 10.0).round() / 10.0;

    let speed = r1(120.0 + 4.0 * next());
    let vla = r1(16.3 + 1.2 * next());
    let hla = r1(1.0 + 1.5 * next());
    let total_spin = (7000.0 + 400.0 * next()).round();
    let spin_axis = r1(2.0 + 2.5 * next());
    let ball = mapping::GsBall {
        speed,
        spin_axis,
        total_spin,
        back_spin: (total_spin * spin_axis.to_radians().cos()).round(),
        side_spin: (total_spin * spin_axis.to_radians().sin()).round(),
        hla,
        vla,
    };
    let club_speed = r1(speed / 1.41);
    let club = mapping::GsClub {
        speed: club_speed,
        speed_at_impact: club_speed,
        angle_of_attack: r1(-4.0 + 1.0 * next()),
        face_to_target: r1(0.5 + 1.0 * next()),
        path: r1(-1.0 + 1.5 * next()),
    };
    (ball, club)
}

/// Simulated R10: ready, 82 % battery, one shot after two seconds.
fn demo_loop(tx: &Sender<Cmd>) {
    log::r10("Demo mode – simulating an R10");
    state::set_r10(R10Status::Ready, "Ready – hit away (demo)");
    state::with(|s| {
        s.r10_battery = Some(82);
        s.r10_device = Some("E4:51:2A:7C:19:0B".into());
        // OPENLAUNCH_DEMO=range: a whole range session to try the shots explorer with.
        s.session = if std::env::var("OPENLAUNCH_DEMO").as_deref() == Ok("range") { demo_range() } else { demo_session() };
    });
    let _ = tx.send(Cmd::Ready(true));
    thread::sleep(Duration::from_secs(2));
    let (ball, club) = test_shot();
    let tempo = Some(mapping::Tempo { backswing: 0.81, downswing: 0.27 });
    let _ = tx.send(Cmd::Shot { ball, club: Some(club), tempo, test: false });
    loop {
        thread::park();
    }
}

/// A few earlier shots with different clubs, so the SHOTS page has something to show
/// (demo mode and README screenshots).
#[must_use]
pub fn demo_session() -> Vec<crate::history::ShotRecord> {
    // (club, ball speed factor vs. the 7-iron, launch angle offset, spin factor)
    let clubs = [("DR", 1.24, -5.5, 0.38), ("DR", 1.22, -5.0, 0.4), ("DR", 1.25, -6.0, 0.36), ("I7", 1.0, 0.0, 1.0),
        ("I7", 1.0, 0.0, 1.0), ("I7", 1.0, 0.0, 1.0), ("PW", 0.84, 8.0, 1.25), ("PW", 0.84, 8.0, 1.25)];
    clubs
        .iter()
        .enumerate()
        .map(|(i, &(code, speed, vla, spin))| {
            let (mut ball, mut club) = test_shot();
            ball.speed = (ball.speed * speed * 10.0).round() / 10.0;
            ball.vla = ((ball.vla + vla) * 10.0).round() / 10.0;
            ball.total_spin = (ball.total_spin * spin).round();
            ball.back_spin = (ball.back_spin * spin).round();
            ball.side_spin = (ball.side_spin * spin).round();
            club.speed = (ball.speed / (1.41 + (speed - 1.0) * 0.3) * 10.0).round() / 10.0;
            club.speed_at_impact = club.speed;
            // Spread the test_shot seeds (time-based) a little.
            thread::sleep(Duration::from_millis(3));
            let mut rec = crate::history::ShotRecord::new(ball, Some(club), Some(code.into()), true, i as u32 + 1);
            rec.time = format!("2026-10-19T18:{:02}:{:02}+02:00", 10 + i * 2, (i * 17) % 60);
            // Tempo varies a little around 3:1, slightly quicker with the wedge.
            let back = [0.84, 0.80, 0.86, 0.78, 0.81, 0.83, 0.74, 0.76][i];
            let down = [0.27, 0.28, 0.27, 0.26, 0.28, 0.27, 0.26, 0.25][i];
            rec.tempo = Some(mapping::Tempo { backswing: back, downswing: down });
            rec
        })
        .collect()
}

/// A range session of 65 shots with a full bag of fourteen clubs, with realistic spread (the same every time):
/// face and path vary, start direction and spin axis follow from them.
#[must_use]
pub fn demo_range() -> Vec<crate::history::ShotRecord> {
    // (club, shots, ball mph, VLA, spin, club mph, AoA, path, face to path)
    let clubs = [
        ("DR", 8, 145.0, 11.5, 2700.0, 103.0, 1.5, -2.0, 2.5),
        ("W3", 5, 136.0, 12.0, 3500.0, 97.0, -1.0, -1.5, 2.0),
        ("W5", 4, 131.0, 12.8, 4000.0, 94.0, -1.5, -1.4, 1.9),
        ("H3", 4, 128.0, 13.0, 4300.0, 91.0, -2.0, -1.3, 1.8),
        ("H4", 4, 126.0, 13.5, 4600.0, 90.0, -2.2, -1.3, 1.8),
        ("I5", 5, 123.0, 14.0, 5200.0, 88.0, -2.5, -1.2, 1.7),
        ("I6", 5, 120.0, 15.0, 6000.0, 86.0, -3.0, -1.1, 1.6),
        ("I7", 6, 117.0, 16.5, 6800.0, 83.0, -3.5, -1.0, 1.5),
        ("I8", 4, 112.0, 18.0, 7400.0, 80.0, -3.8, -0.9, 1.3),
        ("I9", 4, 106.0, 20.0, 8000.0, 76.0, -4.0, -0.8, 1.2),
        ("PW", 5, 98.0, 24.0, 8800.0, 72.0, -4.5, -0.5, 1.0),
        ("GW", 4, 91.0, 27.0, 9200.0, 68.0, -4.8, -0.4, 0.9),
        ("SW", 4, 80.0, 30.0, 9500.0, 63.0, -5.0, -0.3, 0.8),
        ("LW", 3, 68.0, 34.0, 9800.0, 57.0, -5.2, -0.2, 0.7),
    ];
    let mut state: u32 = 0x9e37_79b9;
    // xorshift32 → value in [-1, 1]
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        f64::from(state) / f64::from(u32::MAX) * 2.0 - 1.0
    };
    let r1 = |v: f64| (v * 10.0).round() / 10.0;
    let mut out = Vec::new();
    for (code, shots, speed, vla, spin, club_speed, aoa, path, f2p) in clubs {
        for _ in 0..shots {
            let n = out.len();
            let strike = 1.0 + 0.025 * next();
            let path = r1(path + 1.6 * next());
            let face_to_path = f2p + 1.4 * next();
            let face = r1(path + face_to_path);
            let total_spin = (spin * (1.0 + 0.08 * next())).round();
            let spin_axis = r1(2.2 * face_to_path + 1.5 * next());
            let ball = mapping::GsBall {
                speed: r1(speed * strike),
                spin_axis,
                total_spin,
                back_spin: (total_spin * spin_axis.to_radians().cos()).round(),
                side_spin: (total_spin * spin_axis.to_radians().sin()).round(),
                hla: r1(0.8 * face + 0.2 * path + 0.6 * next()),
                vla: r1(vla + 1.5 * next()),
            };
            let cs = r1(club_speed * (1.0 + 0.02 * next()));
            let club = mapping::GsClub { speed: cs, speed_at_impact: cs, angle_of_attack: r1(aoa + 1.2 * next()), face_to_target: face, path };
            let mut rec = crate::history::ShotRecord::new(ball, Some(club), Some(code.into()), n != 23, n as u32 + 1);
            let secs = 17 * 3600 + n * 75;
            rec.time = format!("2026-10-21T{:02}:{:02}:{:02}+02:00", secs / 3600, secs / 60 % 60, secs % 60);
            rec.tempo = Some(mapping::Tempo { backswing: ((0.81 + 0.05 * next()) * 100.0).round() / 100.0, downswing: 0.27 });
            // As if GSPro had sent its numbers: a little shorter and higher than the estimate.
            let f = crate::flight::fly(&rec.ball);
            rec.gspro = Some(crate::gspro_db::GsproResult {
                carry: f.carry * 0.96,
                total: f.total * 0.95,
                side: f.total_side * 1.05,
                apex: f.apex * 1.18,
                descent: f.descent + 4.0,
                to_pin: None,
            });
            out.push(rec);
        }
    }
    out
}

/// Connect to the R10, forward events, reconnect forever.
fn r10_loop(cfg: &Config, tx: &Sender<Cmd>) {
    loop {
        state::set_r10(R10Status::Searching, "Searching for R10 …");
        log::r10("Searching for R10 … (switch it on; turn off iPhone Bluetooth so it doesn't grab the R10)");
        let transport = match &cfg.device {
            Some(id) => connect_by_id(id),
            None => BleTransport::auto_connect().map_err(|e| e.to_string()),
        };
        let transport = match transport {
            Ok(t) => t,
            Err(e) => {
                state::set_r10(R10Status::Searching, "Not found – R10 on? iPhone Bluetooth off?");
                log::r10(&format!("No R10 connected: {e}"));
                log::r10(&format!("Retrying in {} s …", BLE_RETRY.as_secs()));
                thread::sleep(BLE_RETRY);
                continue;
            }
        };

        state::set_r10(R10Status::Connected, "Connected, handshake in progress …");
        log::r10(&format!("Bluetooth connected ({})", transport.device_address()));
        let device = transport.device_address().to_string();
        state::with(|s| s.r10_device = Some(device));
        let battery = transport.battery_handle();
        let mtu = usize::from(transport.mtu());
        let mut client = Client::new(transport, mtu);
        if let Err(e) = client.start() {
            log::r10(&format!("Registration failed: {e}"));
            thread::sleep(BLE_RETRY);
            continue;
        }

        run_session(cfg, &mut client, tx, battery.as_deref());

        let _ = tx.send(Cmd::Ready(false));
        state::with(|s| {
            s.r10_battery = None;
            s.r10_device = None;
        });
        state::set_r10(R10Status::Searching, "Connection lost – reconnecting …");
        notify::alert(Alert::R10Lost, "R10 connection lost");
        log::r10(&format!("Lost connection to R10 – retrying in {} s …", BLE_RETRY.as_secs()));
        thread::sleep(BLE_RETRY);
    }
}

#[cfg(not(target_os = "linux"))]
fn connect_by_id(id: &str) -> Result<BleTransport, String> {
    tenover::btleplug::BtleplugTransport::connect(id)
        .map(BleTransport::Btleplug)
        .map_err(|e| e.to_string())
}

#[cfg(target_os = "linux")]
fn connect_by_id(id: &str) -> Result<BleTransport, String> {
    tenover::bluez::BlueZTransport::connect(id)
        .map(BleTransport::BlueZ)
        .map_err(|e| e.to_string())
}

/// Poll one connected R10 session until the transport fails.
fn run_session<T: tenover::Transport>(
    cfg: &Config,
    client: &mut Client<T>,
    tx: &Sender<Cmd>,
    battery: Option<&std::sync::atomic::AtomicU8>,
) {
    let mut last_battery: Option<u8> = None;
    let mut logged_battery: Option<u8> = None;
    let mut config_sent = false;
    let mut ready_logged = false;
    let mut handshake_deadline = Some(Instant::now() + Duration::from_secs(30));

    loop {
        if let Some(deadline) = handshake_deadline
            && Instant::now() > deadline
        {
            log::r10("⚠️  No handshake from R10 after 30 s. Confirm the macOS pairing dialog or restart the R10.");
            state::set_r10(R10Status::Connected, "No handshake – confirm pairing dialog?");
            handshake_deadline = None;
        }

        if let Some(level) = battery.map(|b| b.load(std::sync::atomic::Ordering::Relaxed))
            && level <= 100
            && last_battery != Some(level)
        {
            // Log the first reading and then every 5 % step.
            if logged_battery.is_none_or(|prev| prev.abs_diff(level) >= 5) {
                log::r10(&format!("Battery: {level} %"));
                logged_battery = Some(level);
            }
            if level <= LOW_BATTERY && last_battery.is_none_or(|prev| prev > LOW_BATTERY) {
                notify::alert(Alert::BatteryLow, &format!("R10 battery low ({level} %)"));
            }
            last_battery = Some(level);
            state::with(|s| s.r10_battery = Some(level));
        }

        match client.poll() {
            Ok(Some(event)) => match event {
                Event::Registered { .. } => log::r10("Registered, handshake in progress …"),
                Event::HandshakeComplete => {
                    handshake_deadline = None;
                    log::r10("Handshake complete");
                    state::set_r10(R10Status::Connected, "Connected, arming …");
                }
                Event::Subscribed { success } if !success => {
                    log::r10("⚠️  R10 rejected the shot data subscription");
                }
                Event::Subscribed { .. } | Event::WakeUpResponse { .. } => {}
                Event::ShotConfigResponse { success } => {
                    if success {
                        log::r10(&format!("Tee distance set ({:.2} m)", cfg.tee_distance));
                    } else {
                        log::r10("⚠️  R10 rejected the tee distance (harmless)");
                    }
                }
                Event::Ready => {
                    if !config_sent && cfg.tee_distance > 0.0 {
                        config_sent = true;
                        let sc = ShotConfig { tee_range: Some(cfg.tee_distance), ..ShotConfig::default() };
                        if let Err(e) = client.send_shot_config(&sc) {
                            log::r10(&format!("Could not send tee distance: {e}"));
                        }
                    }
                    if !ready_logged {
                        log::r10("🟢 Ready – hit away");
                        ready_logged = true;
                    }
                    state::set_r10(R10Status::Ready, "Ready – hit away");
                    let _ = tx.send(Cmd::Ready(true));
                }
                Event::StateChange(dev_state) => {
                    ready_logged = false;
                    match dev_state {
                        DeviceState::Standby => {
                            log::r10("Standby – waking R10 …");
                            state::set_r10(R10Status::Connected, "Standby – waking …");
                            let _ = tx.send(Cmd::Ready(false));
                        }
                        DeviceState::Error => {
                            log::r10("⚠️  R10 reports a device error (not armed)");
                            state::set_r10(R10Status::Connected, "Device error");
                            let _ = tx.send(Cmd::Ready(false));
                        }
                        DeviceState::Recording | DeviceState::Processing => {
                            state::set_r10(R10Status::Ready, "Measuring shot …");
                        }
                        DeviceState::InterferenceTest => {
                            log::r10("Interference test running …");
                            state::set_r10(R10Status::Connected, "Interference test …");
                        }
                        DeviceState::Waiting => {}
                    }
                }
                Event::DeviceError(err) => {
                    let what = match err.code {
                        ErrorCode::PlatformTilted => {
                            let tilt = err
                                .tilt
                                .map(|(r, p)| format!(" (Roll {r:.1}°, Pitch {p:.1}°)"))
                                .unwrap_or_default();
                            // Detail last, so the UI can translate the text (i18n::tr_detail).
                            format!("R10 is tilted – please level it{tilt}")
                        }
                        ErrorCode::Overheating => "R10 overheating".to_string(),
                        ErrorCode::RadarSaturation => "Radar saturated (too close to wall/net?)".to_string(),
                        ErrorCode::Unknown => {
                            // Raw code and tilt in the log, to learn what the R10 means.
                            let tilt = err
                                .tilt
                                .map(|(r, p)| format!(", Roll {r:.1}°, Pitch {p:.1}°"))
                                .unwrap_or_default();
                            log::r10(&format!("R10 error code {}{tilt}", err.raw_code));
                            "Unknown device error".to_string()
                        }
                    };
                    log::r10(&format!("⚠️  {what} [{:?}]", err.severity));
                    notify::alert(Alert::DeviceError, &what);
                    state::set_r10(R10Status::Connected, what);
                }
                Event::Shot(shot) => handle_shot(cfg, &shot, tx),
            },
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(tenover::Error::Transport(e)) => {
                log::r10(&format!("Bluetooth error: {e}"));
                return;
            }
            Err(e) => log::r10(&format!("Protocol warning: {e}")),
        }
    }
}

fn handle_shot(cfg: &Config, shot: &tenover::ShotData, tx: &Sender<Cmd>) {
    let Some(b) = &shot.ball else {
        log::r10(&format!("Swing #{} without ball data – not sent", shot.shot_id));
        return;
    };
    if b.ball_speed <= 0.0 && !cfg.send_empty {
        log::r10(&format!("Swing #{} without ball speed – not sent", shot.shot_id));
        return;
    }

    let ball = mapping::ball(b);
    let club = shot.club.as_ref().and_then(mapping::club);

    let mut line = format!(
        "⛳ Shot #{}: ball {:.1} mph | VLA {:.1}° | HLA {:.1}° | spin {:.0} rpm, axis {:.1}°",
        shot.shot_id, ball.speed, ball.vla, ball.hla, ball.total_spin, ball.spin_axis
    );
    if let Some(c) = &club {
        line.push_str(&format!(
            " | club {:.1} mph, face {:.1}°, path {:.1}°, AoA {:.1}°",
            c.speed, c.face_to_target, c.path, c.angle_of_attack
        ));
    }
    let tempo = shot.swing.as_ref().and_then(mapping::tempo);
    if let Some(t) = &tempo {
        line.push_str(&format!(" | tempo {:.1}:1 ({:.2} s / {:.2} s)", t.ratio(), t.backswing, t.downswing));
    }
    log::r10(&line);

    let _ = tx.send(Cmd::Shot { ball, club, tempo, test: false });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_range_is_a_full_session() {
        let shots = demo_range();
        assert_eq!(shots.len(), 65);
        assert_eq!(shots, demo_range(), "the same every time");
        let mut clubs: Vec<_> = shots.iter().filter_map(|s| s.club_code.clone()).collect();
        clubs.dedup();
        assert_eq!(clubs.len(), 14);
        assert!(shots.windows(2).all(|w| w[0].time < w[1].time), "in order");
        assert_eq!(shots.iter().filter(|s| !s.delivered).count(), 1);
    }
}
