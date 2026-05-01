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

## Connection state

`ConnectionState` in `app.rs`:
```rust
pub enum ConnectionState { Connecting, Connected, Reconnecting, Disconnected }
```

Transitions:
- Initial connect → `Connecting` → `Connected` on first `ScanReceived`
- SSH drop → `ConnectionLost` message → `Reconnecting` (all `ForwardEntry.status` set to `Paused`)
- Backoff loop sends `Reconnecting` messages while retrying
- New session up → `Reconnected` message → `Connecting` (Reverse entries get `Reactivate` commands; Local entries reactivate via scan reconciliation)
- First scan after reconnect → `Connected`

`DiscoveryError` and `StreamEnded` do **not** set `model.running = false` — the sidecar reconnect loop manages session lifecycle.

## Sidecar reconnect loop

The sidecar thread (`main.rs`) runs `run_sidecar`, which contains an outer reconnect loop:

1. `DiscoveryStream::start` for current session
2. On success: send `Reconnected`, spawn local scan, create `ForwardManager`, drive discovery
3. On discovery end: signal `ForwardManager` shutdown (oneshot), await graceful shutdown (aborts all listener tasks), abort local scan, send `ConnectionLost`
4. Reconnect: send `Reconnecting` immediately, try `Session::connect`, sleep and double backoff (cap 30s) only on failure; reset backoff to 1s on success
5. Loop from step 1

`ForwardManager` is created per session cycle via `ForwardManager::new(session, event_tx)` and shut down via `shutdown_rx: oneshot::Receiver<()>`. The command channel receiver (`fwd_cmd_rx`) is owned by the sidecar and borrowed by each manager so queued commands survive reconnects.

## Exit gotcha

`process::exit(0)` is called after terminal restore. Do NOT try graceful cleanup via destructors:
- crossterm `read()` thread has no clean cancellation
- Remote agent is cleaned up via stale PID mechanism on next connection
