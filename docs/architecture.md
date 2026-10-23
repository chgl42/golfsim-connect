# Architecture

How OpenLaunch Connect is built: what runs where, how a shot travels from the launch monitor to
GSPro, where data is kept, and where the code is easy or hard to change. Decisions with their
reasons are recorded as [architecture decision records](adr/README.md).

For building, testing and the workflow, see [CLAUDE.md](../CLAUDE.md) and the [README](../README.md).

## Context

GSPro runs on an Apple Silicon Mac under CrossOver. CrossOver doesn't pass Bluetooth through to
Windows programs, so GSPro can't talk to a Garmin Approach R10 itself. OpenLaunch Connect is a
native macOS app that does the Bluetooth LE part and sends each shot to GSPro's OpenConnect API
over TCP.

```mermaid
flowchart LR
  R10["Garmin Approach R10"] -- "Bluetooth LE (CoreBluetooth)" --> App["OpenLaunch Connect<br/>(native macOS)"]
  App -- "OpenConnect JSON<br/>TCP 127.0.0.1:921" --> GSPro["GSPro<br/>(Windows, in CrossOver)"]
  GSPro -. "writes" .-> DB[("GSPro.db<br/>LastShot")]
  DB -. "read-only, polled" .-> App
  App -- "GitHub releases API<br/>(update check)" --> GH["github.com"]
```

What GSPro does **not** send back over OpenConnect (its carry, total, apex, offline, distance to
the pin) is read from its own SQLite database in the CrossOver bottle
([ADR-002](adr/adr-002-gspro-results-from-its-database.md)).

## Building blocks

One crate with a library and two binaries. The GUI is part of the library, behind the cargo
feature `gui` (on by default), so another crate can build its own app binary from it
([ADR-008](adr/adr-008-ui-in-the-library.md)); the CLI builds without it.

