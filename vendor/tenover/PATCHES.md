# Local modifications to tenover (10over)

This directory is a vendored copy of [divotmaker/10over](https://github.com/divotmaker/10over)
(crate `tenover` 0.1.3), licensed under MIT OR Apache-2.0. The following changes were made
for OpenLaunch Connect (Apache-2.0 §4(b)):

1. **Bundled protoc** – `build.rs` uses `protoc-bin-vendored` when `PROTOC` is not set, so no
   system Protocol Buffers installation is needed (`Cargo.toml`: build-dependency added).
2. **Battery level** – `src/btleplug.rs` reads the standard BLE Battery Level characteristic
   (0x2A19), subscribes to updates, and exposes it via `BtleplugTransport::battery_handle()`;
   `src/ble.rs` adds `BleTransport::battery_handle()`.
3. **Connection watchdog** – `background_loop` in `src/btleplug.rs` ends the session when macOS reports
   the R10 disconnected *or reconnected* (`CentralEvent`s; a power-cycled R10 can be reconnected so fast
   that the old session never notices), checks `is_connected()` every 3 s (with timeout) and, when
   nothing arrived meanwhile, reads the battery level as a probe (2 failed probes = lost). Without it,
   switching the R10 off went unnoticed on macOS. `DeviceError::raw_code` keeps unknown error codes.
4. **Re-arm after an error** – `client.rs`: when the R10 is in `Error` state and has not reported an
   error for 10 s (e.g. levelled again), a wake-up is sent; the R10 stays silent in `Error` otherwise.
   On session end the notification sender is dropped before `disconnect()` (with timeout), which
   could hang with the R10 gone and kept the app from reconnecting.
5. **Error reason kept** – `proto.rs`/`client.rs`: a state notification that also carries an error
   (e.g. `Error` + platform tilted) yields `StateChange` *and* `DeviceError`; before, the error was dropped.
6. **Diagnostics hook** – `set_diagnostics()` in `lib.rs` routes BLE scan and watchdog messages to the
   app's log (`eprintln!` remains the default).
7. **Only the R10** – `is_r10` in `src/btleplug.rs` also accepted any device with Garmin's manufacturer ID,
   so the app connected to a Garmin watch (an epix advertises the ID without a name). Now it matches the
   name "Approach R10" only, like the BlueZ backend; other Garmin devices are logged as skipped.
8. **Removed** – `src/bin/` (FRP server binary), `examples/`, CI config and `rust-toolchain.toml`.

Upstreaming of (1)–(7) is planned.
