# Changelog

All notable changes to OpenLaunch Connect are documented here.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- **Overlay page:** choose the overlay's size (75–150 %), one or two lines, how opaque its background is and which
  values it shows (club, battery, tempo, ball speed, VLA, HLA, spin, spin axis, club speed, smash factor), with a
  preview. Changes apply at once. The overlay's position moved here from the settings.
- **Exclude shots:** *Exclude* next to a shot on the shots page leaves it out of the averages and CSV exports
  (e.g. a misread); it stays listed, struck through, and *Count* takes it back. Kept in the shot history.
- **Estimated carry and total distance** on the shots page, per shot and as averages per club (in metres or
  yards, chosen separately from the speed unit in Settings → Display). GSPro doesn't send its own results back,
  so they are estimated from the launch data (no wind, sea level) and marked with *; GSPro may show other numbers.

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

[Unreleased]: https://github.com/chgl42/openlaunch-connect/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/chgl42/openlaunch-connect/releases/tag/v1.0.0
