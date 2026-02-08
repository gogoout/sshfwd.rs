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

- ✅ Linux (current)
- 🚧 macOS (planned)
- 🚧 Windows (planned)

## Technology Stack

- **UI Framework**: [ratatui](https://github.com/ratatui-org/ratatui) - Terminal user interface library
- **Language**: Rust
- **Inspiration**: k9s keyboard layout and interface design

## Installation

```bash
# Clone the repository
git clone https://github.com/yourusername/sshfwd.rs.git
cd sshfwd.rs

# Build and install
cargo install --path .
```

## Usage

```bash
# Launch the TUI
sshfwd

# Connect to a specific host
sshfwd user@hostname
```

### Keyboard Shortcuts

_(TODO: Document keyboard shortcuts once implemented)_

## Comparison with VS Code Remote

While VS Code offers automatic port forwarding when it detects ports in terminal output, `sshfwd` provides:

- Dedicated TUI interface for port management
- Persistent port annotations and comments
- Faster keyboard-driven workflow
- Lightweight alternative that doesn't require VS Code

## Development

See [CLAUDE.md](./CLAUDE.md) for development commands and architecture information.

### Building

```bash
cargo build                # Debug build
cargo build --release      # Release build
```

### Testing

```bash
cargo test
```

### Linting

```bash
cargo clippy --all-targets --all-features
cargo fmt
```

## Roadmap

- [ ] Core SSH connection management
- [ ] Port detection on remote servers
- [ ] TUI interface with ratatui
- [ ] Port forwarding toggle functionality
- [ ] Connection history persistence
- [ ] Port annotation system
- [ ] macOS support
- [ ] Windows support
- [ ] Configuration file support
- [ ] Custom keyboard shortcut mapping

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

_(TODO: Add license information)_

## Acknowledgments

- [ratatui](https://github.com/ratatui-org/ratatui) for the excellent TUI framework
- [k9s](https://github.com/derailed/k9s) for interface design inspiration
- VS Code Remote Development for feature inspiration
