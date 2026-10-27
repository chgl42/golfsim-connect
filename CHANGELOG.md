# Changelog

All notable changes to GolfSim Connect (formerly OpenLaunch Connect) are documented here.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [1.2.0] – 2026-10-27

### Added
- **Tile overlay:** a second overlay with one tile of the shots page – dispersion, 3D flight, table, details or
  correlation – on top of GSPro, also in full screen, and click-through like the status overlay. On the Overlay page
  (tab *Tile*) choose its place, size (small, medium, large), background, the tile and its options, and whether it
  shows the whole session or only the shots with the club chosen in GSPro. Where it shares its place with the status
  overlay it sits right under it (above it at the bottom of the screen). A new shot flies in the 3D tile.
- **Several tile overlays and saved arrangements:** *+* in the head of the Overlay page adds another tile overlay
  (tabs *Status*, *Tile 1*, *Tile 2*, …; *Remove tile* takes one away); overlays at the same place stack. The name
  in the head is the arrangement in use – click it to switch, rename, save the overlays as a new arrangement (e.g.
  *Range* with dispersion and 3D flight, *Course* with only the status) or delete one. Every change is kept in the
  arrangement in use. With more than one, the Play page shows *Overlay: …* to switch quickly.
- **3D flight tile, interactive:** drag to turn the camera, scroll to zoom, double-click to go back; switching between
  *Tee*, *Side view* and *Above* glides there. A new shot flies in real time with a tracer, bounces and rolls out, and ▶
  plays the selected (or latest) shot again.
- **GSPro's own flight results:** after each shot the app reads GSPro's carry, total, apex, offline and landing angle
  from GSPro's database in the CrossOver bottle (read-only) and keeps them with the shot. The chip *GSPro results*
  on the shots page shows them in the tables, details, dispersion and 3D tiles instead of the estimate, where GSPro
  sent them; the details say *from GSPro*.
- **Misreads are caught:** a shot with a reading no real shot gives – no spin measured, a launch angle above 60°,
  a start direction or spin axis far off, a smash factor no club reaches – still goes to GSPro, but is left out of
  the stats and exports right away (*excluded* on the shots page, *Count* takes it back). The overlay shows the
  doubtful value in amber, the Play page says *Misread? (spin)*.
- **The app knows where GSPro is:** it follows GSPro's log to see whether GSPro shows its menus, the range or a
  course. In the menus the tile overlays step aside so they don't cover GSPro's buttons, and come back on the range
  or a course; the status overlay stays.
- **Distance to the pin in the overlay:** after a shot on a course, the overlay shows how far the ball is left from
  the pin, from GSPro's results (in m or yd).
- **Overlays placed by hand:** *Move on screen* on the Overlay page makes the status or the tile overlay grabbable;
  drag it in GSPro into a gap of its display and let go – it stays there (also on another screen size) and lets clicks
  through again. Choosing one of the nine places brings it back to that place.
- **Left/right in the tables:** the shots table shows where each ball landed left or right of the target line
  (*Side\**, e.g. *8 R*), the averages per club the mean side ± spread and the *Width\** of the dispersion
  (left-most to right-most landing). The details' *Side\** is now also at the landing point.
- **Help page:** *HELP* in the navigation lists the common fixes – R10 not found or lost, GSPro not connected,
  CrossOver setup, overlay not visible – and how to ask for help: copy the log and open an issue on GitHub.
- **"Copied":** the Log page's button is now *COPY LOG* and shows a short "Copied" note when it has copied the log.

### Changed
- **OpenLaunch Connect is now GolfSim Connect:** the window, the top bar (a new wordmark), notifications, the
  app (`GolfSim Connect.app`) and the release download (`GolfSim-Connect-macos-arm64.zip`) carry the new name. On
  the first start your settings and shot history move from `OpenLaunch Connect` to the folder `GolfSim Connect`
  by themselves; while OpenLaunch Connect is still running, the app says so and stops without moving anything (quit
  it first; the install script does that for you). The command-line version stays `openlaunch`.
