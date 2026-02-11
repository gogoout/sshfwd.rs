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
- **Smart lifecycle management** — auto-pauses when remote port disappears, reactivates when it returns (unlike VS Code's stale forwards)
- **Clear error recovery** — bind failures show a modal to choose a different port (no silent fallbacks)
- **Visual grouping** — forwarded ports appear at the top, separated from unforwarded ports
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

# Development: override agent binary
sshfwd user@hostname --agent-path ./target/debug/sshfwd-agent
```

### TUI Interface

```
╭ ● user@host │ 5 ports │ 2 fwd ────────────────────╮
│ FWD       PORT    PROTO   PID      COMMAND         │
│▶->:5432  5432    tcp     1234     postgresql/15/..│
│ ->:8080  8080    tcp6    5678     node server.js  │
│ ──────── ──────── ─────── ──────── ────────────────│
│          3000    tcp     9012     ruby bin/rails s│
│          6379    tcp     3456     redis-server    │
╰───────────────────────────────────────────────────╯
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
- `ForwardManager` runs on a tokio runtime alongside discovery
- Each forward binds a local `TcpListener`, accepts connections, and tunnels them via `russh` `channel_open_direct_tcpip`
- Forward states: `Starting` → `Active` / `Paused` (port disappeared) / modal reopened on bind error
- Forwards persist to `~/.sshfwd/forwards.json` keyed by destination

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
