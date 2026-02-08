# sshfwd.rs

TUI-based SSH port forwarding management tool using ratatui, inspired by k9s keyboard layout.

See [README.md](./README.md) for full architecture and design details.

## Dependencies

**Workspace dependency management:**
- All dependency versions are defined in the root `Cargo.toml` under `[workspace.dependencies]`
- Features are **not** specified in the root - they are declared explicitly in each crate's `Cargo.toml`
- Sub-crates reference dependencies with `{ workspace = true, features = [...] }`

**Example:**
```toml
# Root Cargo.toml
[workspace.dependencies]
serde = "1"           # Version only, no features

# crates/sshfwd-common/Cargo.toml
[dependencies]
serde = { workspace = true, features = ["derive"] }  # Feature specified here
```

This pattern:
- Centralizes version management (update once, affects all crates)
- Makes feature usage explicit and visible in each crate
- Allows different crates to use different features of the same dependency

## Testing

**Agent (local):**
```bash
cargo build -p sshfwd-agent
./target/debug/sshfwd-agent --once        # Single scan, exits immediately
./target/debug/sshfwd-agent --version     # Print version
```

**Build all agent binaries (multi-platform):**
```bash
./scripts/build-agents.sh                 # Cross-compile for all platforms (Linux/macOS × x64/ARM)

# Or manually for specific platform:
rustup target add x86_64-unknown-linux-musl
cargo build -p sshfwd-agent --target x86_64-unknown-linux-musl --profile release-agent
# Output: target/x86_64-unknown-linux-musl/release-agent/sshfwd-agent (~2-4 MB)
```

**Main app (requires SSH server):**
```bash
cargo build -p sshfwd                                   # Embeds prebuilt agents from prebuilt-agents/
cargo run -p sshfwd -- user@hostname                    # Auto-detects platform, deploys correct binary
cargo run -p sshfwd -- user@hostname --agent-path PATH  # Override for development/testing
```

## Architecture Notes

**Multi-platform agent distribution:**
- Main binary embeds 4 pre-compiled agent binaries (~1.6MB total):
  - `linux-x86_64`: 458KB (statically-linked ELF, musl libc)
  - `linux-aarch64`: 450KB (statically-linked ELF, musl libc)
  - `darwin-x86_64`: 335KB (Mach-O, dynamically linked)
  - `darwin-aarch64`: 329KB (Mach-O, dynamically linked)
- Build script (`build.rs`) generates embedded module at compile time
- Runtime: `uname -sm` → platform detection → binary selection → deployment

**Agent lifecycle:**
- Deployed to: `~/.sshfwd/{arch}/sshfwd-agent` (e.g., `~/.sshfwd/x86_64/sshfwd-agent`)
- PID file: `~/.sshfwd/agent.pid` (shared across architectures)
- Hash-based deployment: Only uploads if SHA256 differs from remote
- Atomic upload: temp file → `mv` → `chmod +x`
- Cleanup: Verifies process name via `/proc/{pid}/comm` before killing stale processes

**Timeout resilience:**
- Heartbeat interval: 2 seconds (agent stdout)
- Timeout threshold: 6 seconds per attempt
- Max consecutive timeouts: 3 (18 seconds total)
- Behavior: Warn on 1st & 2nd timeout, fail on 3rd
- Counter reset: Any successful data reception

**Platform support:**
- ✅ Remote: Linux x86_64/ARM64 (tested)
- ✅ Local: macOS (tested), Linux (supported)
- 🚧 Remote: macOS (stub ready, needs `lsof`)
- 🚧 Windows (planned)

## Verification

**Code quality checks:**
```bash
cargo fmt -- --check                      # Verify formatting
cargo clippy --all-targets --all-features # Check for issues
cargo test --workspace                    # Run all tests (22 total)
cargo build                               # Verify compilation
```

**Test suite breakdown:**
- `sshfwd`: 3 tests (platform detection, OS/arch normalization, target triple)
- `sshfwd-agent`: 12 tests (TCP parsing, address normalization, deduplication)
- `sshfwd-common`: 7 tests (JSON serialization, protocol round-trips)

**Agent binary verification:**
```bash
./scripts/build-agents.sh                 # Build all platforms
ls -lh prebuilt-agents/*/sshfwd-agent     # Verify binaries exist
file prebuilt-agents/*/sshfwd-agent       # Check architecture

# Expected output:
# linux-x86_64:    ELF 64-bit LSB pie executable, x86-64, static-pie linked
# linux-aarch64:   ELF 64-bit LSB executable, ARM aarch64, statically linked
# darwin-x86_64:   Mach-O 64-bit executable x86_64
# darwin-aarch64:  Mach-O 64-bit executable arm64
```

**End-to-end testing:**
```bash
# Test with live server (replace with your hostname)
cargo run -p sshfwd -- user@hostname

# Expected behavior:
# 1. Connects via SSH
# 2. Detects platform (e.g., "Linux x86_64")
# 3. Deploys correct agent binary
# 4. Streams port snapshots every 2 seconds
# 5. Ctrl+C: Cleans up and exits

# Verify deployment on remote server:
ssh user@hostname "file ~/.sshfwd/x86_64/sshfwd-agent"
# Should show: ELF 64-bit LSB pie executable, x86-64

# Verify cleanup:
ssh user@hostname "ps aux | grep sshfwd-agent"
# Should show: no running processes
```

**Hash verification:**
```bash
# Local binary hash
sha256sum prebuilt-agents/linux-x86_64/sshfwd-agent

# Remote binary hash (should match)
ssh user@hostname "sha256sum ~/.sshfwd/x86_64/sshfwd-agent"
```

## Troubleshooting

**Cross-compilation issues on macOS:**
```bash
# Install musl-cross for Linux target support
brew install filosottile/musl-cross/musl-cross

# Verify linkers are available
which x86_64-linux-musl-gcc aarch64-linux-musl-gcc

# Check .cargo/config.toml has linker configuration:
# [target.x86_64-unknown-linux-musl]
# linker = "x86_64-linux-musl-gcc"
```

**Agent deployment failures:**
- Check SSH connectivity: `ssh user@hostname echo OK`
- Check remote disk space: `ssh user@hostname df -h ~`
- Check remote permissions: `ssh user@hostname mkdir -p ~/.sshfwd && ls -la ~/.sshfwd`

**Timeout issues:**
- First 2 timeouts: Warnings only, agent continues
- Third timeout: Fatal error, connection closed
- Check network stability and remote server load
- Agent heartbeat: Every 2 seconds via stdout

## Development Workflow

**Adding a new platform:**
1. Add target to `PLATFORMS` array in `scripts/build-agents.sh`
2. Add target triple mapping in `crates/sshfwd/src/ssh/agent.rs::Platform::target_triple()`
3. Add platform normalization in `detect_platform()` if needed
4. Update build script to handle platform-specific linker
5. Rebuild agents: `./scripts/build-agents.sh`
6. Test: `cargo run -p sshfwd -- user@newplatform`

**Modifying agent deployment:**
- Agent manager: `crates/sshfwd/src/ssh/agent.rs`
- Platform detection: `AgentManager::detect_platform()`
- Binary resolution: `AgentManager::resolve_agent_binary()`
- Upload logic: `AgentManager::upload()`
- Spawn logic: `AgentManager::spawn_agent()`

**Modifying timeout handling:**
- Discovery stream: `crates/sshfwd/src/discovery/mod.rs`
- Timeout constants: `STALENESS_TIMEOUT`, `MAX_CONSECUTIVE_TIMEOUTS`
- Retry logic: `DiscoveryStream::next_event()`
- Error messages: `crates/sshfwd/src/error.rs`