- macOS asks **once more for Bluetooth and notifications** after the update, because the app has a new identity
  (`com.golfsimconnect.app`): allow both again.
- The overlay windows are called *GolfSim Overlay* and *GolfSim Tile Overlay* now: a window capture that picks
  them by title (e.g. in OBS) has to be chosen again once.
- The own CSV layout is called *GolfSim Connect*, its file `golfsim-connect-shots-<date>.csv`; the columns stay.
- **Overlay in tiles:** the overlay shows its values in columns of two – status and club, ball and club speed, smash
  factor and spin, VLA and HLA, distance to the pin and battery – followed by the impact graphics, with symbols
  instead of words (a golf ball with speed lines, a driver, club face and ball, a ball with an arrow around it, a
  stopwatch for the tempo, a flag). On the Overlay page a click on a tile in the preview picks what it shows (any
  value, an impact graphic, which takes the column, or empty), lets a value fill its column in large figures
  (*Double height*) or removes the column, dragging a tile swaps it with another or, dropped on the line between two
  columns, puts it into a new column there, *+* after the last column adds one and *Reset* brings back the default.
  One line shows the same tiles in a row.
- **3D flight tile** looks like a driving range now: sky with haze at the horizon, mown fairway, target greens with
  flags and distance boards every 50 m (yd), height lines every 10 m. The flights cast shadows, fade in from the tee
  and get thinner further away; each landing is marked with a ring, followed by its bounces and roll. A selected
  shot glows and shows its apex and carry. A switch *Shots / Mean* shows the single shots or one bold mean flight
  per club. *Above* looks diagonally from the top right at 45° (turned more to the side in wide tiles) zoomed onto
  the landing area over a range as wide as a real one, so it fills the tile; the tee view has height lines where the
  flights peak.
- **Overlay keeps its size:** a message (no R10, no GSPro, shot not delivered) now lies on a band over the values,
  and the last shot's values stay dimmed underneath instead of disappearing; before the first shot each value
  shows a dash. Every column is as wide as its widest value, so the overlay no longer changes width from shot to
  shot. The status words are shorter: *No GSPro* and *Not ready*.
- **One app at a time:** starting GolfSim Connect while it is already running (or the command-line
  `openlaunch` beside it) now shows "GolfSim Connect is already running." and stops, instead of a second copy
  writing the same settings and shot history. The running one carries on. The empty file `instance.lock` in the data
  folder is what marks the running copy; leave it there.
- **Shot shapes:** a shot counts as *push* or *pull* from 3° off the target line (was 2°, about 5 m at 150 m),
  and the overlay's top view bends the flight only when the shape is named with a curve (from 3° of spin axis), so
  graphic and name always agree.
- **Shots page:** *CONFIGURE* is now called *CUSTOMIZE*, and *CANCEL* next to *DONE* undoes the changes made to the
  pages and tiles since.
- **Languages:** the language choice in the settings lists *System* and every language the app has, each in its own
  language; a new language comes as its translation table and two lines in the code. The settings file now stores the language as a code
  (`"de"`, `"en"`, `"system"`); older files still load, and a code this version doesn't know is kept and follows the
  system language. Club names (*7-Iron*, *Eisen 7*) are translated like every other text.
- **Home folder and email addresses masked in the log:** in the terminal, on the Log page and in the copy, the log
  shows your home folder as `~` and email addresses as `<email>`. Nothing else is masked – look the log over before
  you post it.
- **For apps built on the library:** `Settings::claim_data_dir()` replaces `Settings::migrate_data_dir()`
  (`ui::run_with` calls it); its error `AlreadyRunning` names in `earlier` an app of an earlier name still
  running. `history::ExportFormat::OpenLaunch` is now `ExportFormat::GolfSimConnect`; settings that store it as
  `"open_launch"` still load.
- **For extensions (breaking):** extensions built against the earlier API must be changed. `i18n::register` takes a
  language code with the table (`register("de", table)`), and a table for a code the core doesn't ship adds that
  language; `i18n::untranslated` takes the code to check. The settings' `Language` is a code instead of an enum and
  no longer `Copy`: `Language::ALL` and `Language::is_german` are gone, `Language::choices()` lists the picker's
  choices, and `i18n::set` takes `&Language`. `i18n::german()` is replaced by `i18n::active()` (the code in use).

