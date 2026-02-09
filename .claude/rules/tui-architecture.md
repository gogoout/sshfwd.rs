# TUI Architecture

## Elm Architecture (TEA)

All TUI state flows through `crates/sshfwd/src/app.rs`:
- **Model**: single state struct (ports, connection state, spinner, selection)
- **Message**: enum of all events (scan data, keyboard, tick)
- **update()**: pure state transitions, sets `needs_render` flag
- **view()**: delegates to `ui/` module

## Key patterns

- Port rows sorted by port → PID → protocol to prevent flicker between scans
- `needs_render` flag: skip draw calls when state hasn't visually changed
- Three event sources merged via `tokio::select!`: tick (16ms), crossterm keyboard, discovery stream

## Exit gotcha

`process::exit(0)` is called after terminal restore. Do NOT try graceful cleanup via destructors:
- crossterm `EventStream` drop blocks on pending stdin read
- openssh `Session` drop blocks on SSH master process
- Remote agent is cleaned up via stale PID mechanism on next connection
