# ADR-011: Where GSPro is, from its log

**Date:** 2026-10-24
**Status:** Accepted

## Context

The tile overlays are meant for the range and a course; in GSPro's menus they cover its buttons.
OpenConnect doesn't say where GSPro is, and a shot sent from the menu is simply ignored. GSPro is a
Unity program and writes `Player.log` next to its database; loading the range, loading a course
and going back to the menu each leave a line there. Unity moves the old log to `Player-prev.log`
when GSPro starts. The log also holds the player's name.

## Decision

A watcher thread (`gspro_log`) follows `Player.log` once a second from where it stopped, reading
only whole lines, and starts over when the file is replaced (other inode) or shorter. A few fixed
markers set the mode (`RangeLevelLoaded()`, `Loading course …`, `Starting Main Init`, `End Round
Button Click`, `Reset Scene Complete`, …); every other line is ignored and nothing from the log is
written to the app's log. The mode is kept in `state.gspro_mode` and published as
`Event::GsproMode`, so extensions can react to it. Unknown (`None`) means: behave as before.

## Consequences

- Positive: tile overlays leave GSPro's menus alone; the mode is there for later features without
  touching GSPro.
- Negative / trade-offs: the markers are undocumented and a GSPro update may change them – then
  the mode stays unknown and the app behaves as without it. Opening GSPro's settings from the
  range isn't logged, so it isn't seen.

## Alternatives considered

- Asking GSPro over OpenConnect – it has no such message.
- Reading GSPro's window title or screen – the title doesn't change; the screen needs screen
  recording permission and is fragile.
