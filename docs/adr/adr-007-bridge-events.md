# ADR-007: Bridge events through an mpsc fan-out

**Date:** 2026-10-23
**Status:** Accepted

## Context

"Shot recorded", "GSPro's result arrived" and "R10 ready / problem" existed only as changes inside
the global `state`. Recording a shot (session, `shots.jsonl`, notification) lived in the OpenConnect
protocol code of `gspro`, and storing GSPro's result in `gspro_db`. The GUI found new shots by
comparing session keys from frame to frame. Extensions – sounds, a voice announcement that waits
for GSPro's result, more launch monitors – need to react to these moments without editing the
core each time.

## Decision

- **A small `events` module:** one `Event` enum and two functions. `subscribe()` returns a std
  `mpsc::Receiver<Event>`; `publish(Event)` sends a clone to every subscriber under one lock and
  drops the senders whose receiver is gone. The channels are unbounded, so publishing never
  blocks; all subscribers see the same order. No new dependency.
- **What an event carries:** what happened and enough to act on it without a lock on `state` –
  `ShotRecorded` the whole `ShotRecord` and whether it is a test shot; `GsproResult` the result and
  the time and shot number of the session shot it belongs to (`None` for a shot not in the session);
  `R10Status` / `GsproStatus` the new status and its detail text, published only when one of them
  changes; `BatteryLow` the level; `DeviceError` the problem text.
- **A recorder in the core:** `recorder::record_shot` and `recorder::gspro_result` write the state,
  the session and the history and send the notification, then publish. `gspro` and `gspro_db`
  call them once; `gspro` is left with the protocol, `gspro_db` with reading and matching. They are
  called, not subscribed, so the state is written before the bridge goes on (the GUI, the next shot
  and the tests see it at once) and an event is only published once its state is there.
- **What stays in `state`:** the snapshot the GUI draws every frame (statuses, battery, last shot,
  session, log). Events say *that* something happened; `state` says how things are.

## Consequences

- Positive: a feature reacts to a shot or a result by subscribing, without a change in `gspro`,
  `gspro_db` or `engine`; the protocol code no longer knows about the history or notifications.
- Negative / trade-offs: a receiver that is kept but never read keeps every event (memory grows
  with the shots); subscribers must drain or drop it. Events published before a subscriber
  subscribed are not replayed – it starts from `state`. The GUI still compares session keys to
  animate a new shot (it compares the list each tile shows, which isn't always the session).

## Alternatives considered

- Callbacks registered with the core – run on the bridge threads, so a slow one would hold up a
  shot; channels hand the work to the subscriber's own thread.
- A broadcast channel crate (e.g. `crossbeam`, `bus`) – a dependency for what a `Vec<Sender>` does.
- The recorder as a subscriber on its own thread – the state would be written later than today,
  shots and their results could be recorded out of step, and tests would have to wait.
- Replacing `state` with events – the GUI needs a snapshot to draw each frame; out of scope.
