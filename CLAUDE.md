# sshfwd.rs

TUI-based SSH port forwarding tool using ratatui, inspired by k9s.

## Commands

- `./scripts/build-agents.sh` — cross-compile agent binaries for all platforms
- `cargo build -p sshfwd` — build main app (embeds prebuilt agents)
- `cargo run -p sshfwd -- user@hostname` — run with TUI
- `cargo run -p sshfwd -- user@hostname --agent-path PATH` — dev override

## Rules

- [Workspace Dependencies](/.claude/rules/workspace-dependencies.md) — versions in root, features in crates
- [TUI Architecture](/.claude/rules/tui-architecture.md) — TEA pattern, exit gotcha, rendering
- [SSH Connection](/.claude/rules/ssh-connection.md) — russh, ssh2-config, ProxyJump, auth
- [Agent & Platform](/.claude/rules/agent-platform.md) — adding platforms, cross-compilation, deployment
- [Port Forwarding](/.claude/rules/port-forwarding.md) — ForwardManager, modal UI, persistence, display rows

## Verification

After making changes:
```bash
cargo fmt -- --check
cargo clippy --all-targets --all-features
cargo test --workspace                      # 22 tests
cargo build -p sshfwd
```

## Publishing to crates.io

**IMPORTANT:** Never run `cargo publish` from local machine. Publishing is automated via GitHub Actions.

**Release process:**
1. Update version in root `Cargo.toml`:
   - `[workspace.package]` version
   - `[workspace.dependencies]` sshfwd-common version
2. Commit: `git add Cargo.toml && git commit -m "Bump version to X.Y.Z"`
3. Tag: `git tag vX.Y.Z && git push origin vX.Y.Z`
4. GitHub Actions automatically:
   - Builds agent binaries for all platforms (Linux x86_64/ARM64, macOS Intel/ARM64)
   - Publishes `sshfwd-common` to crates.io
   - Waits 60 seconds for index update
   - Publishes `sshfwd` to crates.io with embedded agents

**Requirements:**
- `CARGO_REGISTRY_TOKEN` secret configured in GitHub repository settings
