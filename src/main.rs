//! openlaunch — command-line version of OpenLaunch Connect.
//!
//! R10 ──BLE──> openlaunch ──TCP 127.0.0.1:921──> GSPro

use std::thread;
use std::time::{Duration, Instant};

use clap::Parser;
use openlaunch_connect::engine::{self, Config};
use openlaunch_connect::gspro::{Cmd, Link};
use openlaunch_connect::log;

#[derive(Parser, Debug)]
#[command(
    name = "openlaunch",
    version,
    about = "Garmin Approach R10 (Bluetooth) → GSPro OpenConnect – no iPhone, no E6"
)]
struct Args {
    /// Address of the GSPro OpenConnect API
    #[arg(long, default_value = "127.0.0.1:921")]
    gspro: String,

    /// Distance from R10 to ball in metres (sent to the R10; 0 = don't send)
    #[arg(long, default_value_t = 2.1)]
    tee_distance: f32,

    /// Only connect to this Bluetooth peripheral id (macOS UUID from the log)
    #[arg(long)]
    device: Option<String>,

    /// Also forward swings without measured ball speed (default: drop them)
    #[arg(long)]
    send_empty: bool,

    /// Send one test shot to GSPro and exit (checks the GSPro side only, no R10 needed)
    #[arg(long)]
    test_shot: bool,

    /// Print raw JSON messages to/from GSPro
    #[arg(long)]
    debug: bool,
}

fn main() {
    let args = Args::parse();

    // A test shot records nothing, so it runs beside a running bridge.
    if args.test_shot {
        std::process::exit(run_test_shot(&args));
    }
    if let Err(e) = openlaunch_connect::settings::Settings::claim_data_dir() {
        eprintln!("openlaunch: {e}");
        std::process::exit(1);
    }

    log::info(&format!("openlaunch {} – R10 -> GSPro ({})", env!("CARGO_PKG_VERSION"), args.gspro));

    let tx = engine::start(Config {
        gspro_addr: args.gspro.clone(),
        tee_distance: args.tee_distance,
        device: args.device.clone(),
        send_empty: args.send_empty,
        debug: args.debug,
        demo: false,
    });

    // Keyboard: "t" + Enter sends a test shot over the live connection.
    log::info("Tip: type »t« + Enter to send a test shot to GSPro.");
    let stdin = std::io::stdin();
    let mut line = String::new();
    while stdin.read_line(&mut line).is_ok_and(|n| n > 0) {
        if matches!(line.trim(), "t" | "T" | "test") {
            log::info("Test shot (7-iron) over the live connection …");
            let _ = tx.send({ let (ball, club) = engine::test_shot(); Cmd::Shot { ball, club: Some(club), tempo: None, test: true } });
        }
        line.clear();
    }

    // stdin closed (e.g. started in the background): keep bridging.
    loop {
        thread::park();
    }
}

/// Send one realistic 7-iron shot to GSPro and report whether it arrived.
fn run_test_shot(args: &Args) -> i32 {
    log::info(&format!("Test shot to {} …", args.gspro));
    let mut link = Link::new(args.gspro.clone(), true);
    link.handle(Cmd::Ready(true)); // so the initial heartbeat already reports "ready"
    link.ensure_connected();
    if !link.is_connected() {
        log::info("❌ GSPro not reachable. Start GSPro, keep its Connect window open, check port 921.");
        return 1;
    }
    link.handle({ let (ball, club) = engine::test_shot(); Cmd::Shot { ball, club: Some(club), tempo: None, test: true } });
    let until = Instant::now() + Duration::from_secs(3);
    while Instant::now() < until && link.is_connected() {
        link.read_responses();
        thread::sleep(Duration::from_millis(100));
    }
    log::info("Done. The ball should have flown in GSPro (about a 7-iron, ~165 yd).");
    0
}
