# ADR-016: Pages and tile kinds decorate themselves

**Date:** 2026-11-15
**Status:** Accepted

## Context

A page (`ui::Page`, [ADR-010](adr-010-pages-own-their-state.md)) has only `id`, `label` and
`ui`; a tile kind (`explorer::TileKind`, [ADR-009](adr-009-tile-kinds-as-modules.md)) only its
id, name and drawing. The navigation and the menus of tile kinds draw every entry the same, so an
extension cannot show that one of its pages or kinds is special, or not available yet, where it
is listed. ADR-010 foresaw a defaulted `fn badge(&self) -> Option<&str>` for this. The core must
stay free of any notion of *why* an entry is marked.

## Decision

- **Two default methods on both traits:** `fn badge(&self) -> Option<&'static str>` – a mark
  drawn after the label or name, one space apart, in its colour – and
  `fn greyed(&self) -> Option<&'static str>` – `Some(why)` draws the entry greyed with `why` as
  its tooltip. Both default to `None`, so existing pages and kinds compile and look as before.
  Texts come translated from the extension (`tr`, `&'static str` like `label` and `name`); the
  core adds no text.
- **Asked every frame,** not registered once: a change (e.g. an entry no longer greyed) shows in
  the next frame, without a restart.
- **Greyed is a look, not a lock:** the entry is drawn at the kit's disabled factor, the new
  public `kit::DISABLED` (0.45, also used by `kit::button`), keeps its shape and stays clickable –
  a page opens, a kind is chosen, as always. What the page or the kind's body shows then is the
  extension's. The core hides, blurs or disables nothing.
- **Where:** the top-bar navigation; the three menus of tile kinds (the "Add tile" menu, the
  tile header's kind menu while customizing, the Overlay page's tile menu), built by one helper so
  they cannot drift; and the tile's title on the Shots page. Not the tile overlays' title over
  GSPro. The accessible label of a marked entry is `"<label> <badge>"`.
- `Page::label` and `TileKind::name` keep their meaning: the plain name, used for titles, the
  drag label and the menus' selected text.

## Consequences

- Positive: an extension marks its entries in their places with two small methods; the core only
  paints them. `kit::DISABLED` lets an extension grey its own values the same way.
- Negative / trade-offs: two more methods on public traits. A badge in the core's condensed font
  falls back to the body font's glyphs (e.g. ★). A greyed tile kind can only explain itself in its
  own body: a kind cannot switch to a page yet.

## Alternatives considered

- One `Decoration` struct method – each part defaults on its own as two methods, and `badge`
  reads as ADR-010 foresaw it.
- A side table registered with the page or kind – a second place to keep in step, and it would not
  change at runtime without another API.
- Greyed entries not clickable – the click is the way to what the extension shows about it.
