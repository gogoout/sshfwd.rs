# TUI Architecture

## Elm Architecture (TEA)

All TUI state flows through `crates/sshfwd/src/app.rs`:
- **Model**: single state struct (ports, forwards, connection state, modal, selection)
- **Message**: enum of all events (scan data, key press, mouse events, forward events, tick)
- **update()**: pure state transitions, sets `needs_render` flag, returns `ForwardCommand`s
- **view()**: takes `&mut Model`, renders table + hotkey bar, then modal overlay if `ModalState != None`

## Event loop (dua-cli pattern)

- Keyboard + mouse: dedicated OS thread with bare `crossterm::event::read()` (no `poll()`) → `crossbeam_channel::bounded(0)`
- Background: discovery + tick (1s) → `crossbeam_channel::unbounded()`
- Main loop: `crossbeam_channel::select!` over both channels, render after each event
- Key events accept `Press` and `Repeat` (filter only `Release`) for held-key responsiveness
- Mouse: `EnableMouseCapture`/`DisableMouseCapture` in setup/teardown/panic hook
- Terminal backend wrapped in `BufWriter` to batch write syscalls per frame

## Key patterns

- `needs_render` flag: skip draw calls when state hasn't visually changed
- `view()` takes `&mut Model` — `render()` writes `table_state` and `table_content_area` for mouse hit-testing
- Display rows and table grouping details: see [Port Forwarding](/.claude/rules/port-forwarding.md)

## Exit gotcha

`process::exit(0)` is called after terminal restore. Do NOT try graceful cleanup via destructors:
- crossterm `read()` thread has no clean cancellation
- Remote agent is cleaned up via stale PID mechanism on next connection
