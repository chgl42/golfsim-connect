# OpenLaunch Connect – project context for Claude

Native macOS bridge: **Garmin Approach R10 (Bluetooth LE) → GSPro OpenConnect API** (TCP `127.0.0.1:921`).
Purpose: GSPro runs on an Apple Silicon Mac under **CrossOver**; CrossOver/Wine does not pass Bluetooth through to
Windows programs. This app does BLE natively on macOS and talks TCP to GSPro in the bottle.
Replaces the detour R10 → iPhone Garmin app → E6 → gspro-r10.

During private development (0.x) the project was called **OpenR10 Connect**; renamed because more launch
monitors are planned. The public repo starts at 1.0.0 with a single commit. R10-specific is essentially `engine.rs`; device names ("R10") stay in the UI where the device is meant.

Open-source project (MIT OR Apache-2.0), repo `chgl42/openlaunch-connect`. The maintainer communicates in **German**;
UI source texts, logs, README, code comments and this file are **English**; the UI is also available in German (`i18n.rs`).
Not affiliated with Garmin or GSPro – use trademarks descriptively only.

## Structure

- `src/lib.rs` – library used by both binaries
  - `engine.rs` – R10 thread (BLE, reconnect, events → status) and start of the GSPro thread; `test_shot()`, `demo_session()`
  - `gspro.rs` – OpenConnect client (`Link`), heartbeats, reconnect, reply parsing (codes 200/201/5xx)
  - `mapping.rs` – R10 data → OpenConnect (m/s → mph, **spin axis inverted**, back/side spin from total + axis), `Tempo`
  - `state.rs` – shared status for the GUI (R10/GSPro status, device, battery, last shot, session shots, club/hand, log)
  - `history.rs` – shot history: `shots.jsonl` (append, not in demo mode), per-club stats, CSV export
    (OpenLaunch / Garmin Golf / Awesome Golf layouts), smash factor. Test shots (`Cmd::Shot { test: true }`) go to GSPro
    but not into the history.
  - `settings.rs` – `~/Library/Application Support/OpenLaunch Connect/settings.json` (`migrate_data_dir()` moves the old
    "OpenR10 Connect" folder on start; reads old GSProR10Connect/R10 Bridge settings as a fallback)
  - `i18n.rs` – UI translation DE/EN, gettext style: `tr("English text")`, German table in `de()`;
    `tr_detail()` translates status texts with a " (…)" suffix. New UI texts always go through `tr()` – otherwise the
    test `every_ui_text_has_a_german_translation` fails. The log stays English. Language: Settings (System/English/Deutsch).
  - `notify.rs` – macOS notifications (mac-notification-sys, NSUserNotification – works ad-hoc signed):
    `notify::alert(Alert::…, "English text")`, at most once per kind and minute, `ENABLED` from the settings (CLI: off).
    Not testable from a sandboxed shell (no LaunchServices start, "NSNotificationCenter connection invalid") – use the
    "Test" button in the settings of the real app.
  - `update.rs` – update check on start (ureq → GitHub releases API, `is_newer`), result in `state.update`, shown as a
    pill in the top bar; opt-out `check_updates`, skipped in demo mode.
  - `launcher.rs` – find/open the GSPro launcher in `~/Applications/CrossOver/…`
  - `log.rs` – timestamped log to stderr + `state`
- `src/bin/openlaunch-connect.rs` – **the app** (egui/eframe), styled after GSPro's in-game look
  (navy, tiles, condensed figures, status panel with READY, battery glyph). Layout base size 1000×470 (`BASE_SIZE`);
  with the interface size *Auto* (`ui_scale: None`) larger windows zoom the whole UI uniformly (`apply_window_zoom`,
  `fit_zoom`, aspect ratio locked); a fixed interface size (1.0–1.5) sets the zoom and leaves the window free, with a
  minimum of the base size at that zoom (`min_size`), and pages get the extra room. The overlay divides the zoom out
  (layer transform) so it keeps its size. The *Overlay* page sets the overlay's position, size, lines, opacity and
  contents (`OverlayStyle` in the settings, applied and saved at once) and shows a preview with a sample shot;
  `OverlayContent` builds what both the overlay window and the preview draw. Window size is saved in the settings (`window_size`).
  Text in DIN Condensed (`cond()`) inside boxes/buttons always via `caps_text()`: the capitals sit ~¼ of the font size
  above the middle of the text box; `caps_text` centres the capitals on `pos.y` (measured).
  Controls (buttons, toggles, segments, stepper, cards) are drawn with the painter – see `button()`, `card()`, `toggle()`.
