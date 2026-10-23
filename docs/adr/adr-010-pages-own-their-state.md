# ADR-010: Pages own their state

**Date:** 2026-10-23
**Status:** Accepted

## Context

All page state lived as fields of `App` (28 fields), and the pages were methods on `App`. A new
page touched five places: the `Page` enum, the parsing of `OPENLAUNCH_PAGE` in `App::new`, the
navigation array in `top_bar`, the `match self.page` dispatch and the page list of
`readme_screenshots`. Pages also reached into each other's fields: the Settings page's "Reset
shots view", the arrangement menu shared by the Play and Overlay pages (it reset the Overlay
page's tab). A separate crate (e.g. a closed-source add-on with a SOUND page) could not add a page
without editing the core.

## Decision

- **A trait `ui::Page`**: `id()` (lower case, also the value of `OPENLAUNCH_PAGE`), `label()`
  (the navigation entry, translated with `tr`) and `ui(&mut self, ui, &mut AppCtx)`. A page keeps
  its own state in its fields. The core pages are `PlayPage`, `ShotsPage`, `LogPage`,
  `OverlayPage` and `SettingsPage`.
- **`ui::AppCtx`** lends a page what pages share, for the frame: `settings()`,
  `change_settings(…)` – now the one way settings change (ADR-006), applied to the settings in
  use and the Settings page's copy, then saved – and `commands()`, the sender to the bridge.
  For the core pages it also holds the Settings page's copy (`edit`), the shots explorer's state
  (`explorer`, which the tile overlays use as well) and the overlay being moved by hand
  (`moving`, shared with the overlay windows), plus the arrangement operations
  (`change_overlays`, `use_arrangement`, `arrangement_name`, `arrangement_menu`). `App` builds
  it from its own fields; the overlay windows and the window size use it through `App::ctx()`.
- **Registration by argument, not a global**: `ui::run_with(pages)` runs the app with more
  pages; `ui::run()` is `run_with(Vec::new())`. The navigation is the list `page::pages(extra)`:
  Play, Shots, Log, Overlay, the extra pages in the order given, Settings last.
  `OPENLAUNCH_PAGE` and `readme_screenshots` go through that list.
- **Cross-page reach-ins are explicit**: the arrangement menu returns the arrangement to delete
  instead of setting the Overlay page's field; the Overlay page drops a tab that no longer exists
  itself. "Reset shots view" goes through `change_settings`, and the Shots page keeps its page
  index within the pages there are.
- **Not in this step:** a premium marker or badge. It can be added later as a trait method with a
  default (`fn badge(&self) -> Option<&str> { None }`), which doesn't break existing pages.

## Consequences

- Positive: a new page is one type and one entry in a list; another crate adds one without
  editing the core. `App` keeps the window, zoom, overlays and capture; 11 page-only fields moved
  into the pages. The navigation entries are accessible widgets (kittest finds them by label).
- Negative / trade-offs: `Page` and `AppCtx` are public interfaces – changing them can break an
  extension. `AppCtx` gives an extension only the settings, `change_settings` and the command
  sender; the explorer state and the arrangements stay internal until a page outside the core
  needs them. Tests reach a page's state through `App::page_mut::<P>()` (a downcast).

## Alternatives considered

- A global `ui::register_page(…)` like `i18n::register` – a page has state and is made per app,
  and a global list would leak test pages into the other tests' screenshots.
- Keeping the `Page` enum with an `Extra(usize)` variant – still five places for a core page.
- Moving the explorer state into the Shots page – the tile overlays use it every frame, whichever
  page is shown.
