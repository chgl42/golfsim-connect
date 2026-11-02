# ADR-014: Update source as an extension point

**Date:** 2026-11-02
**Status:** Accepted

## Context

The core asked the GitHub releases API for a newer version on every start and compared the answer
with its own version (`CARGO_PKG_VERSION` of the core). An app built on the core as a library has
its own version and may publish its releases elsewhere, so the check compared the wrong versions
and asked the wrong place. And the open core contacted a server on its own, which needed `ureq`,
rustls and Mozilla's root certificates (an extra licence) for one request.

## Decision

- **The core has no update source.** `update::UpdateSource` (`installed()`, `check()`) is a trait;
  another crate registers one with `update::register(Box<dyn UpdateSource>)` before
  `ui::run_with`. It is kept in a `OnceLock`: a second call is logged and ignored, the first source
  stays. This follows the registration pattern of `i18n::register` (ADR-013).
- **The core keeps the rest:** the opt-out `check_updates` (unchanged, ADR-006), the skip in demo
  mode, the `update` thread (`check()` runs there, never on the UI thread, so it may block), the
  state (`state.update`, `state.update_check`), the pill in the top bar and the settings row.
  `update::is_newer` is public so a source can compare versions the same way.
- **Without a source** `check_in_background` does nothing: no thread, no request. The settings card
  hides the *Updates* row and shows the core's version as *Installed*; with a source it shows
  `source.installed()`.
- The core's binaries register no source, and `ureq` leaves the core.

## Consequences

- Positive: the open core contacts nobody on its own. An app built on it can tell its users about
  its own versions from its own place, in its own format. Fewer dependencies (no `ureq`, no TLS
  stack, no CDLA-licensed root certificates).
- Negative / trade-offs: users of the core's own app get no update notice; the README points to the
  releases page instead. A breaking change for anyone who relied on the built-in check. A source
  is registered once per process and can't be replaced.

## Alternatives considered

- Keep the GitHub check as the default source in the core – the core would still call home, and an
  app on the core would have to replace it.
- Make the URL a setting – still a fixed format (GitHub's API) and still a request from the core.
- Pass the source to `ui::run_with` – changes the signature for every caller; a registration
  function matches how languages are added (ADR-013).
