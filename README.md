<p align="center">
  <img src="docs/images/banner.png" alt="OpenLaunch Connect – Garmin Approach R10 to GSPro on macOS" width="100%">
</p>

<p align="center">
  <a href="https://github.com/chgl42/openlaunch-connect/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/chgl42/openlaunch-connect?label=release"></a>
  <a href="https://github.com/chgl42/openlaunch-connect/actions/workflows/build.yml"><img alt="Build" src="https://github.com/chgl42/openlaunch-connect/actions/workflows/build.yml/badge.svg"></a>
  <img alt="Platform" src="https://img.shields.io/badge/macOS-Apple%20Silicon-blue">
  <img alt="License" src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-green">
</p>

**OpenLaunch Connect** (formerly *OpenR10 Connect*) is a small, native macOS app that connects a **Garmin Approach R10** launch monitor
directly via Bluetooth and forwards every shot to **GSPro** through its OpenConnect API – including GSPro
running under **CrossOver** on an Apple Silicon Mac.

> Not affiliated with, endorsed by, or sponsored by Garmin or GSPro.

The Garmin Approach R10 is the first supported launch monitor; the app is built so that more can follow.

## Why this exists

GSPro is Windows-only. On a Mac it runs well under CrossOver – but the Garmin R10 talks Bluetooth LE,
and CrossOver/Wine does not pass Bluetooth through to Windows programs. The Windows connectors that talk
to the R10 directly therefore can't reach it.

The usual workaround is a chain of devices and apps:

```
R10 ──BLE──> iPhone (Garmin Golf app) ──Wi-Fi──> "E6 Connect" emulation (gspro-r10) ──> GSPro
```

It works, but it is fiddly and fragile: the phone has to stay awake and in the foreground, the Garmin app
has to be put into E6 mode every session, and any of the three hops can drop.

**OpenLaunch Connect removes the detour.** It does the Bluetooth part natively on macOS, where it works, and
hands the shots to GSPro over a local TCP connection, which CrossOver handles perfectly:

```
R10 ──BLE──> OpenLaunch Connect (macOS) ──TCP 127.0.0.1:921──> GSPro (CrossOver)
```

One app, a stable connection, no phone in the loop.

## Features

- **Direct Bluetooth connection** to the R10 – no iPhone, no Garmin app, no E6
- **Automatic reconnects** on both sides (R10 and GSPro); start them in any order
- **All shot data the R10 measures**: ball speed, launch angle (VLA), launch direction (HLA), total spin,
  spin axis, back/side spin, club speed, angle of attack, club path, face to target, face to path
- **Club and handedness from GSPro** shown live, plus **smash factor**
- **Shot history** with per-club averages and spread (session or all time)
- **CSV export** in three layouts: OpenLaunch, **Garmin Golf** app or **Awesome Golf** (same columns and units row as theirs)
- **In-game overlay** on top of GSPro – also in full screen – with status, club, battery and the last shot;
  place it in any corner or at the middle of any edge
- **Swing tempo** (backswing : downswing, e.g. 3.0 : 1) per shot, per club and in the overlay
- **R10 battery level** and device warnings (tilted, overheating, radar saturation)
- **macOS notifications** while GSPro is in front: low battery, R10 errors, lost connections, shots not delivered
- **Test shot** button to check the GSPro side without hitting a ball
- **Start GSPro** from the app (finds the CrossOver launcher), optionally automatically
- **English and German** interface (follows the macOS language, or choose in Settings)
- Metric or imperial display, built-in log, settings persisted per user
- Interface styled after GSPro's own in-game look

<p align="center">
  <img src="docs/images/screenshot-play.png" alt="Play view with the last shot and READY status" width="80%">
</p>

<p align="center">
  <img src="docs/images/screenshot-shots.png" alt="Shots page with per-club statistics and the shot list" width="80%">
</p>

<p align="center">
  <img src="docs/images/screenshot-overlay.png" alt="Overlay shown on top of GSPro" width="40%">
</p>

## Requirements

- Mac with **Apple Silicon** (macOS 12 or newer)
- **Garmin Approach R10**
- **GSPro** with a license for the R10 (e.g. running in CrossOver)

