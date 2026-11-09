# ADR-015: Shot changes through one function

**Date:** 2026-11-09
**Status:** Accepted

## Context

A recorded shot is changed after the fact: *Exclude* / *Count* in the explorer, and a club
corrected when the wrong one was selected in GSPro. The explorer did the exclusion by hand: it
edited the session, its own copy of the history and `shots.jsonl`, and published nothing, so an
extension holding its own list of shots learned of the change only after a restart. A second
kind of change (the club) and changes made by extensions would have repeated that by hand in
every place.

## Decision

- **One function:** `recorder::change_shot(time, shot_number, &ShotChange) -> Result<ShotRecord, String>`
  is the only way to change a recorded shot. `ShotChange` is `Excluded(bool)` or `Club(String)`
  (a GSPro club code). Nothing else writes a shot after it was recorded, except GSPro's result
  (`recorder::gspro_result`).
- **Order (ADR-007):** the history file first (write-then-rename, `history::update`), then the
  session shot if it is in the session and the counter `state.shot_changes`, then
  `Event::ShotChanged` with the whole record as it is now. If the file can't be written or has no
  shot with that key, nothing in memory changes and nothing is published. A change that makes no
  difference writes nothing and publishes nothing. In demo mode (`history::PERSIST` off) the
  session is the history: no file is written.
- **Threads:** `ShotChanged` is published from the thread that made the change (the GUI, an
  extension's thread), which widens ADR-007's "published from the bridge threads".
- **Views:** pages own their state (ADR-010): the explorer keeps its copy of the history and loads
  it again when `state.shot_changes` moved; the session is read every frame anyway.
- **Club correction:** changes only `club_code` and `club_gspro`, never the measured values,
  GSPro's result or the misread. `club_gspro` takes GSPro's club at the first correction (`""`
  when GSPro sent none) and is cleared when the shot is corrected back to it; it is written only
  when set, so older files read as before. The clubs offered are those found in the history, as
  GSPro spells them: the core holds no list of GSPro's codes.

## Consequences

- Positive: an extension sees every change as it happens, through the same events as new shots;
  file, session and views can't disagree after a change; a new kind of change is one more
  `ShotChange` variant.
- Negative / trade-offs: `change_shot` reads and rewrites the whole history file for each shot it
  changes, on the caller's thread (the GUI for the explorer) – fine for a selection of shots, not
  for bulk edits. A shot's key `(time, shot_number)` must stay unique in the file.

## Alternatives considered

- Keep the explorer's direct edits and add an event – every caller would still repeat the three
  writes, in its own order.
- A change event the core subscribes to itself – the state would be written later than the
  caller goes on (ADR-007's reason for calling the recorder).
