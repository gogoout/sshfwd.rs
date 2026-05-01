# Port Forwarding

## Architecture

`ForwardManager` (in `forward/mod.rs`) runs on the same tokio runtime as discovery. It receives `ForwardCommand`s via an `mpsc` channel and sends `ForwardEvent`s back via the `crossbeam` background channel.

A `ForwardManager` is created per session cycle and torn down on disconnect; the command channel (`fwd_cmd_rx`) is borrowed across cycles so commands queued during reconnect are not lost.

## Forward kinds

Two kinds of forwarding exist, distinguished by `ForwardKind` and keyed by `ForwardKey`:

```rust
pub enum ForwardKind { Local, Reverse }
pub struct ForwardKey { pub kind: ForwardKind, pub remote_port: u16 }
```

- **Local** (`ForwardKind::Local`): binds a local `TcpListener` on `127.0.0.1` and tunnels accepted connections to the remote via `channel_open_direct_tcpip`. `remote_port` = remote app's port.
- **Reverse** (`ForwardKind::Reverse`): calls `tcpip_forward` on the SSH server so it listens on `remote_port` and pushes incoming connections back via `server_channel_open_forwarded_tcpip`. The handler connects them to `127.0.0.1:local_port`. `remote_port` = the bind port on the SSH server.

`model.forwards: HashMap<ForwardKey, ForwardEntry>` holds all active/paused forwards.

## Forward lifecycle

### Local

1. `Start { kind: Local, remote_port, local_port, remote_host }` → bind local port → `Started` (or `BindError`)
2. Remote port disappears from scan → `Pause` → abort listener, keep handle for reactivation
3. Remote port reappears → `Reactivate` → re-bind using remembered `local_port`
4. User stops → `Stop` → abort listener, remove handle, `Stopped`

### Reverse

1. `Start { kind: Reverse, remote_port, local_port, remote_host: "127.0.0.1" }` → `tcpip_forward(remote_port)` → server listens → `Started` (or `BindError`)
2. Incoming connection via `server_channel_open_forwarded_tcpip` → `handle_incoming` → connect to `127.0.0.1:local_port` → bidirectional copy
3. User stops → `Stop` → `cancel_tcpip_forward(remote_port)` → `Stopped`
4. On reconnect → `Reactivate { kind: Reverse, ... }` is issued for all reverse entries by `Message::Reconnected` handler

`reconcile_forwards` (scan-driven reactivation) skips `kind != Local` — reverse forwards don't depend on the remote scan.

## No random port fallback

Bind failures immediately send `BindError`. The TUI reopens the port input modal with the error message so the user can pick a different port. Never silently fall back to port 0.

## AppMode and mode toggle

```rust
pub enum AppMode { Forward, Reverse }
```

`m` key toggles `model.mode`. In Forward mode the table shows remote scan ports + local forwards. In Reverse mode it shows local scan ports (from `model.local_ports`) + reverse forward entries.

Local scan is performed by `discovery::local::spawn_local_scan`, which runs every 2 seconds via `sshfwd_common::scanner::create_scanner()` in a `spawn_blocking` task. It is started per session cycle alongside the remote discovery stream.

## Modal UI (`ModalState`)

`ModalState` in `app.rs`:
- `None` — normal navigation
- `PortInput { kind, remote_port, local_port, buffer, remote_host, error }` — centered modal overlay

**Forward mode triggers:**
- `Enter`/`f` on unforwarded remote port → immediate same-port start (no modal)
- `F`/`Shift+Enter` on unforwarded remote port → open Local modal (`Local port:` label)
- `BindError` event → modal with `error: Some(message)`, pre-filled port

**Reverse mode triggers:**
- `Enter`/`f` on unforwarded local port → open Reverse modal (`Remote bind port:` label), buffer pre-filled with local port as default
- `Enter`/`f` on active Reverse forward → stop it (toggles off like Forward mode)
- `Enter`/`f` on `InactiveReverseForward` → `Stop` (removes persisted entry)

## Display rows and table grouping

`ui/table.rs` exposes `DisplayRow` enum and `build_display_rows(model)`, which branches on `model.mode`:

**Forward mode:**
- Forwarded ports + inactive forwards merged (sorted by port)
- `InactiveForward(u16)` — paused Local forwards whose remote port is not in current scan, shown when `model.show_inactive_forwards` is true (toggle `p`)
- Separator row (dim `─`, only when both groups non-empty)
- Non-forwarded remote ports (sort: port → PID → protocol)

**Reverse mode:**
- `LocalPort(usize)` — index into `model.local_ports`; shows active Reverse forward status if one exists
- `InactiveReverseForward(u16)` — remote bind port of a paused Reverse forward whose local source port is not in current local scan
- FWD column glyphs: `->:NNNN` (Local, NNNN = local port), `<-:NNNN` (Reverse, NNNN = remote bind port)

Header shows `M:Fwd` (cyan) or `M:Rev` (magenta) mode chip, and mode-appropriate port count.

`selected_index` is a visual index into display rows. Navigation skips separator rows. `adjust_selection()` preserves the selected port across reorderings.

## Persistence

`forward/persistence.rs` stores active forwards in `~/.sshfwd/forwards.json`, keyed by destination string. `PersistedForward` has `#[serde(default)] kind: ForwardKind` so old files without `kind` load as `Local` (backward-compatible).

On startup, all persisted forwards load as `Paused`. Local entries reactivate when the first scan finds their remote port. Reverse entries reactivate when `Message::Reconnected` is processed (issues `Reactivate` for all `kind == Reverse` entries).

## Desktop notifications

`notify.rs` sends fire-and-forget notifications (via `notify-rust`) when ports change between scans. Change detection lives in `notify::detect_port_changes()`, called from `app::update()` on each `ScanReceived`. Uses `model.prev_scan_ports` diff; first scan is skipped (no baseline). Uses `ForwardKey::local(port)` — reverse-bind ports are never matched against the remote scan. Changes are batched via `NotifyBatch` with a 2-second debounce. The batch flushes on `Tick` after the quiet period. Controlled by `model.notifications_enabled` (CLI flag `--no-notify`).

## Adding a new ForwardCommand

1. Add variant to `ForwardCommand` in `forward/mod.rs`
2. Handle in `ForwardManager::handle_command()` match — dispatch per `ForwardKind` if direction matters
3. Send corresponding `ForwardEvent` back
4. Handle the event in `app::update()` `Message::ForwardEvent` branch
