# ADR-017: Overlay kinds from other crates

**Date:** 2026-11-16
**Status:** Accepted

## Context

The core draws two kinds of overlay window over GSPro: the status overlay and the tile overlays
with one explorer tile each. An extension could add a page, a tile kind or translations, but not a
window of its own over full-screen GSPro – a panel of its own had nowhere to go but into a tile overlay,
whose size and frame belong to the explorer's tiles. And a build without the extension must not
lose the extension's overlay settings (ADR-006).

## Decision

- **A small interface, like tile kinds** ([ADR-009](adr-009-tile-kinds-as-modules.md)): the public
  trait `ui::overlay::OverlayKind` – `id` (the id of its entry in the settings), `name`
  (translated), `short_label` (optional, the tab's label when the tabs don't fit; the name by
  default), `size` (the room it draws in, in points; `None` when there is nothing to show, then
  there is no window) and `ui` (draws it). `ui::overlay::register_overlay_kind(&Kind)` before
  `ui::run_with`; an id that is already there is left out; `ui::overlay::overlay_kind(id)` finds a
  registered one. The app takes the registered kinds once at start.
- **The tile overlays' window:** an extension's overlay is a window like a tile overlay's – the
  same builder (borderless, transparent, click-through except while moved, always on top, never
  active, no taskbar entry), placed through AppKit (ADR-005), drawn at zoom 1, hidden in GSPro's
  menus and while the overlays are off, nine places or by hand (*Move on screen*). One helper
  (`Windows::show` in `ui/overlay.rs`) draws both kinds of window; the windows are keyed by one
  small enum (`OverlayWindow`: status, tile *k*, extension *k*), not parallel maps. At a place it
  shares, an extension's overlay stacks under the tile overlays there. Its title is
  `GolfSim Connect Overlay <id>`.
- **Settings:** `Settings::extension_overlays` and `Arrangement::extension_overlays`, lists of
  `{ id, pos, spot, …the kind's fields }` (`settings::ExtensionOverlay`), read with
  `tolerant::list` and following the arrangements like the core's overlays. An entry of a kind the
  build doesn't have is an ordinary entry and survives a save; a place the build doesn't know reads
  as off. The lists are not written while empty, so a plain build's file stays as it was.
- **The Overlay page:** one tab per registered kind between *STATUS* and the tiles, with on/off,
  the place and *Move on screen*, and a preview. Without a registered kind the page is as before.

## Consequences

- Positive: an extension shows a panel of its own over full-screen GSPro that sits, moves and
  stacks like the core's overlays, with one trait and one call; a build without it keeps its
  settings. The tile overlays and the extensions' overlays share their window code.
- Negative / trade-offs: `ui::overlay` is now a public module (`OverlayKind`,
  `register_overlay_kind`, `overlay_kind`); changing it can break an extension. One overlay per
  kind (the first entry of its id is shown). A kind's `ui` isn't handed its entry's fields yet:
  they are kept, but a kind keeps its own settings itself.

## Alternatives considered

- A tile kind shown in a tile overlay – its size and frame are the explorer's, and it would show
  up in the explorer's menus.
- Kinds passed into `ui::run_with` – changes its signature for every caller, for something
  registered once (as ADR-009).
- A window an extension opens itself through egui – no AppKit placement, stacking or hiding in
  GSPro's menus, and every extension would rebuild them.
