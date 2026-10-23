# ADR-002: Take GSPro's results from its database

**Date:** 2026-10-23
**Status:** Accepted

## Context

GSPro answers a shot over OpenConnect with a status code at most; it never sends back its own
carry, total, apex, offline or the distance left to the pin. The app first showed an estimate of
its own (`flight`), which differs from what GSPro shows. GSPro keeps its latest shot in its SQLite
database in the CrossOver bottle (table `LastShot`, one JSON row whose ID grows with every shot).

## Decision

A watcher thread (`gspro_db`) opens `GSPro.db` read-only every second, and when a new row appears
matches it to one of the recent shots by ball speed (km/h or mph, depending on GSPro's units) and
launch angle. The result is stored with the shot (`ShotRecord::gspro`); views prefer it and fall
back to the estimate.

## Consequences

- Positive: the app shows GSPro's own numbers; nothing has to change in GSPro.
- Negative / trade-offs: depends on an undocumented table and JSON layout that a GSPro update may
  change; the match is heuristic; the result arrives up to a second (or the flight time) later.

## Alternatives considered

- Only the estimate – visibly different from GSPro.
- Reading GSPro's screen – fragile and needs screen recording permission.
