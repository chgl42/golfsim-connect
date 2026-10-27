# OpenLaunch Connect – project context for Claude

Native macOS bridge: **Garmin Approach R10 (Bluetooth LE) → GSPro OpenConnect API** (TCP `127.0.0.1:921`).
Purpose: GSPro runs on an Apple Silicon Mac under **CrossOver**; CrossOver/Wine does not pass Bluetooth through to
Windows programs. This app does BLE natively on macOS and talks TCP to GSPro in the bottle.
Replaces the detour R10 → iPhone Garmin app → E6 → gspro-r10.

During private development (0.x) the project was called **OpenR10 Connect**; renamed because more launch
monitors are planned. The public repo starts at 1.0.0 with a single commit. R10-specific is essentially `engine.rs`; device names ("R10") stay in the UI where the device is meant.

Open-source project (MIT OR Apache-2.0), repo `chgl42/openlaunch-connect`. The maintainer communicates in **German**;
UI source texts, logs, README, code comments and this file are **English**; the UI is also available in German (`i18n/`).
Not affiliated with Garmin or GSPro – use trademarks descriptively only.

## Structure

Overview with diagrams (threads, a shot end to end, data, where change is hard): `docs/architecture.md`.
Decisions and their reasons: `docs/adr/` (new decisions get an ADR in the same PR, from `docs/adr/template.md`).