## Installation

### Option A – install script (recommended)

```bash
curl -fsSL https://raw.githubusercontent.com/chgl42/openlaunch-connect/main/install.sh | bash
```

Downloads the latest release, installs it to `/Applications` and removes the download quarantine.

### Option B – manual download

1. Download `OpenLaunch-Connect-macos-arm64.zip` from the [latest release](https://github.com/chgl42/openlaunch-connect/releases/latest).
2. Unzip and move **OpenLaunch Connect.app** to `/Applications`.
3. The app is ad-hoc signed but not notarized, so Gatekeeper blocks it on first launch. Run once:
   ```bash
   xattr -dr com.apple.quarantine "/Applications/OpenLaunch Connect.app"
   ```

On first start macOS asks **"OpenLaunch Connect would like to use Bluetooth"** → *Allow*.

### Option C – build from source

Requires [Rust](https://rustup.rs) 1.94 or newer.

```bash
git clone https://github.com/chgl42/openlaunch-connect.git
cd openlaunch-connect
./make-app.sh --install
```

## Usage

1. **Turn off Bluetooth on your iPhone** (or quit the Garmin Golf app) – otherwise the phone grabs the R10.
2. Switch on the R10. The very first time, hold the button until the LED is **solid blue** (pairing mode).
3. Start **OpenLaunch Connect**. If macOS shows a pairing request, confirm it.
4. Start GSPro (button **START GSPRO** or automatically, see settings) and click *Play* in the launcher.
5. When the status panel shows a green **READY**, hit away.

| Status | Meaning |
|---|---|
| **READY** (green) | R10 armed and GSPro connected |
| **WAITING FOR GSPRO** / **R10 NOT READY** (amber) | one side is connected, the other isn't ready yet |
| **NO R10** (red) | searching for the R10 – also shown a few seconds after the R10 is switched off; the app reconnects on its own |

Hover over the status panel for details; the **LOG** page shows everything that happens.

**Swing tempo:** next to the buttons the play page shows the tempo of the last swing (backswing : downswing,
e.g. *3.0 : 1*, with both times). It needs a real swing – test shots have none.

**Shots page:** every shot of the session (or all time) with averages and spread per club. *EXPORT CSV* writes
the shots shown, in the layout picked next to the button: OpenLaunch, Garmin Golf or Awesome Golf.

**Overlay:** while GSPro runs in full screen, a small panel on top of it shows the status, club, battery,
tempo and the last shot – or what's wrong. It ignores the mouse, so it never gets in the way; choose its
position (or turn it off) in the settings.

**Updates:** on start the app asks GitHub for the latest release (nothing else is sent) and shows *Update x.y.z*
in the top bar when there is a newer one – click it to open the release page. You can turn the check off in the settings.

**Notifications:** when GSPro is in front, macOS notifications report a low R10 battery, R10 errors (e.g. tilted),
a lost R10 or GSPro connection and shots that didn't reach GSPro – each at most once a minute.

**Test shot:** click **TEST SHOT IN 3 S** and switch to the GSPro window – a slightly randomised 7-iron
is sent after three seconds. GSPro only processes shots while its window is active and a ball is teed up
(range or course, not a menu).

## Settings

<p align="center">
  <img src="docs/images/screenshot-settings.png" alt="Settings page" width="80%">
</p>

| Group | Setting | Description |
|---|---|---|
| GSPro | **Launcher** | CrossOver launcher of GSPro. *Find* searches `~/Applications/CrossOver/…`; *Choose …* picks it manually. |
| | **Open GSPro automatically on start** | Starts GSPro together with the app (never twice). |
| | **Address** | OpenConnect address (default `127.0.0.1:921`). Applies after restart. |
| R10 | **Distance to ball** | Sent to the R10 (default 2.10 m). Applies after restart. |
| Display | **Language** | System (follows macOS), English or Deutsch. |
| | **Units** | Metric (km/h) or imperial (mph) – display only; GSPro always gets mph. |
| | **Overlay** | Position on the screen – any corner or the middle of any edge; the middle cell turns it off. |
| Notifications & updates | **Notifications** | On/off. *Test* sends one right away – if none appears, allow notifications for OpenLaunch Connect in *System Settings → Notifications*. |
| | **Updates** | Check GitHub for a newer release on start. |

*Log GSPro messages* (every message exchanged with GSPro) is switched on the log page.

Changes apply with **SAVE**; the notification and debug-log switches take effect immediately.

Settings are stored in `~/Library/Application Support/OpenLaunch Connect/settings.json`, the shot history
in `shots.jsonl` in the same folder (one JSON object per shot; test shots are not recorded).

## Troubleshooting

- **NO R10 stays red** – Is the R10 on? Is iPhone Bluetooth off? Check *System Settings → Privacy & Security → Bluetooth* and allow OpenLaunch Connect.
- **R10 shows "Device error"** – usually the R10 is not level. Put it on a flat surface.
- **"No handshake"** – a macOS pairing dialog may be hidden; otherwise switch the R10 off and on.
- **No notifications** – click *Test* in the settings; if nothing appears, allow notifications for OpenLaunch Connect in *System Settings → Notifications* (macOS shows them only while the app is in the background).
- **No tempo shown** – the R10 didn't time that swing (e.g. very slow swings, or a test shot).
- **WAITING FOR GSPRO** – GSPro isn't running or its connect window is closed. Only one OpenConnect client can be connected – quit other connectors (e.g. gspro-r10).
- **Shot sent but no ball flight** – GSPro must be the active window with a ball teed up.
- macOS does **not** list the R10 in the Bluetooth settings – that's normal for this kind of BLE device; the app connects on its own.

## Command-line version

The same bridge is available without a window:

```bash
cargo build --release --bin openlaunch
./target/release/openlaunch                # bridge; type t + Enter for a test shot
./target/release/openlaunch --test-shot    # check the GSPro connection only
./target/release/openlaunch --help
```

## How it works

- **Bluetooth:** the R10 protocol (BLE → MultiLink → GFDI → protobuf) is handled by
  [10over](https://github.com/divotmaker/10over) by Eric Thill, vendored in `vendor/tenover` with small
  additions (see [PATCHES.md](vendor/tenover/PATCHES.md)). On macOS it uses CoreBluetooth via btleplug.
- **GSPro:** shots are sent to GSPro's OpenConnect API as JSON. Conversion follows the long-proven
  [gspro-r10](https://github.com/mholow/gsp-r10-adapter): speeds m/s → mph, spin axis inverted,
  back/side spin derived from total spin and axis.
- GSPro expects exactly one JSON message per TCP read and ignores repeated shot numbers – the app sends
  messages one at a time and numbers shots uniquely across restarts.
- The R10 does not measure loft, lie, closure rate or impact location; those fields stay empty in GSPro.
- The UI is built with [egui](https://github.com/emilk/egui).

## Development

- `cargo test --all-targets` runs the unit tests (also on Linux; BlueZ needs `libdbus-1-dev`).
- `tools/mock_gspro.py [port]` is a tiny GSPro OpenConnect stand-in; point the app at it with the
  GSPro address setting or `openlaunch --gspro 127.0.0.1:<port>`.
- `OPENLAUNCH_DEMO=1` simulates an R10 (READY, a few sample shots).
- README screenshots are rendered offscreen (no window needed):
  `cargo test --bin openlaunch-connect readme_screenshots -- --ignored`, then `python3 tools/banner.py .` for the banner.

## Releases

What changed in each version: [CHANGELOG.md](CHANGELOG.md).

GitHub Actions builds the app on an Apple Silicon runner for every push. Tagging a version creates a release:

```bash
git tag v0.7.0 && git push origin v0.7.0
```

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
Third-party licenses are listed in `THIRD_PARTY_LICENSES.html` (inside the app bundle and attached to each release).
See [NOTICE](NOTICE).

Garmin and Approach are trademarks of Garmin Ltd. or its subsidiaries. GSPro is a trademark of its respective owner.
This is an independent community project – use at your own risk.
