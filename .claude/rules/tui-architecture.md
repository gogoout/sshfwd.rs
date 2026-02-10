# TUI Architecture

## Elm Architecture (TEA)

All TUI state flows through `crates/sshfwd/src/app.rs`:
- **Model**: single state struct (ports, connection state, selection)
- **Message**: enum of all events (scan data, keyboard, tick)
- **update()**: pure state transitions, sets `needs_render` flag
- **view()**: delegates to `ui/` module

## Event loop (dua-cli pattern)

- Keyboard: dedicated OS thread with bare `crossterm::event::read()` (no `poll()`) → `crossbeam_channel::bounded(0)`
- Background: discovery + tick (1s) → `crossbeam_channel::unbounded()`
- Main loop: `crossbeam_channel::select!` over both channels, render after each event
- Key events accept `Press` and `Repeat` (filter only `Release`) for held-key responsiveness
- Terminal backend wrapped in `BufWriter` to batch write syscalls per frame

## Key patterns

- Port rows sorted by port → PID → protocol to prevent flicker between scans
- `needs_render` flag: skip draw calls when state hasn't visually changed

## Exit gotcha

`process::exit(0)` is called after terminal restore. Do NOT try graceful cleanup via destructors:
- crossterm `read()` thread has no clean cancellation
- openssh `Session` drop blocks on SSH master process
- Remote agent is cleaned up via stale PID mechanism on next connection
