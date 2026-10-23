# ADR-005: Place overlay windows through AppKit

**Date:** 2026-10-23
**Status:** Accepted

## Context

The overlays are extra egui viewports that float over GSPro. eframe takes a second window's size in
native points but reports it in zoomed ones, a layer transform isn't applied in it, and egui has no
way to keep a window above a full-screen app on another space or to order it right above another
app's window.

## Decision

On macOS the overlay frames, window levels and ordering are set through AppKit (`objc2`, `mac`
module in the app), in native points within the visible frame of GSPro's screen. The overlay pass
runs with the overlay's own size as zoom.

## Consequences

- Positive: overlays sit where they are placed and stay with GSPro.
- Negative / trade-offs: platform code with `unsafe` AppKit calls; a Windows version needs its own
  implementation.

## Alternatives considered

- egui's viewport commands only – wrong sizes under zoom, no control over stacking.
