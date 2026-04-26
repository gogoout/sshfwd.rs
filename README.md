```
              __    ____             __
   __________/ /_  / __/      ______/ /
  / ___/ ___/ __ \/ /_| | /| / / __  /
 (__  |__  ) / / / __/| |/ |/ / /_/ /
/____/____/_/ /_/_/   |__/|__/\__,_/
```
# sshfwd.rs

A TUI-based SSH port forwarding management tool built with Rust. Inspired by [k9s](https://github.com/derailed/k9s)' keyboard-driven interface.

![Demo](assets/demo.gif)

## Features

- **Automatic port detection** — deploys a lightweight agent that streams listening ports in real time
- **One-key forwarding** — `Enter`/`f` to forward with matching local port, `F`/`Shift+Enter` for custom port
- **Reverse forwarding** — press `m` to switch to Reverse mode; pick a local service and expose it on a remote port (SSH `-R` style)
- **Smart lifecycle management** — auto-pauses when remote port disappears, reactivates when it returns (unlike VS Code's stale forwards)
- **Auto-reconnect** — transparently reconnects with exponential backoff on connection drop; all forwards restore automatically
- **Clear error recovery** — bind failures show a modal to choose a different port (no silent fallbacks)
- **Visual grouping** — forwarded ports appear at the top, separated from unforwarded ports
- **Inactive forward visibility** — toggle `p` to show persisted forwards whose remote port isn't running
- **Desktop notifications** — batched notifications when ports appear, disappear, or reactivate (disable with `--no-notify`)
- **Session persistence** — remembers active forwards per destination in `~/.sshfwd/forwards.json`
- **Pure Rust SSH** — no system OpenSSH dependency, uses `russh` for in-process connections
- **ProxyJump support** — recursive tunneling through jump hosts via SSH config

## Platform Support

**Remote servers (agent):**
- Linux x86_64 / ARM64 (aarch64) — statically linked via musl
- macOS (Apple Silicon & Intel) — native binaries

**Local machine (main app):**
- macOS (Apple Silicon & Intel)
- Linux (x86_64 / ARM64)
- Windows via WSL (experimental)

> The agent is automatically deployed when you connect. No manual configuration needed.

## Installation

```bash
cargo install sshfwd
```

The published crate includes prebuilt agent binaries for all supported platforms. The agent is automatically deployed to remote servers when you connect.

## Usage

```bash
# Connect to a remote server
sshfwd user@hostname

# Disable desktop notifications
sshfwd user@hostname --no-notify

# Development: override agent binary
sshfwd user@hostname --agent-path ./target/debug/sshfwd-agent
```

### TUI Interface

**Forward mode** (default) — shows remote listening ports:

```
╭ ● user@host │ M:Fwd │ 5 ports │ 2 fwd ────────────╮
│ FWD       PORT    PROTO   PID      COMMAND         │
│▶->:5432  5432    tcp     1234     postgresql/15/..│
│ ->:8080  8080    tcp6    5678     node server.js  │
│ ──────── ──────── ─────── ──────── ────────────────│
│          3000    tcp     9012     ruby bin/rails s│
│          6379    tcp     3456     redis-server    │
╰───────────────────────────────────────────────────╯
 <j/k>Navigate <g/G>Top/Bottom <Enter/f>Forward <F>Custom Port <m>Mode <p>Inactive <q>Quit
```

**Reverse mode** (`m` to toggle) — shows local listening ports and exposes them on the remote:

```
╭ ● user@host │ M:Rev │ 3 local ───────────────────╮
│ FWD       PORT    PROTO   PID      COMMAND        │
│▶<-:8080  3000    tcp     9012     ruby bin/rails │
│          5173    tcp     1234     vite            │
│          5432    tcp     3456     postgresql      │
╰──────────────────────────────────────────────────╯
 <j/k>Navigate <g/G>Top/Bottom <Enter>Reverse <m>Mode <p>Inactive <q>Quit
```

`<-:8080` means local port 3000 is exposed on remote port 8080. Press `Enter` on a local port to configure the remote bind port.

Forwarded ports are grouped at the top with a visual separator.

### Port Input Modal

When pressing `F`/`Shift+Enter`, or when a bind error occurs:

```
╭─ Forward port 5432 ───────────╮
│                               │
│  Address already in use       │
│  Local port: 5432█            │
│                               │
│  <Enter>Confirm  <Esc>Cancel  │
╰───────────────────────────────╯
```

### Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `j` / `Down` | Move selection down |
| `k` / `Up` | Move selection up |
| `g` | Jump to top |
| `G` | Jump to bottom |
| `m` | Toggle Forward / Reverse mode |
| `Enter` / `f` | Toggle forwarding (Forward: same local port; Reverse: opens modal) |
| `F` / `Shift+Enter` | Forward with custom local port — Forward mode only |
| `p` | Toggle inactive persisted forwards |
| `q` / `Esc` / `Ctrl+C` | Quit |

## Development

### Build from Source

**Prerequisites:**
- Rust 1.82.0 or later
- For Linux agent cross-compilation on macOS: `brew install filosottile/musl-cross/musl-cross`

**Build:**
```bash
git clone https://github.com/gogoout/sshfwd.rs.git
cd sshfwd.rs

# Cross-compile agents for all platforms
./scripts/build-agents.sh

# Build and install the main application
cargo install --path crates/sshfwd
```

For development, use `cargo build --release -p sshfwd` to build without installing.

### Verification

```bash
cargo fmt -- --check
cargo clippy --all-targets --all-features
cargo test --workspace
```

All checks run automatically in CI. Pull requests must pass before merging.

See [CLAUDE.md](./CLAUDE.md) for development rules and workspace conventions.

### Architecture

**Workspace Crates:**
1. **sshfwd-common** — Shared types (`ScanResult`, `ListeningPort`, `AgentResponse`), serialized as JSON
2. **sshfwd-agent** — Remote binary deployed via SSH. Parses `/proc/net/tcp{,6}`, maps inodes to processes, streams JSON snapshots every 2s
3. **sshfwd** — Main application: SSH session, agent deployment, TUI, port forwarding

**TUI Architecture (Elm / TEA):**
- All state flows through `app.rs` with a pure Model/Message/update/view pattern
- Event loop uses the [dua-cli pattern](https://github.com/Byron/dua-cli): dedicated OS thread for keyboard input, `crossbeam_channel::select!` multiplexing

**Port Forwarding:**
- `ForwardManager` runs on a tokio runtime alongside discovery; one manager per session cycle, torn down and rebuilt on reconnect
- **Local** (`->:N`): binds a local `TcpListener`, tunnels accepted connections via `channel_open_direct_tcpip`
- **Reverse** (`<-:N`): calls `tcpip_forward` on the SSH server; incoming connections are pushed back via `server_channel_open_forwarded_tcpip` and forwarded to `127.0.0.1:local_port`
- Forward states: `Starting` → `Active` / `Paused` (port disappeared or disconnected) / modal reopened on bind error
- Forwards persist to `~/.sshfwd/forwards.json` keyed by destination; backward-compatible (old files load as Local)
- Auto-reconnect: exponential backoff 0s → 30s cap; all listener tasks are aborted cleanly on disconnect so ports are released before the next bind

**Data Flow:**
```
┌─ Main App ──────────┐                   ┌──── Remote Server ────┐
│                     │                   │                       │
│ 1. Connect (SSH)    │───── russh ───────│ 2. Upload Agent       │
│ 3. Deploy Agent     │──── exec ch ──────│ 4. Run Agent Loop     │
│ 5. Parse JSON       │◄── stdout pipe ───│    (scan every 2s)    │
│ 6. Display TUI      │                   │                       │
│ 7. Forward Ports    │── direct-tcpip ───│ 8. Tunnel Traffic     │
│                     │                   │                       │
└─────────────────────┘                   └───────────────────────┘
```

**Key Design Decisions:**
- **Pure Rust SSH** — `russh` avoids spawning SSH master processes that fight with the TUI for terminal control
- **Agent-based discovery** — persistent remote process streams port data; no repeated `exec` calls
- **Hash-based deployment** — only uploads agent binary if SHA256 differs from what's already on the remote
- **Atomic upload** — temp file → `mv` → `chmod +x` prevents mid-upload execution
- **Stale cleanup** — verifies `/proc/{pid}/comm` before killing to avoid hitting reused PIDs
- **No random port fallback** — bind failures surface immediately via error modal so the user stays in control
- **Reconnect over swap** — on disconnect, `ForwardManager` is torn down (aborting all listener tasks) and rebuilt fresh; simpler than live session swapping and reuses the existing reactivation path

## License

Licensed under the [MIT license](LICENSE-MIT).

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you shall be licensed as above, without any
additional terms or conditions.

## Acknowledgments

- [ratatui](https://github.com/ratatui-org/ratatui) — TUI framework
- [russh](https://crates.io/crates/russh) — Pure Rust SSH
- [k9s](https://github.com/derailed/k9s) — Interface design inspiration
- [dua-cli](https://github.com/Byron/dua-cli) — Event loop pattern
