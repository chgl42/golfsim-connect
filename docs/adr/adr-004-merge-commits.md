# ADR-004: Merge pull requests with a merge commit

**Date:** 2026-10-23
**Status:** Accepted

## Context

Every issue gets one branch and one pull request, and pull requests are kept small. A branch often
holds several commits that each make sense alone (for example a fix found while testing on the
real setup).

## Decision

Pull requests are merged with `gh pr merge --merge` after the `build` check is green on the head
commit. No squash, no auto-merge.

## Consequences

- Positive: the single commits stay traceable in `main`; merging stays a deliberate step.
- Negative / trade-offs: a busier history than with squash merges.

## Alternatives considered

- Squash merge – loses the single commits.
- Auto-merge – merges at any hour, without the last look.
