# ADR-012: One instance per data folder

**Date:** 2026-10-26
**Status:** Accepted

## Context

Settings and the shot history live in one data folder. Two running instances – the app started
twice, a self-built app beside an installed one, an extension's app beside the core app, or the CLI
beside the app – all append to `shots.jsonl` and overwrite `settings.json`, so the history gets
lines twice or out of order and settings changes are lost. The data folder of the earlier name
(`OpenR10 Connect`) was only moved while the new folder didn't exist at all, so a new folder
holding only a `settings.json` left the history behind; and a move done by two instances at once
isn't safe.

## Decision

At start, before the settings are read, the app (`ui::run_with`) and the CLI call
`Settings::claim_data_dir()` (`instance.rs`). It takes an exclusive, non-blocking advisory lock
(std `File::try_lock`, `flock` on macOS) on `instance.lock` in the data folder and keeps the file
open until the process ends; only then does it move an earlier data folder in. The order is lock,
migrate, read settings.

- A second instance doesn't get the lock and stops without writing (it only opens the existing
  lock file): the app shows a short native message (`rfd`), the CLI prints "already running" and
  exits with 1. `--test-shot` takes no lock, because it records nothing.
- The OS releases the lock when the process ends, also on a crash, so it is never stale.
- Where there can be no lock (no `HOME`, the folder can't be made), the app logs it and runs as
  before, without one.
- Migration moves the first earlier folder that exists while the data folder has no
  `shots.jsonl`, entry by entry with `shots.jsonl` last, so a move cut short goes on at the next
  start. On a name clash the earlier entry wins and the other is kept as `<name>.before-move`.
  Lock files never move. A data folder with a `shots.jsonl` is never changed.

## Consequences

- Positive: one writer per data folder; the history always ends up in the folder the app uses;
  no new dependency, no PID files, no IPC.
- Negative / trade-offs: the second app doesn't bring the running window to the front; a build
  from before the lock running at the same time isn't kept out; the lock is advisory, so other
  programs writing into the folder aren't stopped. An empty `instance.lock` stays in the folder.

## Alternatives considered

- A PID file – goes stale after a crash and needs a liveness check that can be fooled by a reused
  PID.
- A local socket or other IPC (also to bring the first window to the front) – more code and
  platform work for little gain.
- Locking only around each write – two instances would still interleave the history and undo each
  other's settings.
