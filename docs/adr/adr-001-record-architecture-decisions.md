# ADR-001: Record architecture decisions

**Date:** 2026-10-23
**Status:** Accepted

## Context

The project is maintained by one person with an AI assistant. Many choices were made after
something failed in a real setup (GSPro under CrossOver, the R10 over Bluetooth) and are easy to
undo by someone who doesn't know why they were made. Until now the reasons were spread over
CLAUDE.md, code comments and commit messages.

## Decision

Decisions that shape the code are recorded as short ADRs in `docs/adr/`, numbered
`adr-NNN-title.md`, indexed in `docs/adr/README.md`. The record lands in the same pull request as
the change. Lessons that are facts about GSPro or the R10 (not choices) stay in CLAUDE.md.

## Consequences

- Positive: reasons are found in one place; reviews and architecture passes don't re-suggest what
  was rejected for a reason.
- Negative / trade-offs: a little writing per decision.

## Alternatives considered

- Keep everything in CLAUDE.md – it is loaded into every AI session and should stay short.