### Fixed
- The overlays covered every window: they now sit right above GSPro – on top while GSPro is in front or the top window
  on its screen (also in full screen), else right under the windows in front of GSPro, GolfSim Connect's own
  too, and hidden while GSPro isn't running. With two screens (e.g. a projector) they stay on GSPro's
  screen, wherever the main window is.
- Overlays could vanish and only come back after clicking GolfSim Connect, when its window was on GSPro's screen
  behind GSPro.
- A shot hit while GSPro was reconnecting (e.g. after a restart of GSPro) was dropped; it now waits up to 10 s for
  GSPro and is sent as soon as it is back.
- When GSPro Connect got stuck (e.g. still holding the connection of an earlier run), the app said *connected*
  while no shot arrived. After two shots without any answer from GSPro it now shows *No GSPro* with the hint to
  press *Reset GSPro Connect* in GSPro's settings; the hint goes away as soon as GSPro answers again.
- **Shot history kept when the data folder moves:** if the data folder already held only settings, the history in
  the folder of the earlier name (OpenR10 Connect) stayed behind. It now moves in as long as the data folder has no
  history; a data folder with a history is never changed. Where both folders held a file of the same name, the one
  from the earlier folder is used and the data folder's own copy is kept as `<name>.before-move` (for example
  `settings.json.before-move`); it can be deleted once everything looks right. A move that could not finish
  (`migration.pending` in the data folder) goes on at the next start, keeps settings changed since it began and
  adds the earlier shots in front of the newer ones, once.
- Settings no longer reset to the defaults when they contain a value this version doesn't know (e.g. written by a
  newer version): only that tile or overlay item is left out, everything else is kept, and settings this version
  doesn't know at all are saved back unchanged. A settings file that can't be read at all is kept as
  `settings.json.bad` next to it instead of being overwritten.
- The overlay was cut off at 75 % and 100 % when the main window was zoomed (e.g. *Auto* interface size in a larger
  window); it now keeps its size whatever the main window's zoom, and stays clear of the menu bar and the Dock.
- **GSPro replies:** the player info GSPro sends after a shot (the club in use) is no longer taken as the reply to
  the next shot, which logged that shot as "No direct reply" and could show the GSPro Connect hint for nothing.
- **Overlay page:** switching to another arrangement in the card's head no longer crashes when it has fewer tile
  overlays than the tab shown, and no longer copies the overlay being edited into the arrangement switched to.

## [1.1.0] – 2026-10-21

### Added
- **Shot explorer** on the shots page: pages of tiles on a grid – a shots table, the averages per club, the details
  of a shot (all values, including the estimated flight), the **dispersion** of the landing points (carry or total)
  from above with the mean and an ellipse around all shots of each club, and a **correlation** chart of any two
  values (e.g. face to path against spin axis) with the correlation coefficient and axes through zero (values with a
  sign are centred, so the quadrants stay in place). A **3D flight** tile shows the estimated flights with the
  roll-out, from the tee, from the side or from above. *Configure* adds tiles (the empty tile with +), picks what
  each tile shows and its size (1×1 up to the whole page), lets you drag tiles into place and removes them; the page
  shown is renamed, moved and removed (after asking) right in its tab, *+* adds a page (also with a right-click on a
  tab). It starts with three pages: *Range* (table, dispersion, flights from above), *Ball flight* (from the tee,
  the side and above) and *Data* (table and details). The layout is saved; *Reset shots view* in the settings brings
  back the default pages (after asking). A filter by club (a menu when the clubs don't fit in a row; clubs in bag
  order, each with its own colour) and *Hide excluded* apply to every tile; shots clicked in a table or chart are
  selected everywhere and can be shown alone, excluded or counted again together. Larger windows (fixed interface
  size) fit more tiles. *Export CSV* writes the shots shown and asks for the layout in a dialog.
