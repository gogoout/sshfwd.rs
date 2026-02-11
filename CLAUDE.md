# sshfwd.rs

TUI-based SSH port forwarding tool using ratatui, inspired by k9s.

## Commands

- `./scripts/build-agents.sh` — cross-compile agent binaries for all platforms
- `cargo run -p sshfwd -- user@hostname` — run with TUI
- `cargo run -p sshfwd -- user@hostname --agent-path PATH` — dev override for local agent

## Rules

- [Workspace Dependencies](/.claude/rules/workspace-dependencies.md) — versions in root, features in crates
- [TUI Architecture](/.claude/rules/tui-architecture.md) — TEA pattern, exit gotcha, rendering
- [SSH Connection](/.claude/rules/ssh-connection.md) — russh, ssh2-config, ProxyJump, auth
- [Agent & Platform](/.claude/rules/agent-platform.md) — adding platforms, cross-compilation, deployment
- [Port Forwarding](/.claude/rules/port-forwarding.md) — ForwardManager, modal UI, persistence, display rows
- [Publishing](/.claude/rules/publishing.md) — **NEVER `cargo publish` locally**, use GitHub Actions

## Verification

After making changes:
```bash
cargo fmt -- --check
cargo clippy --all-targets --all-features
cargo test --workspace                      # 22 tests
cargo build -p sshfwd
```
