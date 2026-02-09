# sshfwd.rs

A TUI-based SSH port forwarding management tool built with Rust.

## Overview

`sshfwd` is a terminal user interface application that makes SSH port forwarding management intuitive and efficient. Inspired by k9s' keyboard-driven interface and VS Code's remote development features, it provides a streamlined workflow for managing port forwards across multiple SSH connections.

## Features

- **🔍 Automatic Port Detection** - Automatically discover open ports on remote servers
- **📝 Port Annotations** - Add descriptive comments to remember what each port is for
- **🔄 One-Key Toggle** - Quickly enable/disable port forwarding for any port
- **↔️ Bidirectional Forwarding** - Support for both local → remote and remote → local forwarding
- **📜 Connection History** - UI to browse and reconnect to recently used servers
- **⌨️ Keyboard-Driven** - Efficient navigation with k9s-inspired keyboard shortcuts

## Platform Support

**Remote servers (agent):**
- ✅ **Linux x86_64** - Fully tested and working
- ✅ **Linux ARM64 (aarch64)** - Built and ready
- 🚧 macOS (planned - agent stub ready, needs `lsof` integration)
- 🚧 Windows (planned)

**Local machine (main app):**
- ✅ **macOS** (Apple Silicon & Intel) - Fully tested
- ✅ **Linux** - Supported
- 🚧 Windows (planned)

> **Note**: The main app automatically detects the remote platform and deploys the correct agent binary. No manual configuration needed!

## Technology Stack

