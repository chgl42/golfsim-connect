# ADR-006: Settings tolerate values they don't know

**Date:** 2026-10-23
**Status:** Accepted (tiles of an unknown kind: kept since [ADR-009](adr-009-tile-kinds-as-modules.md))

## Context

`settings.json` was read in one go with serde. One value this build doesn't know – a tile kind or
overlay item of a newer or extended build, or a typo – failed the whole file: the app started with
the defaults, and the next save (any click that applies at once) wrote them over the user's
settings. Builds with more features (e.g. more launch monitors, a closed-source add-on) will write
settings this build doesn't know, and going back to an older version must not cost the user
everything.

## Decision

- **Leave out only what holds the unknown value.** Lists of the settings (explorer pages and tiles,
  tile overlays, arrangements) are read entry by entry through `serde_json::Value`; an entry that
  doesn't deserialize is dropped with a log line (`tolerant::list`). In the overlay layout an
  unknown item leaves its place empty (`tolerant::layout`). Top-level enum values (overlay
  position, language, export layout) fall back to their default (`tolerant::or_default`). These are
  three small `deserialize_with` helpers in `settings.rs`; the types and the file format stay as
  they are.
- **Keep what this build doesn't know.** Unknown top-level keys are collected in
  `Settings::extra` (`#[serde(flatten)]`) and written back unchanged.
- **Never overwrite an unreadable file silently.** If `settings.json` can't be parsed at all, it is
  copied to `settings.json.bad` (replacing an older copy) and logged before the app falls back.
- **One way to change settings in the app.** `App::change_settings` applies a change to the settings
  in use and to the Settings page's copy (whose unsaved edits stay), then saves.

## Consequences

- Positive: a newer value costs one tile or overlay item, not all settings; settings of an
  extended build survive a save by the plain build at the top level; valid files are written
  exactly as before.
- Negative / trade-offs: entries that were dropped, and unknown keys inside nested objects (e.g. a
  new field of a tile), are still lost on the next save – keeping them would need a raw copy per
  entry and merging on save. Type errors in a scalar (e.g. a string where a number belongs) still
  fail the file; that case is covered by the `.bad` copy.

## Alternatives considered

- `#[serde(other)]` fallback variants – an `Unknown` variant in every enum, handled in every
  `match`, and the value is still lost.
- Hand-written `Deserialize` impls per type – more code for the same effect.
- A settings version number with migrations – doesn't help an older build reading newer settings.
