# ADR-003: Vendor tenover with local patches

**Date:** 2026-10-23
**Status:** Accepted

## Context

[10over](https://github.com/divotmaker/10over) (crate `tenover`, MIT OR Apache-2.0) implements the
R10's protocol. In its released form it needed a system `protoc`, didn't notice an R10 switched
off on macOS, could hang on disconnect, dropped error reasons and connected to other Garmin
devices.

## Decision

The crate is vendored in `vendor/tenover/` and patched there. Every change is listed in
`vendor/tenover/PATCHES.md` (as Apache-2.0 §4(b) asks); upstreaming is planned.

## Consequences

- Positive: fixes ship without waiting for a release upstream; the build needs no system tools.
- Negative / trade-offs: upstream updates have to be merged by hand; the vendored code is outside
  the project's lint rules.

## Alternatives considered

- A fork as a git dependency – one more repository to keep in step, same merge work.
- Waiting for upstream – the bridge wasn't usable on macOS without the fixes.
