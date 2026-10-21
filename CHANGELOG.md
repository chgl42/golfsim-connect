# Changelog

All notable changes to OpenLaunch Connect are documented here.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

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

[Unreleased]: https://github.com/chgl42/openlaunch-connect/compare/v1.1.0...HEAD
[1.1.0]: https://github.com/chgl42/openlaunch-connect/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/chgl42/openlaunch-connect/releases/tag/v1.0.0
