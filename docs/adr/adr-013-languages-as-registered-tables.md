# ADR-013: Languages as registered tables

**Date:** 2026-10-26
**Status:** Accepted (supersedes the translation bullet of [ADR-008](adr-008-ui-in-the-library.md))

## Context

The UI knew exactly one translation. `Language` was a closed enum (System, English, German), `tr`
checked a German flag, `i18n::register(table)` could only add German texts, and club names
switched on `i18n::german()` in code. A third language would have meant a new enum variant, a new
flag and edits in feature code in several places.

## Decision

- **A language is data:** a code (lower case, `"de"`), its name in its own language (`"Deutsch"`)
  and a table (`fn(&str) -> Option<&'static str>`, one file with one `match`, e.g.
  `src/i18n/de.rs`). The core's languages are one list, `SHIPPED`; a new language is its table
  file and one line there. English is the source language: its texts are the keys, it has no
  table.
- **Extensions register per language:** `i18n::register(code, table)` adds a table to a language.
  `tr` looks in the core's table of the active language first, then in the registered ones in
  order, then falls back to the English key. A code the core doesn't ship adds that language; its
  picker name is its table's text for the key `i18n::NATIVE_NAME` (else the code).
  `i18n::untranslated(code, sources)` checks an extension's texts per language.
- **The active language is an index** into `i18n::languages()` (the shipped ones, then the
  registered ones), an atomic in the app and per thread in unit tests. `i18n::active()` gives its
  code.
- **Resolution:** the setting's code if the app has it; else (`"system"`, or a code this build
  doesn't have) the macOS locale, matched by prefix on the codes (`de-AT` → `de`, longest code
  wins); else English.
- **The setting stores the code** (`"system"`, `"en"`, `"de"`). Older names (`"english"`,
  `"german"`) read as their codes and are saved as codes; a code this build doesn't have is kept
  in the file and resolves like System (ADR-006).
- **Every shipped language is complete:** the core's test checks each UI text, bridge status
  detail and notification text in every shipped language. Extension tables and unknown texts fall
  back to English at runtime.

## Consequences

- Positive: a new language is a table file and one line; the picker ("System" plus every
  language, each in its own name) and the locale follow without feature code. An extension can
  add texts to a language or a whole language.
- Negative / trade-offs: `register` and `untranslated` take a code now, `Language` is a code
  instead of an enum and `german()` is gone – a breaking change for extensions. A build older
  than this one reads `"de"` as unknown and shows the default (System), so a German Mac still
  sees German; and it saves its default (`"system"`) back, so a forced `"en"` on a German Mac
  becomes System after one save there. A registered language's name lives in its table under a reserved key.

## Alternatives considered

- Data files (gettext `.po`, Fluent, JSON) – a new dependency and a format, for texts that are
  compiled in anyway; one `match` per language is checked by the compiler and the test.
- Keeping the enum and adding a variant per language – every language touches the setting, the
  picker and the resolution.
- Storing language names (`"german"`) instead of codes – names don't match locales and don't
  scale to extension languages.
