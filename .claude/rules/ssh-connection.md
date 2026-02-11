# SSH Connection (russh)

## Why not openssh crate?

The previous `openssh` crate spawned SSH master processes that fought with the TUI for terminal control (raw mode, signal handlers). `russh` is pure Rust — in-process TCP sockets and async streams, no child processes.

## SSH config parsing

Uses [`ssh2-config`](https://crates.io/crates/ssh2-config) for `~/.ssh/config` parsing. This handles `Include` directives, proper glob matching, tabs/spaces/`=` separators, and all standard SSH directives.

Parsed via `SshConfig::parse_default_file()` with `ALLOW_UNKNOWN_FIELDS | ALLOW_UNSUPPORTED_FIELDS` to avoid errors on unrecognized directives.

Config resolution lives in `crates/sshfwd/src/ssh/config.rs` — a thin wrapper mapping `HostParams` fields to our `ResolvedConfig` struct.

## Connection flow

1. Parse destination (`user@host`)
2. Resolve SSH config via `ssh2-config`
3. If ProxyJump: recursively connect to jump host, tunnel via `channel_open_direct_tcpip`
4. Otherwise: direct `TcpStream::connect` to resolved host:port
5. Auth: ssh-agent → IdentityFile from config → default keys (`id_ed25519`, `id_rsa`, `id_ecdsa`)

## Adding SSH config directives

To use an additional directive from `ssh2-config`:
1. Add field to `ResolvedConfig` in `config.rs`
2. Map from `HostParams` in `resolve_host_config()`
3. Use the parsed value in `Session::connect()`
