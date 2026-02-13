# Port Forwarding

## Architecture

`ForwardManager` (in `forward/mod.rs`) runs on the same tokio runtime as discovery. It receives `ForwardCommand`s via an `mpsc` channel and sends `ForwardEvent`s back via the `crossbeam` background channel.

Each forward binds a local `TcpListener` on `127.0.0.1`, accepts connections, and tunnels them through `russh` `channel_open_direct_tcpip`.

## Forward lifecycle

1. `Start` → bind local port → `Started` event (or `BindError`)
2. Remote port disappears from scan → `Pause` → abort listener, keep handle for reactivation
3. Remote port reappears → `Reactivate` → re-bind using remembered `local_port`
4. User stops → `Stop` → abort listener, remove handle, `Stopped` event

## No random port fallback

Bind failures immediately send `BindError`. The TUI reopens the port input modal with the error message so the user can pick a different port. Never silently fall back to port 0.

## Modal UI (`ModalState`)

`ModalState` in `app.rs` replaces the old `InputMode`:
- `None` — normal navigation
- `PortInput { remote_port, buffer, remote_host, error }` — centered modal overlay

Triggers:
- `F` / `Shift+Enter` on a non-forwarded port → opens modal with `error: None`
- `BindError` event → opens modal with `error: Some(message)`, pre-filled with the failed port
- Typing in the modal clears the error field

`Enter`/`f` on a non-forwarded port starts forwarding immediately (same local port, no modal).

## Display rows and table grouping

`ui/table.rs` exposes `DisplayRow` enum and `build_display_rows(model)`:
- Forwarded ports and inactive forwards merged together (sorted by port)
- `InactiveForward(u16)` — paused forwards whose remote port is not in the current scan, shown when `model.show_inactive_forwards` is true (toggle `p`), rendered dim with `(inactive)` label
- Separator row (dim `─`, only when both groups non-empty)
- Non-forwarded ports (original sort: port → PID → protocol)

`selected_index` is a visual index into display rows. Navigation skips separator rows. `adjust_selection()` in `app.rs` preserves the selected port across display-row reorderings (scan updates, forward start/stop).

Pressing `Enter`/`f` on an inactive forward sends `ForwardCommand::Stop`, removing the persisted forward.

## Persistence

`forward/persistence.rs` stores active forwards in `~/.sshfwd/forwards.json`, keyed by destination string. On startup, persisted forwards load as `Paused` and reactivate when the first scan finds their remote port. The `Reactivate` command carries `local_port` so it works even without a prior listener handle (fresh process).

## Desktop notifications

`notify.rs` sends fire-and-forget notifications (via `notify-rust`) when ports change between scans. Change detection lives in `notify::detect_port_changes()`, called from `app::update()` on each `ScanReceived`. Uses `model.prev_scan_ports` diff; first scan is skipped (no baseline). Controlled by `model.notifications_enabled` (CLI flag `--no-notify`). On macOS, the icon is set to Terminal.app via `set_application("com.apple.Terminal")`; on Linux, uses `utilities-terminal` freedesktop icon.

## Adding a new ForwardCommand

1. Add variant to `ForwardCommand` in `forward/mod.rs`
2. Handle in `ForwardManager::run()` match
3. Send corresponding `ForwardEvent` back
4. Handle the event in `app::update()` `Message::ForwardEvent` branch
