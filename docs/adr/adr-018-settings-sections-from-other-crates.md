# ADR-018: Settings sections from other crates

**Date:** 2026-11-24
**Status:** Accepted

## Context

The *Settings* page was a fixed grid of the core's four cards with no room for another crate's
settings, so an extension's settings could only go on a page of their own. The page edits the
core's settings in a copy and saves them with one SAVE; "Unsaved changes" shows while the copy
differs. `settings.json` has one writer, the core, through `AppCtx::change_settings` or SAVE
([ADR-006](adr-006-settings-tolerate-unknown-values.md)); an extension keeps its data in files of its own.

## Decision

- **A public trait `ui::SettingsSection`** (`ui/section.rs`): `id`, `title` (in capitals,
  translated by the extension), `icon` (a `kit::Icon`), `ui(&mut self, ui, settings: &Settings)`,
  and with defaults `dirty` (false), `save` (`Ok(())`), `height` (140, the core's two-row card),
  `badge` and `greyed` (None, as on pages and tile kinds,
  [ADR-016](adr-016-pages-and-tile-kinds-decorate-themselves.md)). Every method is asked every frame.
- **Read-only settings, own files:** a section gets `&Settings`, not `AppCtx`, so it cannot write
  `settings.json` – the boundary holds by type. It reads and writes only its own file; the core
  never writes a section's data.
- **Registration by argument** ([ADR-010](adr-010-pages-own-their-state.md)):
  `ui::run_with_sections(pages, sections)`; `run_with(pages)` stays and passes none, so existing
  callers compile unchanged. The *Settings* page owns the sections in its fields.
- **Layout:** the four core cards as before, then a card per section in the order given, two to a
  row, a row as tall as its taller card. The cards scroll in a vertical scroll area; the footer
  (RESET SHOTS VIEW, the note, SAVE) stays fixed below it, right after the cards.
- **One SAVE:** SAVE is enabled while the core's copy or any section has unsaved changes, and
  "Unsaved changes" shows meanwhile. It writes `settings.json` only if the core's copy changed, then
  calls `save` on each section with changes, all of them even after one failed; the first failure
  shows as the existing "Saving failed: …" note (it stays until a SAVE succeeds). There is no revert:
  unsaved edits stay until SAVE, as for the core's settings.
- **Decoration:** the badge is drawn after the card's title, one space apart; a greyed card is
  drawn at `kit::DISABLED` (`Ui::multiply_opacity`) with the reason as the tooltip of its heading
  row, and its controls stay usable. The heading's accessible label is `"<title> <badge>"`.
- The core adds no UI text for this.

## Consequences

- Positive: an extension's settings sit on the one *Settings* page and save with the same SAVE,
  while `settings.json` stays the core's. The page scrolls, so it has room for any number of cards.
- Negative / trade-offs: another public trait. A card has a fixed height the section states
  (`kit::card` draws in a fixed rect). The footer now sits right after the cards; before, it followed
  the last card's content and, without an update source, covered the cards' last row.

## Alternatives considered

- Changing `run_with`'s signature – breaks every extension; a new function keeps them compiling.
- Handing a section `AppCtx` – it could then write `settings.json`, and the one-writer rule would
  hold only by convention.
- A global `register_settings_section` – a section has state and is made per app (as pages,
  ADR-010).
- A revert control per section – the core's settings have none; SAVE is the one action.