- **Estimated carry and total distance** on the shots page, per shot and as averages per club (in metres or yards,
  chosen separately from the speed unit in Settings → Display). GSPro doesn't send its own results back, so they are
  estimated from the launch data (no wind, sea level) and marked with *; GSPro may show other numbers.
- **Shot shape** (straight, draw, fade, hook, slice, push, pull and combinations like push-draw), from the start
  direction and the spin axis: a column in the shots table, a value in the details, and in the overlay's top view
  the start arrow continues as the ball's curving flight with the shape's name. Named for right-handers.
- **Overlay page:** choose the overlay's size (75–150 %), one or two lines, how opaque its background is and which
  values it shows (club, battery, tempo, ball speed, VLA, HLA, spin, spin axis, club speed, smash factor), with a
  preview. Changes apply at once. The overlay's position moved here from the settings.
- **Impact graphics in the overlay** (Overlay page → *Impact from the side* / *from above*, each on its own, on by
  default): next to the values, the club head arriving at the angle of attack and the ball leaving at the launch
  angle (side view), and a broad arrow for the club path with a narrow one for the start direction (top view, angles
  drawn three times larger), with the numbers underneath. Small and without numbers in the one-line overlay.
- **Exclude shots:** select shots on the shots page and *Exclude* them to leave them out of the averages and CSV
  exports (e.g. misreads); they stay listed, struck through, and *Count* takes them back. Kept in the shot history.

### Changed
- **Interface size** (Settings → Display): *Auto* zooms the interface with the window as before; a fixed size of
  100, 125 or 150 % lets the window take any shape, and the pages use the extra room (larger tables, log and
  overlay preview).

### Fixed
- A shot sent just as GSPro closed the connection was counted as delivered (and not reported as lost).
- After a test shot without a connected R10, GSPro was told the launch monitor was ready.
- Garmin Golf export: the sidespin had the opposite sign of the spin axis, and zero values could be written as `-0`.
- *All time* on the shots page now includes shots hit while it is open (also in the export).
- Settings are saved safely: a crash while saving no longer loses them.
- A shot is no longer lost when the previous line of the shot history was cut off (e.g. by a crash).
- Values that round to zero are shown as *0.0* instead of *-0.0* (play page, shots page, overlay, CSV exports).

## [1.0.0] – 2026-10-20

First public release. The 0.x versions were developed privately; this release sums them up.

### Features
- **Direct Bluetooth connection** to the Garmin Approach R10 – no iPhone, no Garmin app, no E6. Only devices named
  *Approach R10* are used, so Garmin watches nearby are left alone.
- **Forwarding to GSPro** via OpenConnect: all measured ball and club data, with automatic reconnects on both sides.
  Club and handedness come from GSPro.
- **R10 status:** battery level, device warnings with their reason (e.g. *R10 is tilted* with roll and pitch),
  automatic re-arm after an error and automatic reconnect when the R10 is switched off and on again.
- **Play page:** last shot, smash factor, **swing tempo** (backswing : downswing), *Test shot in 3 s* and
  *Start GSPro* (CrossOver launcher).
- **In-game overlay** on top of GSPro, also in full screen: status, club, battery and the last shot (or what's
  wrong). Click-through; any corner or the middle of any edge, or off.
- **Shot history** with averages and spread per club (session or all time); **CSV export** in the OpenLaunch,
  Garmin Golf or Awesome Golf layout.
- **macOS notifications** while GSPro is in front: low battery, R10 errors, lost connections, undelivered shots.
- **Resizable window** (also full screen): the interface zooms evenly and keeps its proportions; the overlay keeps
  its size. The window size is remembered.
- **Update check** on start (can be turned off); *Update* in the settings checks right away.
- Metric or imperial display, English or German interface, built-in log with *Copy* for bug reports,
  command-line version `openlaunch`.

[Unreleased]: https://github.com/chgl42/openlaunch-connect/compare/v1.2.0...HEAD
[1.2.0]: https://github.com/chgl42/openlaunch-connect/compare/v1.1.0...v1.2.0
[1.1.0]: https://github.com/chgl42/openlaunch-connect/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/chgl42/openlaunch-connect/releases/tag/v1.0.0
