```
              __    ____             __
   __________/ /_  / __/      ______/ /
  / ___/ ___/ __ \/ /_| | /| / / __  /
 (__  |__  ) / / / __/| |/ |/ / /_/ /
/____/____/_/ /_/_/   |__/|__/\__,_/
```
# sshfwd.rs

A TUI-based SSH port forwarding management tool built with Rust. Inspired by [k9s](https://github.com/derailed/k9s)' keyboard-driven interface.

## Features

- **Automatic Port Detection** — Deploys a lightweight agent to the remote host that streams listening ports in real time
- **One-Key Forwarding** — Press `Enter`/`f` to forward a port with matching local port, `F`/`Shift+Enter` for a custom local port
- **Error Recovery** — Bind failures open a modal dialog to pick a different port instead of silently falling back
- **Forwarded Ports Grouped at Top** — Active forwards are visually separated from unforwarded ports
- **Persistence** — Forwarded ports are remembered across sessions per destination (`~/.sshfwd/forwards.json`)
- **Pure Rust SSH** — Uses `russh` for in-process SSH with no system OpenSSH dependency
- **ProxyJump Support** — Recursive tunneling through jump hosts via SSH config

## Platform Support

**Remote servers (agent):**
- Linux x86_64 / ARM64 (aarch64) — statically linked via musl

**Local machine (main app):**
- macOS (Apple Silicon & Intel)
- Linux

> The main app automatically detects the remote platform and deploys the correct agent binary. No manual configuration needed.

## Installation

### Quick Install

Install directly via cargo:

```bash
cargo install sshfwd
```

The published crate includes prebuilt agent binaries for all supported platforms (Linux x86_64/ARM64, macOS Intel/ARM64). The agent is automatically deployed to remote servers when you connect.

### Platform Support

**Remote servers (agent):**
- Linux x86_64 / ARM64 (aarch64) — statically linked via musl
- macOS (Apple Silicon & Intel) — native binaries

**Local machine (main app):**
- macOS (Apple Silicon & Intel)
- Linux (x86_64 / ARM64)
- Windows via WSL (experimental)

### Build from Source

For development or unsupported platforms:

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

## Usage

```bash
# Connect to a remote server
sshfwd user@hostname

# Development: override agent binary
sshfwd user@hostname --agent-path ./target/debug/sshfwd-agent
```

### TUI

```
╭ ● user@host │ 5 ports │ 2 fwd ──────────────────────╮
│ FWD       PORT    PROTO   PID      COMMAND          │
│▶->:5432  5432    tcp     1234     postgresql/15/... │
│ ->:8080  8080    tcp6    5678     node server.js    │
│ ──────── ──────── ─────── ──────── ─────────────────│
│          3000    tcp     9012     ruby bin/rails s  │
│          6379    tcp     3456     redis-server      │
╰─────────────────────────────────────────────────────╯
 <j/k>Navigate <g/G>Top/Bottom <Enter/f>Forward <F>Custom Port <q>Quit
```

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
| `Enter` / `f` | Toggle forwarding (same local port) |
| `F` / `Shift+Enter` | Forward with custom local port (modal) |
| `q` / `Esc` / `Ctrl+C` | Quit |

## Architecture

### Workspace Crates

1. **sshfwd-common** — Shared types (`ScanResult`, `ListeningPort`, `AgentResponse`), serialized as JSON
2. **sshfwd-agent** — Remote binary deployed via SSH. Parses `/proc/net/tcp{,6}`, maps inodes to processes, streams JSON snapshots every 2s
3. **sshfwd** — Main application: SSH session, agent deployment, TUI, port forwarding

### TUI Architecture (Elm / TEA)

All state flows through `app.rs`:
- **Model** — single state struct (ports, forwards, connection state, modal, selection)
- **Message** — enum of all events (scan data, key press, forward events, tick)
- **update()** — pure state transitions, returns `ForwardCommand`s
- **view()** — renders table, hotkey bar, and optional modal overlay

Event loop uses the [dua-cli pattern](https://github.com/Byron/dua-cli): bare `crossterm::event::read()` on a dedicated OS thread, `crossbeam_channel::select!` multiplexing keyboard + background channels.

### Port Forwarding

- `ForwardManager` runs on a tokio runtime alongside discovery
- Each forward binds a local `TcpListener`, accepts connections, and tunnels them via `russh` `channel_open_direct_tcpip`
- Forward state: `Starting` → `Active` / `Paused` (port disappeared from scan) / reopened on bind error
- Forwards persist to `~/.sshfwd/forwards.json` keyed by destination; restored as `Paused` on next connection

### Data Flow

```
┌─ Main App ──────────┐                    ┌──── Remote Server ────┐
│                     │                    │                       │
│ 1. Connect (SSH)    │───── russh ────────│ 2. Upload Agent       │
│ 3. Deploy Agent     │──── exec ch ───────│ 4. Run Agent Loop     │
│ 5. Parse JSON       │◄── stdout pipe ────│    (scan every 2s)    │
│ 6. Display TUI      │                    │                       │
│ 7. Forward Ports    │── direct-tcpip ────│ 8. Tunnel Traffic     │
│                     │                    │                       │
└─────────────────────┘                    └───────────────────────┘
```

### Key Design Decisions

- **Pure Rust SSH** — `russh` avoids spawning SSH master processes that fight with the TUI for terminal control
- **Agent-based discovery** — persistent remote process streams port data; no repeated `exec` calls
- **Hash-based deployment** — only uploads agent binary if SHA256 differs from what's already on the remote
- **Atomic upload** — temp file → `mv` → `chmod +x` prevents mid-upload execution
- **Stale cleanup** — verifies `/proc/{pid}/comm` before killing to avoid hitting reused PIDs
- **No random port fallback** — bind failures surface immediately via error modal so the user stays in control

## Development

```bash
# Build agents + main app
./scripts/build-agents.sh
cargo build -p sshfwd

# Verify
cargo fmt -- --check
cargo clippy --all-targets --all-features
cargo test --workspace
```

All checks run automatically in CI. Pull requests must pass before merging.

See [CLAUDE.md](./CLAUDE.md) for development rules and workspace conventions.

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
