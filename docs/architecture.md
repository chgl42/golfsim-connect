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

One crate with a library and two binaries.

| Part | Path | Role |
|---|---|---|
| Library | `src/lib.rs` | Everything both binaries share: device, GSPro link, data, settings, texts |
| App | `src/bin/openlaunch-connect.rs` (+ `openlaunch-connect/`) | The egui/eframe GUI: pages, overlays, shots explorer, macOS window handling |
| CLI | `src/main.rs` | `openlaunch`: the bridge without a GUI (test shots, debug output) |
| R10 protocol | `vendor/tenover/` | Vendored [10over](https://github.com/divotmaker/10over) with local patches ([ADR-003](adr/adr-003-vendor-tenover-with-local-patches.md)) |
| Packaging | `make-app.sh`, `packaging/` | App bundle, icon, ad-hoc signing, licenses |

### Library modules

| Module | Responsibility | Depth |
|---|---|---|
| `engine` | Starts the bridge threads; the R10 loop (scan, connect, reconnect, events → status); demo and test shots | Mixes the R10 adapter with generic start-up and demo code |
| `mapping` | R10 data → OpenConnect units (m/s → mph, spin axis inverted, back/side spin), swing tempo | Deep, pure |
| `gspro` | OpenConnect client `Link`: one JSON per read, pacing, replies 200/201/5xx, heartbeats, reconnect, holding a shot while GSPro is away | Deep; also records shots (history, notifications) |
| `gspro_db` | GSPro's own results from `GSPro.db` (`LastShot`), matched to our shots by ball speed and launch angle | Deep |
| `flight` | Estimated ball flight (drag/lift, 3D, roll-out) when GSPro's result is missing | Deep, pure |
| `misread` | Plausibility check: marks a shot with a reading no club gives (left out of stats, still sent to GSPro) | Small, pure |
| `shape` | Shot shape names (e.g. "Push-Draw") from start direction and spin axis | Deep, pure |
| `history` | `shots.jsonl` (append/update), per-club stats, CSV exports | File IO next to pure stats |
| `settings` | `settings.json`, data directory, migration from earlier names, overlay arrangements | One flat struct |
| `state` | Process-wide status read by the GUI (R10/GSPro status, last shot, session, log) | Shallow: public fields |
| `i18n` | `tr()` lookup, German table, language switch | Closed table |
| `notify` | macOS notifications, rate-limited per kind | Small |
| `update` | Update check against GitHub releases | Small |
| `launcher` | Find and open the GSPro launcher in CrossOver | Small |
| `log` | Timestamped log to stderr and into `state` | Shallow |

### App

- `App` holds the pages (*Play*, *Shots*, *Overlay*, *Settings*, *Log*) and all of their state.
  Pages are methods on `App`.
- **Shots explorer** (`openlaunch-connect/explorer.rs`): pages of tiles on a grid. Tile kinds
  (table, details, averages, dispersion, 3D flight in `flight3d.rs`) are an enum in `settings`.
- **Overlays**: the status overlay and tile overlays are extra egui viewports that stay with GSPro:
  above it while it is in front, ordered right above its window otherwise, on its screen, hidden without it. On macOS their
  frames, window levels and stacking above GSPro are set through AppKit (`mac` module), because
  egui reports a second window's size in zoomed points and doesn't apply layer transforms there
  ([ADR-005](adr/adr-005-overlay-windows-placed-through-appkit.md)).
  `OverlayContent` builds what both the overlay window and the preview on the *Overlay* page draw.
- **Zoom**: the layout base size is 1000×470. With interface size *Auto* larger windows zoom the
  whole UI; the overlay windows run with their own zoom.
- **Look**: controls are drawn with the painter (`button`, `card`, `toggle`, …) in GSPro's
  in-game style. Text in DIN Condensed is centred with `caps_text`.

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
  R -- "Cmd::Shot (mpsc)" --> G
  R -- "R10 status, battery" --> S
  G -- "GSPro status, club, last shot, session" --> S
  G -- "append" --> H
  D -- "GSPro result" --> S
  D -- "update" --> H
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
  participant UI as GUI
  R10->>E: ball + club data (BLE notification)
  E->>E: mapping: units, spin axis, tempo
  E->>L: Cmd::Shot
  L->>GS: one JSON message (mph)
  GS-->>L: 200 (or nothing – not an error)
  L->>L: record_shot: state, session, shots.jsonl
  GS->>GS: simulates, writes LastShot
  DB->>DB: next poll finds the new row
  DB->>DB: match by ball speed + VLA, attach result
  UI->>UI: next repaint shows GSPro's carry / total / to pin
```

The OpenConnect rules learned the hard way (one message per read, unique shot numbers, identical
shots ignored, focus under CrossOver) are listed in [CLAUDE.md](../CLAUDE.md).

### Shared state

There is no event or observer mechanism yet: the threads write into `state`, and the GUI polls it
and compares what changed (for example the key of the last session shot).

| Global | Written by | Read by |
|---|---|---|
| `state::STATE` (via `state::with`) | engine, gspro, gspro_db, update, log | GUI |
| `history::PERSIST` | `engine::start` (off in demo mode) | history writes |
| `settings::SAVE` | tests (off) | `Settings::save` |
| `gspro::DEBUG` | GUI, CLI | `Link` |
| `i18n` language | GUI, tests | `tr` |
| `notify::ENABLED` | GUI | `notify::alert` |

## Data

| What | Where | Written by | Notes |
|---|---|---|---|
| Settings | `~/Library/Application Support/OpenLaunch Connect/settings.json` | GUI, "apply and save at once" | Write-then-rename; old folder names migrated on start |
| Shot history | `…/OpenLaunch Connect/shots.jsonl` | `gspro` (append), `gspro_db` (GSPro's result), explorer (excluded) | One JSON line per shot; test and demo shots are not kept |
| GSPro's results | `…/CrossOver/Bottles/<bottle>/drive_c/users/<user>/AppData/LocalLow/GSPro/GSPro/GSPro.db` | GSPro | Opened read-only |
| App log | stderr + the last 400 lines in `state` | `log` | English |

## Platform

- **Bluetooth**: btleplug on CoreBluetooth. The permission belongs to the app bundle
  (`NSBluetoothAlwaysUsageDescription`). The R10 doesn't appear in the macOS Bluetooth settings.
- **Windows of the app**: AppKit through `objc2` for overlay frames, window levels, ordering
  above GSPro's window and full-screen spaces (`mac` module in the app).
- **GSPro under CrossOver**: GSPro's processes are Wine processes named `GSPro.exe` /
  `GSPconnect.exe`; its files live in the bottle's `drive_c`.

## Quality

- `cargo test --all-targets` runs in CI on a macOS runner, then `make-app.sh` builds the bundle.
- Unit tests cover the pure modules (`mapping`, `flight`, `shape`, stats and CSV in `history`,
  matching in `gspro_db`, settings serialization). GUI tests use egui_kittest.
- README screenshots are rendered offscreen (`readme_screenshots`), not compared to a reference.
- Bluetooth, GSPro and overlay behaviour are tested by hand with a real R10 and GSPro.

## Where change is hard today

Found in an architecture review; the planned work makes these places extensible so that features
can be added without editing the core in many places.

- **Settings**: one closed struct. An unknown value (e.g. a tile kind from a newer build) makes the
  whole file fail to load, and the next save writes defaults over it. The GUI saves from eight
  places, each mirroring fields into the edit copy by hand.
- **Events**: "shot recorded", "GSPro result arrived" and "R10 ready/problem" exist only as changes
  inside `state`. Recording a shot happens inside the protocol code of `gspro`.
- **Tile kinds**: a new kind touches about ten places in three files.
- **Pages**: page state lives as fields of `App` (28 fields); a new page touches five places.
- **UI building blocks and translations** are private to the app binary and to one `match`, so
  code outside the crate can't use them.
- **Launch monitors**: R10 code and generic code are mixed in `engine` and `mapping`; names like
  `R10Status` are used for generic status. Left until a second device is added (issue #1).

Planned order: settings, events, moving the UI into the library together with its building blocks
and translations, then tile kinds and pages.