- **UI Framework**: [ratatui](https://github.com/ratatui-org/ratatui) - Terminal user interface library
- **SSH**: [openssh](https://crates.io/crates/openssh) - Wrapper around system OpenSSH
- **Async Runtime**: [tokio](https://tokio.rs/) - Async I/O
- **Language**: Rust
- **Inspiration**: k9s keyboard layout and interface design

## Architecture

`sshfwd` uses a **three-tier Cargo workspace** with an agent-based port discovery pipeline:

### Crates

1. **sshfwd-common** - Shared types
   - `AgentResponse` - Tagged enum for agent output (Ok/Error)
   - `ScanResult` - Port snapshot: hostname, username, port list, warnings
   - `ListeningPort` - Individual port with optional process info
   - Serialized to/from JSON for agent stdout communication

2. **sshfwd-agent** - Remote binary deployed via SSH
   - **Linux**: Parses `/proc/net/tcp` and `/proc/net/tcp6`, maps inodes to processes
   - **macOS**: Placeholder (will use `lsof` in future)
   - Runs as persistent process, writes JSON snapshots to stdout every 2 seconds
   - Exits cleanly on pipe close (SSH disconnect)
   - Compiled with musl for static linking (no libc dependency on remote)

3. **sshfwd** - Main application
   - SSH session lifecycle (`openssh::Session`)
   - Multi-platform agent distribution: embeds binaries for Linux/macOS × x64/ARM64
   - Runtime platform detection: detects remote OS + architecture via `uname -sm`
   - Agent deployment: hash verification, atomic upload, stale process cleanup
   - Port discovery stream: reads agent stdout, parses JSON, emits events with retry logic
   - TUI: k9s-inspired ratatui interface with Elm Architecture (TEA), vim-style keyboard navigation

### Data Flow

```
┌─ Main App ─────┐                    ┌──── Remote Server ────┐
│                │                    │                       │
│ 1. Connect     │ → SSH Session ──────→ 2. Upload Agent      │
│    (SSH)       │                    │    (temp → mv)        │
│                │                    │                       │
│ 3. Deploy      │ ↔ Execute          │ 4. Run Agent Loop    │
│    Agent       │    (spawn child)    │    (scan 2s)         │
│                │ ← Stdout pipe ──────→ 5. Stream JSON      │
│ 6. Parse       │                    │                       │
│    JSON        │                    │ 6. PID file cleanup  │
│                │                    │    on disconnect     │
│ 7. Display     │                    │                       │
│    Updates     │                    │                       │
│                │ ← Kill command ────→ 7. Exit gracefully   │
└────────────────┘                    │                       │
                                      └───────────────────────┘
```

### Key Design Decisions

- **Multi-platform agent distribution**: Main binary embeds pre-compiled agents for all supported platforms (Linux/macOS × x64/ARM64). Agents are ~330-460KB each, statically linked for maximum compatibility.
- **Runtime platform detection**: Executes `uname -sm` on remote host, detects OS + architecture, automatically selects correct agent binary. Zero configuration required.
- **Agent as persistent process**: Keeps SSH channel open, streams updates every 2 seconds (acts as implicit heartbeat). Agent writes PID to `~/.sshfwd/agent.pid` for stale process cleanup.
- **Resilient timeout handling**: Allows up to 3 consecutive timeouts (18s total) before failing. First 2 timeouts emit warnings, only the 3rd causes failure. Resets counter on successful data reception.
- **JSON lines protocol**: Each scan result is one JSON line to stdout, easy to parse and test. Enables streaming updates and simple integration.
- **No root required**: Linux agent reads world-readable `/proc/net/tcp` and `/proc/net/tcp6`. Process name lookup via `/proc/[pid]/fd/` is optional (gracefully handles permission errors).
- **Atomic agent upload**: Streams binary to temp file, uses atomic `mv`, then `chmod +x`. Prevents mid-upload execution and race conditions.
- **Stale process cleanup**: Before spawning, reads PID file and verifies process name via `/proc/[pid]/comm` before killing. Prevents accidentally killing unrelated processes from PID reuse.
- **Cross-platform scanning**: Scanner trait with platform-specific implementations selected at compile time. Currently implements Linux via `/proc/net/tcp`, macOS stub ready for `lsof` integration.

## Installation

### Prerequisites

- **Rust toolchain** (1.82.0 or later)
- **SSH client** (OpenSSH or compatible)
- **For building Linux agents on macOS**: `musl-cross` toolchain
  ```bash
  brew install filosottile/musl-cross/musl-cross
  ```

### Build from Source

```bash
# Clone the repository
git clone https://github.com/yourusername/sshfwd.rs.git
cd sshfwd.rs

# Build all agent binaries (Linux + macOS, x64 + ARM64)
./scripts/build-agents.sh

# Build and install the main application
cargo install --path crates/sshfwd

# Or just build for development
cargo build --release -p sshfwd
# Binary will be at: target/release/sshfwd
```

The build process automatically embeds all agent binaries (from `prebuilt-agents/`) into the main executable, so you only need to distribute a single binary.

## Usage

```bash
# Connect to a remote server (auto-detects platform and deploys agent)
sshfwd user@hostname

# Development: override agent binary for testing
sshfwd user@hostname --agent-path ./custom/sshfwd-agent
```

The TUI shows a k9s-style bordered table with live port data:

```
╭ ⠹ user@host │ 5 ports ─────────────────────────────╮
│ PORT    PROTO   PID      COMMAND                     │
│▶5432    tcp     1234     /usr/lib/postgresql/15/...   │
│ 8080    tcp6    5678     /usr/bin/node server.js      │
│ 3000    tcp     9012     /usr/bin/ruby bin/rails s    │
│                                                      │
╰──────────────────────────────────────────────────────╯
 <j/k>Navigate <g/G>Top/Bottom <q>Quit
```

### Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `j` / `Down` | Move selection down |
| `k` / `Up` | Move selection up |
| `g` | Jump to top |
| `G` | Jump to bottom |
| `q` / `Esc` / `Ctrl+C` | Quit |

## Comparison with VS Code Remote

While VS Code offers automatic port forwarding when it detects ports in terminal output, `sshfwd` provides:

- Dedicated TUI interface for port management
- Persistent port annotations and comments
- Faster keyboard-driven workflow
- Lightweight alternative that doesn't require VS Code

## Development

See [CLAUDE.md](./CLAUDE.md) for detailed development commands, testing instructions, and workspace structure.

### Quick Start

```bash
# Build all agent binaries for all platforms
./scripts/build-agents.sh

# Build all crates
cargo build

# Run all tests (22 tests: platform detection + parsing + serialization)
cargo test

# Check formatting and linting
cargo fmt -- --check
cargo clippy --all-targets --all-features

# Test the agent locally (Linux only)
./target/debug/sshfwd-agent --once

# Test end-to-end with a remote server
cargo run -p sshfwd -- user@hostname
```

### Test Results

Latest test (2024-02-08) against Linux x86_64 server:
- ✅ Platform auto-detection working
- ✅ Agent deployment successful (458KB statically-linked binary)
- ✅ Port discovery working (5 ports detected)
- ✅ Process identification working (socat processes identified)
- ✅ Continuous streaming (no timeouts)
- ✅ Cleanup on disconnect

## Roadmap

**Phase 1: Core Infrastructure** ✅ Complete
- [x] SSH connection management (via `openssh` crate)
- [x] Port detection on Linux servers (`/proc/net/tcp` parsing)
- [x] Cross-platform agent binary distribution (Linux/macOS × x64/ARM64)
- [x] Runtime platform detection and automatic binary selection
- [x] Resilient timeout handling with retry logic
- [x] Atomic agent deployment with hash verification
- [x] Stale process cleanup and graceful shutdown

**Phase 2: TUI Interface** ✅ Complete
- [x] ratatui-based terminal UI (Elm Architecture / TEA)
- [x] k9s-style bordered table with rounded corners
- [x] Port list with columns: PORT, PROTO, PID, COMMAND
- [x] Vim-style keyboard navigation (j/k, g/G)
- [x] Animated spinner for connection heartbeat
- [x] Stable row ordering (sorted by port, PID, proto) to prevent flickering
- [x] Conditional rendering — only redraws when state changes
- [ ] Port forwarding toggle functionality
- [ ] Connection history persistence
- [ ] Port annotation system

**Phase 3: Advanced Features** (Planned)
- [ ] macOS agent support (stub ready, needs `lsof` integration)
- [ ] Windows agent support
- [ ] Configuration file support
- [ ] Custom keyboard shortcut mapping
- [ ] Multiple simultaneous connections
- [ ] Port forwarding templates
- [ ] Connection profiles

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

_(TODO: Add license information)_

## Acknowledgments

- [ratatui](https://github.com/ratatui-org/ratatui) for the excellent TUI framework
- [k9s](https://github.com/derailed/k9s) for interface design inspiration
- VS Code Remote Development for feature inspiration