- `src/lib.rs` – library used by both binaries; the UI (`ui`) is behind the cargo feature `gui` (default), so the
  CLI builds without egui (`cargo build --bin openlaunch --no-default-features`)
  - `engine.rs` – R10 thread (BLE, reconnect, events → status) and start of the GSPro thread; `test_shot()`, `demo_session()`
  - `flight.rs` – estimated ball flight from the launch data (`fly()`: drag/lift from the spin factor, 3D,
    no wind; rough roll-out) → carry, total, apex, lateral, descent, path. GSPro doesn't send its results back.
  - `shape.rs` – shot shape from HLA and spin axis (`Shape::of`, names like "Push-Draw"; right-hander's names)
  - `gspro_db.rs` – GSPro's own results (carry, total, apex, offline, to pin) from its SQLite DB in the CrossOver bottle
    (`…/AppData/LocalLow/GSPro/GSPro/GSPro.db`, table `LastShot`, read-only, polled each second; matched by ball speed
    in km/h or mph and VLA) → `ShotRecord::gspro`, `history::Distances` picks them or the estimate
  - `gspro_log.rs` – where GSPro is (`GameMode`: menu, range, course) from markers in its `Player.log` next to
    `GSPro.db` (followed each second, starts over with a new log) → `state.gspro_mode`, `Event::GsproMode`; in the
    menus the tile overlays are hidden. The log holds the player's name: only the markers are read, never logged
  - `gspro.rs` – OpenConnect client (`Link`), heartbeats, reconnect, reply parsing (codes 200/201/5xx)
  - `mapping.rs` – R10 data → OpenConnect (m/s → mph, **spin axis inverted**, back/side spin from total + axis), `Tempo`
  - `state.rs` – shared status for the GUI (R10/GSPro status, device, battery, last shot, session shots, club/hand, log)
  - `events.rs` – bridge events (`Event`: shot recorded, GSPro result, R10/GSPro status, battery low, device error):
    `subscribe()` → std mpsc receiver, `publish()` fans out and never blocks (ADR-007)
  - `recorder.rs` – records a shot the GSPro link handed over and GSPro's result of a shot (state, session,
    `shots.jsonl`, notification), then publishes the event; `gspro.rs` and `gspro_db.rs` only call it
  - `history.rs` – shot history: `shots.jsonl` (append, not in demo mode), per-club stats, CSV export
    (OpenLaunch / Garmin Golf / Awesome Golf layouts), smash factor. Test shots (`Cmd::Shot { test: true }`) go to GSPro
    but not into the history.
  - `settings.rs` – `~/Library/Application Support/OpenLaunch Connect/settings.json` (reads old GSProR10Connect/R10 Bridge settings as a fallback);
    `migrate_data_dir_in` moves the old "OpenR10 Connect" folder in while the data folder has no `shots.jsonl`
    or a move is pending (`migration.pending`; entry by entry, `shots.jsonl` last, a clash keeps ours as
    `<name>.before-move`, going on keeps entries changed after the marker and joins the histories once)
  - `instance.rs` – one instance per data folder (ADR-012): `Settings::claim_data_dir()` at start (app and CLI, not
    `--test-shot`) locks `instance.lock`, then runs the migration in `settings.rs`; a second instance gets
    `AlreadyRunning` and writes nothing (`tests/instance_lock.rs` runs the CLI against a held lock)
  - `i18n/` – UI translation, gettext style: `tr("English text")`. A language is data (ADR-013): code, native name,
    table – `mod.rs` holds the API, the list `SHIPPED` and the resolution (setting code, else the macOS locale by
    prefix, else English); each language's table is one file with one `match` (`de.rs`). A new language is its table
    file plus its `mod` line and one line in `SHIPPED`, no feature code. `tr_detail()` translates status texts with a " (…)" suffix.
    New UI texts always go through `tr()` – otherwise the test `every_ui_text_has_a_translation_in_every_shipped_language`
    (scans every file under `src/ui/`) fails. Other crates add texts with `i18n::register(code, table)` (a new code
    adds a language, named by the table's `NATIVE_NAME` text) and check them with `i18n::untranslated(code, &[sources])`.
    Missing texts fall back to English. The log stays English. Language: Settings (System plus every language; stored as
    a code, old names `english`/`german` still read).
  - `notify.rs` – macOS notifications (mac-notification-sys, NSUserNotification – works ad-hoc signed):
    `notify::alert(Alert::…, "English text")`, at most once per kind and minute, `ENABLED` from the settings (CLI: off).
    Not testable from a sandboxed shell (no LaunchServices start, "NSNotificationCenter connection invalid") – use the
    "Test" button in the settings of the real app.
  - `update.rs` – update check on start (ureq → GitHub releases API, `is_newer`), result in `state.update`, shown as a
    pill in the top bar; opt-out `check_updates`, skipped in demo mode.
  - `launcher.rs` – find/open the GSPro launcher in `~/Applications/CrossOver/…`
  - `log.rs` – timestamped log to stderr + `state`; `emit` and `state::push_log` mask each line (crate-internal
    `mask`: home folder → `~`, email addresses → `<email>`, nothing else – look a log over before posting it), so
    the Log page and COPY LOG get it too; a line written straight into `State.log` is not masked. Never log a token
    or the player's name
  - `ui/` – **the app** (ADR-008), see below
- `src/bin/openlaunch-connect/main.rs` – the app's binary: only `openlaunch_connect::ui::run()`
  (another crate adds pages with `ui::run_with(pages)`)
- `src/ui/mod.rs` – **the app** (egui/eframe), styled after GSPro's in-game look
  (navy, tiles, condensed figures, status panel with READY, battery glyph). Layout base size 1000×470 (`BASE_SIZE`);
  with the interface size *Auto* (`ui_scale: None`) larger windows zoom the whole UI uniformly (`apply_window_zoom`,
  `fit_zoom`, aspect ratio locked); a fixed interface size (1.0–1.5) sets the zoom and leaves the window free, with a
  minimum of the base size at that zoom (`min_size`), and pages get the extra room. The overlay window ignores that zoom: its
  pass runs with the overlay's size as zoom (`options.zoom_factor` set around `show_viewport_immediate`), and on macOS
  its frame is set through AppKit (`ui/mac.rs`, `mac::place_window`, native points, within the visible frame) – egui takes a second
  window's size in native points but reports it in zoomed ones, and a layer transform isn't applied in it.
  Pages implement the trait `ui::Page` (`id`, `label`, `ui`) and keep their own state (ADR-010, `ui/page.rs`); what they
  share comes through `AppCtx` (`settings()`, `change_settings`, the one way settings change; `commands()`; for the core
  pages `edit`, `explorer`, `moving` and the arrangement operations). The list `page::pages` makes the navigation
  (Play, Shots, Log, Overlay, extra pages, Help, Settings); a new core page goes there. The *Help* page
  (`ui/help.rs`) follows the README's *Troubleshooting* – change both together. Tests open a page with
  `App::show_page("id")` and reach its state with `App::page_mut::<P>()`.
  Overlay windows, their content and the *Overlay* page are in `ui/overlay.rs`. The *Overlay* page sets the overlay's position, size, lines, opacity and
  contents (`OverlayStyle` in the settings, applied and saved at once) and shows a preview with a sample shot;
  `OverlayContent` builds what both the overlay window and the preview draw. The *Shots* page is the explorer
  (`src/ui/explorer.rs`): pages of tiles (`settings::Explorer`, saved at once) on a grid of
  near-square cells (`grid`, `place`), shared filter and selection (`ExplorerState`, not saved). Tile kinds are
  modules behind the public trait `explorer::TileKind` (ADR-009): `explorer/table.rs`, `details.rs`, `dispersion.rs`,
  `scatter.rs` (correlation), `flight3d.rs`, listed in `CORE_KINDS`; other crates add kinds with
  `explorer::register_tile_kind`. A tile in the settings is `{kind, size, …the kind's fields}` (`Tile::config::<T>()`,
  `change_config`); unknown kinds are kept and show a placeholder. Window size is saved in the settings (`window_size`).
  Text in DIN Condensed (`cond()`) inside boxes/buttons always via `caps_text()`: the capitals sit ~¼ of the font size
  above the middle of the text box; `caps_text` centres the capitals on `pos.y` (measured).
  Controls (buttons, toggles, segments, stepper, cards, the confirm dialog) are drawn with the painter, in the public
  module `ui::kit` (`src/ui/kit.rs`) with the palette – see `button()`, `card()`, `toggle()`, `confirm()`. Only `ui::run`,
  `ui::run_with`, `ui::Page`, `ui::AppCtx`, `ui::kit`, the tile kind interface in `ui::explorer` (`TileKind`, `TileCtx`,
  `Units`, `FlightSummary`, `register_tile_kind`) and the re-exported `ui::egui`/`ui::eframe` are public; everything else
  stays `pub(super)`/private.
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
- Lint like CI before committing: `cargo clippy --all-targets --locked -- -D warnings` (see *Code quality*).
- Test the GSPro side without GSPro: `tools/mock_gspro.py [port]` expects exactly one JSON per `recv()`, answers 501
  otherwise and 200 + 201 for shots. Use `--gspro 127.0.0.1:<port>`.
- README screenshots are rendered offscreen with egui_kittest (no window, no screen recording permission):
  `cargo test --lib readme_screenshots -- --ignored` writes `docs/images/screenshot-*.png`
  (1000×470 @2x, demo status from `demo_state()`), then `python3 tools/banner.py .` for the banner.
  `OPENLAUNCH_SCREENSHOT_LANG=<code> OPENLAUNCH_SCREENSHOT_DIR=<dir>` renders the UI in that language, e.g. `de`
  (check that texts fit).
  Capturing a real window with `screencapture` often fails: wgpu doesn't render occluded windows ("Skipping frame due to occlusion").
- The UI tests (kittest) are lib tests now and run beside the others: they must not write the real settings
  (`settings::SAVE` off; `demo_state()` does it, set it yourself in tests that don't call it), and those that set up
  or look at the shared state hold `test_util::state_lock()` (`demo_state()` returns it: `let _state = demo_state();`).
  In unit tests the language (`i18n::set`) is per thread.
- Unit tests never touch the real data folder: `Settings::load_in`/`save_in` take the home folder (use
  `test_util::TempDir`); in lib tests the shot history is only written to the thread's `history::TEST_FILE`
  (`test_util::TestHistory`), else not at all. Tests that record shots or read `state`'s session/last shot hold
  `test_util::state_lock()`. Fixtures (OpenConnect replies, a `LastShot` row, old settings) are in
  `tests/fixtures/`, read with `crate::fixture!("name")`. Known bugs are pinned as `#[ignore = "bug #n: …"]` tests
  that assert the wanted behaviour – un-ignore them with the fix.
- Coverage (local, report only): `cargo install cargo-llvm-cov`, then `cargo llvm-cov --lib --summary-only`
  (with Homebrew's Rust instead of rustup: `LLVM_COV=/opt/homebrew/opt/llvm/bin/llvm-cov
  LLVM_PROFDATA=/opt/homebrew/opt/llvm/bin/llvm-profdata` in front).
- `OPENLAUNCH_DEMO=1` simulates an R10 (READY, 82 % battery, sample session, one shot; `OPENLAUNCH_DEMO=range`:
  a 65-shot range session with a full bag of fourteen clubs, `engine::demo_range()`, to try the explorer; on the MacBook:
  `open -n --env OPENLAUNCH_DEMO=range "/Applications/OpenLaunch Connect.app"`), `OPENLAUNCH_PAGE=<page id>` (`settings`, `overlay`, `log`, `shots`, `help`)
  opens another page; `OPENLAUNCH_SCREENSHOT[_OVERLAY]=<png>` saves the visible window and quits.

## Code quality

- **Clippy:** CI runs `cargo clippy --all-targets --locked -- -D warnings` before the tests – every warning fails
  the build. `[lints]` in `Cargo.toml` turns on `clippy::pedantic`; its `"allow"` entries are a **ratchet**: the
  lints that fired when it was set up, each with its count then. Don't add entries; to pay one down, delete the
  line and fix what clippy reports (or `#[expect]` the spots where the code is right as it is).
  `vendor/tenover` is its own package and not linted. CI also lints the library and the CLI without the GUI:
  `cargo clippy --lib --bin openlaunch --no-default-features --locked -- -D warnings`.
- No bare `#[allow]`: where code is right despite a lint, use `#[expect(lint, reason = "…")]` with a real reason
  (`allow_attributes[_without_reason]` warn; an `expect` that no longer fires is a warning too).
  `clippy.toml` lets tests use `unwrap`/`expect`.
- **No formatter:** the code is formatted by hand – never run `cargo fmt`/rustfmt; match the surrounding style.
- **Definition of Done** for a PR: clippy clean; `cargo test --all-targets` green; new logic has tests; user-visible
  changes in the CHANGELOG; new UI texts through `tr()` with a text in every shipped language; for UI changes the screenshots
  rendered (`readme_screenshots`) and looked at; Bluetooth, GSPro and overlay paths tested on the MacBook with
  the real setup (checklist: `docs/testing.md`); a decision that shapes the code gets an ADR in `docs/adr/`.

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
- Release candidate: tag `vX.Y.Z-rc.N` on `main` (no version bump needed) → a GitHub pre-release with the
  `[Unreleased]` notes for testing; the app's update check doesn't offer pre-releases.
- Keep local setup notes (machines, paths, personal workflow) out of this file – they go into `CLAUDE.local.md`,
  which is git-ignored.

## Open ideas

Tracked as GitHub issues (`gh issue list`), e.g. more launch monitors (#1), overlay test on top of GSPro in full
screen (#2), webcam putting (#3), signing/notarization (#4).
