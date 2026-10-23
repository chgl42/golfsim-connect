# ADR-009: Tile kinds as modules

**Date:** 2026-10-23
**Status:** Accepted

## Context

The kinds of tiles of the shots explorer (table, details, dispersion, correlation, 3D flight) were
an enum in `settings`, and the tile one flat struct with every kind's switches (`averages`,
`total`, `x`, `y`, `camera`). A new kind touched about ten places in three files: the enum and its
list, the struct, the kind's name, the Overlay page's options, the header, the body, the edits
and their handling, special checks (the 3D paths were only computed with a 3D tile on the page),
and the translations. A separate crate (e.g. gapping or progress tiles) couldn't add a kind at
all, and an older build dropped a tile whose kind it didn't know (ADR-006).

## Decision

- **A small interface:** the public trait `ui::explorer::TileKind` – `id` (the id in the
  settings), `name` (translated), `options` (Overlay page), `header` (controls at the right of the
  header), `title_from` (width from which the title is shown beside them) and `body` (draws the
  tile). Only `id`, `name` and `body` must be written. A kind's switches change its copy of the
  tile; the explorer saves the settings that changed. The deletion test kept it at that: no
  per-kind default config (the kind's config type has its defaults), no per-kind edits.
- **What a kind draws from:** `TileCtx` – the shots shown, their flights (`FlightSummary`, GSPro's
  numbers where chosen), units, selection, club colours, the focused shot, whether the tile is
  only shown (tile overlays) – and what it answers through: `click`, `replay`. What a kind keeps
  for the session is `ctx.state::<T>()` (per tile, e.g. the 3D camera) or `ctx.shared::<T>()`
  (all tiles, e.g. the 3D paths cache). The 3D paths, cameras and the "is a 3D tile on the page"
  check left the explorer for the 3D kind.
- **Each core kind is a module** of `ui/explorer/`: `table.rs` (shots and averages),
  `details.rs`, `dispersion.rs`, `scatter.rs` (correlation, with `Metric`), `flight3d.rs` (with
  `Camera`). Drawing helpers several kinds use (dots, spread, club groups) stay in `explorer.rs`.
- **A registry function,** not an argument of `ui::run`: `explorer::register_tile_kind(&Kind)`
  before `ui::run`. The core's kinds come first, registered ones after them in the menus; an id
  that is already there is left out. A function keeps `ui::run`'s signature free for the pages
  (#74) and works the same as `i18n::register` (ADR-008).
- **Settings:** `Tile` is `{ kind: String, size, config }`, with `config` a JSON map flattened next
  to `kind` and `size`. The file format stays as it was – today's files load unchanged and
  earlier builds read the new ones – and each kind reads its own fields as its own type
  (`tile.config::<T>()`, defaults for what is missing; `change_config` writes only what changed,
  keeping other keys). A tile of a kind this build doesn't have is kept with all its settings and
  shows that it needs a newer or extended version of the app.

## Consequences

- Positive: a new kind is one module plus one entry in `CORE_KINDS` (or one `register_tile_kind`
  call from another crate) and its German texts; settings.rs doesn't change. Tiles of an
  extension's kinds survive a build without it. Unknown keys inside a tile now survive a save,
  paying down part of ADR-006's "left" list. The interface is the test surface: a test kind
  shows up in the add-tile menu and draws.
- Negative / trade-offs: `ui::explorer` is now a public interface (`TileKind`, `TileCtx`,
  `Units`, `FlightSummary`, `register_tile_kind`); changing it can break an extension. Kinds share
  one namespace of keys in a tile (the table's and the 3D tile's `averages` were one field before
  and stay one); an extension should use distinct key names. A kind's config is read from JSON
  each frame – a few small maps, cheap next to drawing.

## Alternatives considered

- Kinds passed into `ui::run(options)` – would collide with #74's pages and change `run`'s
  signature for every caller.
- A nested `"config": { … }` object per tile – cleaner on disk, but needs a migration and an
  earlier build would lose every tile's switches.
- One `serde_json::Value` per kind (`"configs": { "table": {…} }`) – keeps switches per kind
  after a kind change, but changes the format for no user-visible gain.
- Keeping the enum with an `Unknown(String)` variant – every `match` still lists every kind.