| Part | Path | Role |
|---|---|---|
| Library | `src/lib.rs` | Everything both binaries share: device, GSPro link, data, settings, texts |
| App | `src/ui/` (module `ui`, feature `gui`) | The egui/eframe GUI: pages, overlays, shots explorer, macOS window handling |
| App binary | `src/bin/openlaunch-connect/main.rs` | `openlaunch-connect`: calls `ui::run()` |
| CLI | `src/main.rs` | `openlaunch`: the bridge without a GUI (test shots, debug output); `--no-default-features` leaves egui out |
| R10 protocol | `vendor/tenover/` | Vendored [10over](https://github.com/divotmaker/10over) with local patches ([ADR-003](adr/adr-003-vendor-tenover-with-local-patches.md)) |
| Packaging | `make-app.sh`, `packaging/` | App bundle, icon, ad-hoc signing, licenses |

### Library modules

| Module | Responsibility | Depth |
|---|---|---|
| `engine` | Starts the bridge threads; the R10 loop (scan, connect, reconnect, events → status); demo and test shots | Mixes the R10 adapter with generic start-up and demo code |
| `mapping` | R10 data → OpenConnect units (m/s → mph, spin axis inverted, back/side spin), swing tempo | Deep, pure |
| `gspro` | OpenConnect client `Link`: one JSON per read, pacing, replies 200/201/5xx, heartbeats, reconnect, holding a shot while GSPro is away | Deep; hands each shot to the recorder |
| `gspro_db` | GSPro's own results from `GSPro.db` (`LastShot`), matched to our shots by ball speed and launch angle | Deep; hands each result to the recorder |
| `recorder` | Records a shot and GSPro's result of a shot: state, session, `shots.jsonl`, notification, then the event | Small |
| `events` | Bridge events: `subscribe()` → mpsc receiver, `publish()` to every subscriber ([ADR-007](adr/adr-007-bridge-events.md)) | Small, one enum and two functions |
| `flight` | Estimated ball flight (drag/lift, 3D, roll-out) when GSPro's result is missing | Deep, pure |
| `misread` | Plausibility check: marks a shot with a reading no club gives (left out of stats, still sent to GSPro) | Small, pure |
| `shape` | Shot shape names (e.g. "Push-Draw") from start direction and spin axis | Deep, pure |
| `history` | `shots.jsonl` (append/update), per-club stats, CSV exports | File IO next to pure stats |
| `settings` | `settings.json`, data directory, migration from earlier names, overlay arrangements; tolerant of unknown values | One flat struct |
| `state` | Process-wide status read by the GUI (R10/GSPro status, last shot, session, log) | Shallow: public fields |
| `i18n` | `tr()` lookup, German table, language switch; tables added by other crates (`register`) | One `match`, plus registered tables |
| `ui` | The app (feature `gui`): see [App](#app) | Public: `run`, `kit`, re-exported `egui`/`eframe`; the rest `pub(super)` |
| `notify` | macOS notifications, rate-limited per kind | Small |
| `update` | Update check against GitHub releases | Small |
| `launcher` | Find and open the GSPro launcher in CrossOver | Small |
| `log` | Timestamped log to stderr and into `state` | Shallow |

### App

The module `ui` (`src/ui/`):

| File | What |
|---|---|
| `mod.rs` | `run()`, `App` with the *Play*, *Settings* and *Log* pages, zoom, screenshot capture |
| `kit.rs` | Public building blocks: palette, `cond`/`caps_text`, `card`, `button`, `secondary_button`, `toggle`, `segments`, `stepper`, `setting_row`, `Icon`, `confirm`, `setup_style` |
| `overlay.rs` | Overlay windows, `OverlayContent` and its drawing, the *Overlay* page, arrangements |
| `explorer.rs`, `explorer/flight3d.rs` | The *Shots* page (shots explorer) and its 3D flight tile |
| `mac.rs` | AppKit: overlay frames, window levels, ordering above GSPro, the main window's size |

- `App` holds the pages (*Play*, *Shots*, *Overlay*, *Settings*, *Log*) and all of their state.
  Pages are methods on `App`, spread over the files above.
- **Shots explorer** (`ui/explorer.rs`): pages of tiles on a grid. Tile kinds
  (table, details, averages, dispersion, 3D flight in `explorer/flight3d.rs`) are an enum in `settings`.
- **Overlays** (`ui/overlay.rs`): the status overlay and tile overlays are extra egui viewports that stay with GSPro:
  above it while it is in front, ordered right above its window otherwise, on its screen, hidden without it. On macOS their
  frames, window levels and stacking above GSPro are set through AppKit (`ui/mac.rs`), because
  egui reports a second window's size in zoomed points and doesn't apply layer transforms there
  ([ADR-005](adr/adr-005-overlay-windows-placed-through-appkit.md)).
  `OverlayContent` builds what both the overlay window and the preview on the *Overlay* page draw.
- **Zoom**: the layout base size is 1000×470. With interface size *Auto* larger windows zoom the
  whole UI; the overlay windows run with their own zoom.
- **Look**: controls are drawn with the painter (`button`, `card`, `toggle`, `confirm`, …) in GSPro's
  in-game style, in the public module `ui::kit`, so pages from another crate look the same.
  Text in DIN Condensed is centred with `caps_text`.

## Runtime

Three long-running threads besides the GUI, plus short-lived ones (the update check, opening the
GSPro launcher).

```mermaid
flowchart TB
  subgraph threads["Threads"]
    R["r10<br/>engine::r10_loop"]
    G["gspro<br/>gspro::Link::run"]
    D["gspro-db<br/>polls GSPro.db every 1 s"]
  end
  UI["GUI<br/>repaints every 250 ms"]
  S[("state<br/>(global mutex)")]
  H[("shots.jsonl")]
  REC["recorder"]
  EV{{"events<br/>(mpsc fan-out)"}}
  SUB["subscribers<br/>(extensions)"]
  R -- "Cmd::Shot (mpsc)" --> G
  R -- "R10 status, battery" --> S
  G -- "GSPro status, club" --> S
  G -- "shot sent / given up" --> REC
  D -- "matched result" --> REC
  REC -- "last shot, session, result" --> S
  REC -- "append / update" --> H
  R -- "R10Status, BatteryLow, DeviceError" --> EV
  G -- "GsproStatus" --> EV
  REC -- "ShotRecorded, GsproResult" --> EV
  EV --> SUB
  UI -- "reads" --> S
  UI -- "Cmd::Shot (test shot)" --> G
```

### A shot, end to end

```mermaid
sequenceDiagram
  participant R10
  participant E as engine (r10)
  participant L as gspro::Link
  participant GS as GSPro
  participant DB as gspro_db
  participant RC as recorder
  participant UI as GUI
  R10->>E: ball + club data (BLE notification)
  E->>E: mapping: units, spin axis, tempo
  E->>L: Cmd::Shot
  L->>GS: one JSON message (mph)
  GS-->>L: 200 (or nothing – not an error)
  L->>RC: record_shot
  RC->>RC: state, session, shots.jsonl, then publish ShotRecorded
  GS->>GS: simulates, writes LastShot
  DB->>DB: next poll finds the new row
  DB->>DB: match by ball speed + VLA
  DB->>RC: gspro_result
  RC->>RC: state, shots.jsonl, then publish GsproResult
  UI->>UI: next repaint shows GSPro's carry / total / to pin
```

The OpenConnect rules learned the hard way (one message per read, unique shot numbers, identical
shots ignored, focus under CrossOver) are listed in [CLAUDE.md](../CLAUDE.md).

### Shared state

The threads write into `state`, the snapshot the GUI draws each frame. What happens is also
published as an `events::Event` ([ADR-007](adr/adr-007-bridge-events.md)): `ShotRecorded`,
`GsproResult`, `R10Status` and `GsproStatus` (on a change), `BatteryLow`, `DeviceError`. Every
receiver from `events::subscribe()` gets every event, in the same order; an event is published
after its state is written, and publishing never blocks. Nothing in the app subscribes yet: the
GUI still compares the key of the last shot of the list a tile shows to animate a new shot, since
that list isn't always the session (all time, the preview's sample shot).

| Global | Written by | Read by |
|---|---|---|
| `state::STATE` (via `state::with`) | engine, gspro, recorder, gspro_db (reads), update, log | GUI |
| `events` subscribers | `events::subscribe` | `events::publish` (engine, `state::set_r10`/`set_gspro`, recorder) |
| `history::PERSIST` | `engine::start` (off in demo mode) | history writes |
| `settings::SAVE` | tests (off) | `Settings::save` |
| `gspro::DEBUG` | GUI, CLI | `Link` |
| `i18n` language | GUI, tests (per thread in unit tests) | `tr` |
| `i18n` registered tables | `i18n::register` (other crates) | `tr`, `tr_detail`, `untranslated` |
| `notify::ENABLED` | GUI | `notify::alert` |

## Data

| What | Where | Written by | Notes |
|---|---|---|---|
| Settings | `~/Library/Application Support/OpenLaunch Connect/settings.json` | GUI, "apply and save at once" | Write-then-rename; old folder names migrated on start |
| Shot history | `…/OpenLaunch Connect/shots.jsonl` | `recorder` (append, GSPro's result), explorer (excluded) | One JSON line per shot; test and demo shots are not kept |
| GSPro's results | `…/CrossOver/Bottles/<bottle>/drive_c/users/<user>/AppData/LocalLow/GSPro/GSPro/GSPro.db` | GSPro | Opened read-only |
| App log | stderr + the last 400 lines in `state` | `log` | English |

## Platform

- **Bluetooth**: btleplug on CoreBluetooth. The permission belongs to the app bundle
  (`NSBluetoothAlwaysUsageDescription`). The R10 doesn't appear in the macOS Bluetooth settings.
- **Windows of the app**: AppKit through `objc2` for overlay frames, window levels, ordering
  above GSPro's window and full-screen spaces (`ui/mac.rs`).
- **GSPro under CrossOver**: GSPro's processes are Wine processes named `GSPro.exe` /
  `GSPconnect.exe`; its files live in the bottle's `drive_c`.

## Quality

- `cargo test --all-targets` runs in CI on a macOS runner, then `make-app.sh` builds the bundle.
- Unit tests cover the pure modules (`mapping`, `flight`, `shape`, stats and CSV in `history`,
  matching in `gspro_db`, settings serialization). GUI tests use egui_kittest; they are lib tests
  and run beside the others (they hold `test_util::state_lock()` where they use the shared state).
- README screenshots are rendered offscreen (`readme_screenshots`), not compared to a reference.
- Bluetooth, GSPro and overlay behaviour are tested by hand with a real R10 and GSPro.

## Where change is hard today

Found in an architecture review; the planned work makes these places extensible so that features
can be added without editing the core in many places.

- **Settings**: still one flat struct, but it reads what a newer or extended build wrote: an
  unknown value drops only its entry, unknown top-level keys are written back unchanged, and an
  unreadable file is kept as `settings.json.bad` ([ADR-006](adr/adr-006-settings-tolerate-unknown-values.md)).
  The GUI changes settings through one method, `App::change_settings`. Left: unknown keys inside
  nested objects and dropped entries are still lost on the next save.
- **Events**: shots, GSPro's results and status changes are published as events
  ([ADR-007](adr/adr-007-bridge-events.md)), and recording moved from `gspro` into `recorder`.
  Left: the GUI doesn't subscribe yet (it diffs the shots a tile shows), and events published
  before a subscriber subscribed aren't replayed.
- **Tile kinds**: a new kind touches about ten places in three files.
- **Pages**: page state lives as fields of `App` (28 fields); a new page touches five places.
- **UI in the library**: the UI is the module `ui` behind the feature `gui`, its building blocks
  are public (`ui::kit`), and other crates add translations with `i18n::register`
  ([ADR-008](adr/adr-008-ui-in-the-library.md)). Left: `ui::run()` takes no extra pages or tile
  kinds yet – that comes with the two points above.
- **Launch monitors**: R10 code and generic code are mixed in `engine` and `mapping`; names like
  `R10Status` are used for generic status. Left until a second device is added (issue #1).

Planned order: settings, events, moving the UI into the library together with its building blocks
and translations (all done), then tile kinds and pages (#73/#74).