- `src/main.rs` – CLI `openlaunch` (`t` + Enter = test shot, `--test-shot`, `--debug`)
- `vendor/tenover/` – R10 protocol library [divotmaker/10over](https://github.com/divotmaker/10over)
  (MIT/Apache, alpha) with **local patches**, documented in `vendor/tenover/PATCHES.md`: bundled protoc, battery level
  (BLE Battery Service 0x2A19), connection watchdog (macOS connect/disconnect events, `is_connected()` and battery
  probe with timeouts), re-arm after an error clears, error reason kept with the state, diagnostics hook
  (`set_diagnostics()` → "BLE: …" lines in the app log).
- `packaging/` – `Info.plist` (incl. `NSBluetoothAlwaysUsageDescription`), `icon.png` (ball: `tools/icon_ball.py`)
- `make-app.sh` – builds `target/OpenLaunch Connect.app` (sips/iconutil/ad-hoc codesign, licenses + cargo-about report
  in Resources); `--install` (quits a running copy first); `--deploy <ssh-host>` replaces and starts the app on
  another Mac (test builds without CI)
- `install.sh` – downloads the latest release via curl, quits a running copy, installs, removes the quarantine
- `about.toml`/`about.hbs` – cargo-about (third-party licenses); `LICENSE-*`, `NOTICE`
- `docs/images/` – banner (`tools/banner.py`) and screenshots for the README
- `.github/workflows/build.yml` – macOS 14 runner: tests, `make-app.sh`, artifact; tag `v*` → release

## Hard-won lessons about GSPro OpenConnect

1. **One JSON message per TCP read.** Concatenated messages → `501 Bad format`.
   So `Link::send` sends one at a time, waits for the reply (up to 1.5 s) and keeps ≥ 300 ms between messages.
2. **ShotNumber must be new.** Numbers already seen are ignored → the number starts at seconds since 2025-01-01
   and counts up (unique across restarts).
3. **Identical shots are ignored.** That's why `engine::test_shot()` varies the values every time.
4. **Focus:** under CrossOver, GSPro does not reliably process shots while its window is in the background →
   app button "TEST SHOT IN 3 S" (time to switch to GSPro).
5. A ball must be teed up (range/course, not a menu), otherwise GSPro ignores the shot.
6. Reply 201 carries `Player.Club` (e.g. `DR`, `I7`, `PW`) and `Handed` – sent when the club changes in GSPro.
   The club is chosen **only in GSPro**; the R10 doesn't know it.
7. GSPro may not answer a shot with 200 at all – a missing reply is not an error.
8. Default display is metric (km/h); display only – GSPro always gets mph.

## R10 / Bluetooth

- macOS does **not** list the R10 in its Bluetooth settings (pure BLE) – normal; the app connects itself
  (CoreBluetooth via btleplug). LightBlue finds it. The first connection may trigger a pairing dialog.
- iPhone Bluetooth must be off, otherwise the iPhone takes the R10. The R10 can only be connected to one device.
- "Device error" is usually **PlatformTilted** (R10 not level), reported with roll/pitch. On its stand the R10 reads
  ~15–19° pitch, which is fine. Right after being moved it keeps reporting tilt (and error code 0 with tilt values)
  for a while; once the reports stop, the client wakes it up and it is ready again.
- Switched off, the R10 is reported disconnected by macOS at once; switched on, it is found by the next scan.
- The R10 delivers: ball (speed, VLA, HLA, spin, axis), club (speed, face, path, AoA), swing timestamps
  (→ `mapping::tempo`: backswing = downswing start − backswing start, downswing = impact − downswing start;
  the Garmin export writes these times in ms despite its "[sec]" units row).
  No loft/lie/closure rate/impact location – can't be made up.

## Developing & testing

- In a cloud session there is **no macOS target** (rust-std for darwin not available). macOS builds run in GitHub
  Actions; locally on a Mac use `./make-app.sh --install`.
- **Live test with a real R10:** `./make-app.sh`, then `open -n --stderr <log> "target/OpenLaunch Connect.app"` and
  follow the log (the Bluetooth permission belongs to the app; a CLI without a GUI parent process may get
  "Permission denied").
- Linux build/tests work (`cargo test --all-targets`); BlueZ needs `libdbus-1-dev`.
- Test the GSPro side without GSPro: `tools/mock_gspro.py [port]` expects exactly one JSON per `recv()`, answers 501
  otherwise and 200 + 201 for shots. Use `--gspro 127.0.0.1:<port>`.
- README screenshots are rendered offscreen with egui_kittest (no window, no screen recording permission):
  `cargo test --bin openlaunch-connect readme_screenshots -- --ignored` writes `docs/images/screenshot-*.png`
  (1000×470 @2x, demo status from `demo_state()`), then `python3 tools/banner.py .` for the banner.
  `OPENLAUNCH_SCREENSHOT_LANG=de OPENLAUNCH_SCREENSHOT_DIR=<dir>` renders the German UI (check that texts fit).
  Capturing a real window with `screencapture` often fails: wgpu doesn't render occluded windows ("Skipping frame due to occlusion").
- `OPENLAUNCH_DEMO=1` simulates an R10 (READY, 82 % battery, sample session, one shot), `OPENLAUNCH_PAGE=settings|overlay|log|shots`
  opens another page; `OPENLAUNCH_SCREENSHOT[_OVERLAY]=<png>` saves the visible window and quits.

## Workflow

- **One branch and one PR per issue/feature** (`feature/<nr>-<short-name>`, "Closes #<nr>" in the PR), small PRs that
  are built and tested one by one. New requests that come up while working become their own issue/branch – don't
  add them to the running PR (exception: fixes to the feature being built). Merge with a merge commit so the single
  commits stay traceable.
- `main` is protected by the ruleset "Protect main": no force push/deletion, changes only via pull request, CI check
  `build` must be green (no required reviews; admin bypass as an emergency exit). Flow: feature branch → PR →
  `gh pr merge --merge` after green CI. Test build of a PR: artifact via `gh run download <run> -n OpenLaunch-Connect-macos-arm64`.
- Every PR adds its user-visible changes to `## [Unreleased]` in `CHANGELOG.md` (Keep a Changelog: Added / Changed /
  Fixed / Removed; written for users, not a commit list).
- Release: in a PR, bump the version in `Cargo.toml` and `packaging/Info.plist` and rename `[Unreleased]` to
  `[X.Y.Z] – date` (plus compare links); after merging, `git tag vX.Y.Z && git push origin vX.Y.Z` on the merged
  `main` commit. The workflow uses that changelog section as the GitHub release notes.
- Keep local setup notes (machines, paths, personal workflow) out of this file – they go into `CLAUDE.local.md`,
  which is git-ignored.

## Open ideas

Tracked as GitHub issues (`gh issue list`), e.g. more launch monitors (#1), overlay test on top of GSPro in full
screen (#2), webcam putting (#3), signing/notarization (#4).
