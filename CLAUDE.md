# sshfwd.rs

TUI-based SSH port forwarding management tool using ratatui, inspired by k9s keyboard layout.

## Commands

- `cargo test <test_name>` - Run specific test
- `cargo test -- --nocapture` - Run tests with output visible
- `cargo clippy --all-targets --all-features` - Comprehensive linting
- `cargo fmt -- --check` - Check formatting without modifying files

## Notes

- Project is in initial setup phase
- Target platforms: Linux (current), macOS and Windows (planned)

## Verification

After making changes, run:
- `cargo fmt -- --check` - Verify formatting
- `cargo clippy --all-targets --all-features` - Check for issues
- `cargo test` - Ensure tests pass
- `cargo build` - Verify compilation
