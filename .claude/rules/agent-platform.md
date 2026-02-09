# Agent & Platform

## Adding a new platform

1. Add target to `PLATFORMS` in `scripts/build-agents.sh`
2. Add target triple in `crates/sshfwd/src/ssh/agent.rs::Platform::target_triple()`
3. Add normalization in `detect_platform()` if needed
4. Rebuild: `./scripts/build-agents.sh`

## Cross-compilation (macOS)

Requires musl-cross: `brew install filosottile/musl-cross/musl-cross`
Linker config is in `.cargo/config.toml`.

## Agent deployment

- Hash-based: only uploads if SHA256 differs
- Atomic: temp file → `mv` → `chmod +x`
- Stale process cleanup via `/proc/{pid}/comm` verification before kill
