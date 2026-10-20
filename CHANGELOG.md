# Changelog

All notable changes to OpenLaunch Connect are documented here.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

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
