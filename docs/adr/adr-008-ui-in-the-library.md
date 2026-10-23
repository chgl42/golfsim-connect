# ADR-008: UI in the library

**Date:** 2026-10-23
**Status:** Accepted

## Context

The app's UI lived in the binary (`src/bin/openlaunch-connect.rs`, about 4,000 lines, plus the
shots explorer and the 3D flight tile next to it). No other crate could use or extend it: a
separate crate – e.g. a closed-source add-on with more pages – would have had to copy the app to
build its own binary. Its building blocks (palette, painter-drawn controls) and its translations
(one `match` in `i18n`) were closed as well.

## Decision

- **The UI is the library module `ui`** (`src/ui/`), split along its seams: `mod.rs` (`run`,
  `App`, the Play, Settings and Log pages, zoom), `overlay.rs` (overlay windows, their content,
  the Overlay page), `explorer.rs` with `explorer/flight3d.rs` (Shots page), `mac.rs` (AppKit),
  `kit.rs` (building blocks). The binary `openlaunch-connect` only calls `ui::run()`.
- **Behind a cargo feature `gui`, on by default.** It pulls in eframe, image, rfd and the
  `objc2` crates; the binary requires it. The CLI builds without it
  (`cargo build --bin openlaunch --no-default-features`), and CI lints that configuration.
- **A small public surface.** Public are `ui::run`, `ui::kit` and the re-exported `ui::egui` /
  `ui::eframe` (so an extension uses the same versions). `ui::kit` holds the palette, `cond` and
  `caps_text`, `setup_style`, `card`, `setting_row`, `button`, `secondary_button`, `toggle`,
  `segments` / `segments_where`, `stepper`, `Icon` and `confirm` – the one confirm dialog that
  replaced three copies (the export dialog shares its frame). Everything else stays `pub(super)` or private.
- **Translations other crates add to.** `i18n::register(table)` adds a table
  (`fn(&str) -> Option<&'static str>`, e.g. a function with one `match`); `tr` looks in the core
  table first, then in the registered ones in order. `i18n::untranslated(&[sources])` lists the
  `tr("…")` texts of some source code without a German text, for a crate's own test. The core's
  test scans every file under `src/ui/`.
- **UI tests are lib tests now**, run beside the others: in unit tests the language is per
  thread, and UI tests that set up or read the shared state hold `test_util::state_lock()`.
- **Not in this step:** extension points for pages and tile kinds (#73/#74). `run()` takes no
  arguments yet; those points will add what it is given.

## Consequences

- Positive: another crate can depend on the library and build an app from `ui::run()` plus its
  own code (once #73/#74 let it add pages and tiles), drawn with the same `ui::kit` controls and
  translated through `i18n::register`. The CLI no longer compiles egui. The UI is in a few files
  of a size one can read, and the confirm dialogs are one function.
- Negative / trade-offs: `ui::kit` is now an interface – changing a control's signature can break
  an extension. With the default feature, building the CLI still builds egui (features are
  unified across the package); only `--no-default-features` leaves it out. The UI tests share a
  process with the lib's tests and must keep to the locks above.

## Alternatives considered

- A separate UI crate in a workspace – cleaner boundary, but the UI uses many crate-internal
  items (state, settings, history) that would all have to become public; a feature gate gives
  the same result for the CLI with less churn.
- Keeping the UI in the binary and copying it in the add-on – two apps to keep in step.
- The UI unconditionally in the library – simpler, but every user of the library (the CLI, a
  headless tool) would build egui and wgpu.
- Builder-style controls (`Button::new(…).primary()`) – nicer to call, but would change every
  call site; the functions are kept, documented, and the pass-through helpers removed instead.
