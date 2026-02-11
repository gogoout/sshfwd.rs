# SSH Connection (russh)

## Why not openssh crate?

The previous `openssh` crate spawned SSH master processes that fought with the TUI for terminal control (raw mode, signal handlers). `russh` is pure Rust — in-process TCP sockets and async streams, no child processes.

## SSH config parsing

`russh_config` has two bugs:
1. **Error formatting**: `#[error("{}", 0)]` displays literal "0" instead of the I/O error
2. **Parser**: `splitn(2, ' ')` only splits on spaces — tab-separated directives are silently dropped

We bypass both by parsing `~/.ssh/config` ourselves in `session.rs::parse_ssh_host_config()`. This handles tabs, spaces, and `=` separators. Directives parsed: `HostName`, `Port`, `User`, `ProxyJump`, `IdentityFile`.

Host pattern matching supports `*`, `prefix*`, `*suffix`, and exact match — enough for `Host *` wildcard blocks.

## Connection flow

1. Parse destination (`user@host`)
2. Resolve SSH config via our parser + `russh_config` fallback
3. If ProxyJump: recursively connect to jump host, tunnel via `channel_open_direct_tcpip`
4. Otherwise: direct `TcpStream::connect` to resolved host:port
5. Auth: ssh-agent → IdentityFile from config → default keys (`id_ed25519`, `id_rsa`, `id_ecdsa`)

## Adding SSH config directives

To support a new directive (e.g., `ProxyCommand`):
1. Add field to `SshHostConfig` struct
2. Add parsing branch in `parse_ssh_host_config()` loop
3. Use the parsed value in `Session::connect()`
