use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use russh::client::{self, Msg};
use russh::{ChannelMsg, ChannelStream};

use crate::error::SshError;

/// Output from a remote command execution.
pub struct CommandOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub success: bool,
}

/// Minimal russh client handler — accepts all host keys.
struct ClientHandler;

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::PublicKey,
    ) -> Result<bool, Self::Error> {
        // Accept all host keys (matches previous KnownHosts::Accept behavior)
        Ok(true)
    }
}

/// SSH session backed by russh (pure Rust, zero child processes).
///
/// Wrapped in `Arc` so the session can be shared between `AgentManager`
/// (short-lived commands) and `DiscoveryStream` (keeps connection alive).
/// `_jump_session` keeps any ProxyJump hop alive for the connection's lifetime.
#[derive(Clone)]
pub struct Session {
    handle: Arc<client::Handle<ClientHandler>>,
    _jump_session: Option<Box<Session>>,
}

impl Session {
    /// Connect and authenticate to a remote host, respecting ~/.ssh/config.
    ///
    /// Handles ProxyJump by recursively connecting through jump hosts and
    /// tunneling via `channel_open_direct_tcpip`.
    pub fn connect(
        destination: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Self, SshError>> + Send + '_>> {
        Box::pin(async move {
            let (explicit_user, host) = parse_destination(destination);

            // Resolve SSH config for this host (falls back to defaults if no config)
            let ssh_config = russh_config::parse_home(&host)
                .unwrap_or_else(|_| russh_config::Config::default(&host));

            // Our own SSH config parser handles tabs/= separators correctly
            // (russh_config's parser only splits on spaces, silently dropping directives)
            let host_cfg = parse_ssh_host_config(&host);

            let user = explicit_user
                .or(host_cfg.user.clone())
                .unwrap_or_else(|| ssh_config.user());
            let resolved_host = host_cfg
                .hostname
                .unwrap_or_else(|| ssh_config.host().to_string());
            let resolved_port = host_cfg.port.unwrap_or_else(|| ssh_config.port());

            let (mut handle, jump_session) = if let Some(ref jump_dest) = host_cfg.proxy_jump {
                // ProxyJump: connect through the jump host, then tunnel
                let jump = Session::connect(jump_dest).await?;

                let target_host = resolved_host.clone();
                let target_port = resolved_port as u32;

                let channel = jump
                    .handle
                    .channel_open_direct_tcpip(target_host, target_port, "127.0.0.1", 0)
                    .await
                    .map_err(|e| SshError::Connection {
                        destination: destination.to_string(),
                        source: e,
                    })?;

                let tunnel = channel.into_stream();
                let config = Arc::new(client::Config::default());
                let handle = client::connect_stream(config, tunnel, ClientHandler)
                    .await
                    .map_err(|e| SshError::Connection {
                        destination: destination.to_string(),
                        source: e,
                    })?;

                (handle, Some(Box::new(jump)))
            } else {
                // Direct TCP connection (bypasses russh_config::stream() which has
                // a bug formatting I/O errors as literal "0")
                let addr = format!("{resolved_host}:{resolved_port}");
                let stream = tokio::net::TcpStream::connect(&addr)
                    .await
                    .map_err(|e| SshError::Config(format!("failed to connect to {addr}: {e}")))?;

                let config = Arc::new(client::Config::default());
                let handle = client::connect_stream(config, stream, ClientHandler)
                    .await
                    .map_err(|e| SshError::Connection {
                        destination: destination.to_string(),
                        source: e,
                    })?;

                (handle, None)
            };

            // Authenticate
            if !authenticate(&mut handle, &user, &host_cfg.identity_files).await? {
                return Err(SshError::Auth {
                    destination: destination.to_string(),
                    message: format!(
                        "all authentication methods failed (user={user}, host={resolved_host}, identity_files={:?})",
                        host_cfg.identity_files
                    ),
                });
            }

            Ok(Self {
                handle: Arc::new(handle),
                _jump_session: jump_session,
            })
        })
    }

    /// Execute a command and collect all output.
    pub async fn exec(&self, command: &str) -> Result<CommandOutput, SshError> {
        let mut channel = self
            .handle
            .channel_open_session()
            .await
            .map_err(SshError::Remote)?;
        channel
            .exec(true, command)
            .await
            .map_err(SshError::Remote)?;

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut exit_status = 0u32;

        loop {
            match channel.wait().await {
                Some(ChannelMsg::Data { data }) => stdout.extend_from_slice(&data),
                Some(ChannelMsg::ExtendedData { data, ext: 1 }) => stderr.extend_from_slice(&data),
                Some(ChannelMsg::ExitStatus { exit_status: code }) => exit_status = code,
                None => break,
                _ => {}
            }
        }

        Ok(CommandOutput {
            stdout,
            stderr,
            success: exit_status == 0,
        })
    }

    /// Execute a command and return a stream for reading stdout.
    /// The `ChannelStream` implements `AsyncRead`; stderr is silently skipped.
    pub async fn exec_streaming(&self, command: &str) -> Result<ChannelStream<Msg>, SshError> {
        let channel = self
            .handle
            .channel_open_session()
            .await
            .map_err(SshError::Remote)?;
        channel
            .exec(true, command)
            .await
            .map_err(SshError::Remote)?;
        Ok(channel.into_stream())
    }

    /// Execute a command, write data to its stdin, then collect output.
    pub async fn exec_with_stdin(
        &self,
        command: &str,
        data: &[u8],
    ) -> Result<CommandOutput, SshError> {
        let mut channel = self
            .handle
            .channel_open_session()
            .await
            .map_err(SshError::Remote)?;
        channel
            .exec(true, command)
            .await
            .map_err(SshError::Remote)?;
        channel.data(data).await.map_err(SshError::Remote)?;
        channel.eof().await.map_err(SshError::Remote)?;

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut exit_status = 0u32;

        loop {
            match channel.wait().await {
                Some(ChannelMsg::Data { data }) => stdout.extend_from_slice(&data),
                Some(ChannelMsg::ExtendedData { data, ext: 1 }) => stderr.extend_from_slice(&data),
                Some(ChannelMsg::ExitStatus { exit_status: code }) => exit_status = code,
                None => break,
                _ => {}
            }
        }

        Ok(CommandOutput {
            stdout,
            stderr,
            success: exit_status == 0,
        })
    }
}

/// Try ssh-agent first, then IdentityFile from config, then default key locations.
async fn authenticate(
    handle: &mut client::Handle<ClientHandler>,
    user: &str,
    identity_files: &[String],
) -> Result<bool, SshError> {
    let rsa_hash = handle
        .best_supported_rsa_hash()
        .await
        .ok()
        .flatten()
        .flatten();

    // 1. Try ssh-agent
    if let Ok(mut agent) = russh::keys::agent::client::AgentClient::connect_env().await {
        if let Ok(identities) = agent.request_identities().await {
            for key in identities {
                match handle
                    .authenticate_publickey_with(user, key, rsa_hash, &mut agent)
                    .await
                {
                    Ok(res) if res.success() => return Ok(true),
                    _ => continue,
                }
            }
        }
    }

    // 2. Try IdentityFile from SSH config
    for path_str in identity_files {
        if try_key_file(handle, user, rsa_hash, path_str).await? {
            return Ok(true);
        }
    }

    // 3. Try default key files
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let default_keys = [
        format!("{home}/.ssh/id_ed25519"),
        format!("{home}/.ssh/id_rsa"),
        format!("{home}/.ssh/id_ecdsa"),
    ];

    for path_str in &default_keys {
        if try_key_file(handle, user, rsa_hash, path_str).await? {
            return Ok(true);
        }
    }

    Ok(false)
}

async fn try_key_file(
    handle: &mut client::Handle<ClientHandler>,
    user: &str,
    rsa_hash: Option<russh::keys::HashAlg>,
    path_str: &str,
) -> Result<bool, SshError> {
    let path = std::path::Path::new(path_str);
    if !path.exists() {
        return Ok(false);
    }
    let key = match russh::keys::load_secret_key(path, None) {
        Ok(k) => k,
        Err(_) => return Ok(false),
    };
    let key = russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key), rsa_hash);
    match handle.authenticate_publickey(user, key).await {
        Ok(res) if res.success() => Ok(true),
        _ => Ok(false),
    }
}

fn parse_destination(destination: &str) -> (Option<String>, String) {
    if let Some((user, host)) = destination.split_once('@') {
        (Some(user.to_string()), host.to_string())
    } else {
        (None, destination.to_string())
    }
}

/// SSH config fields parsed from `~/.ssh/config`.
///
/// russh_config's parser only splits on spaces, silently dropping directives
/// in tab-separated configs. We parse the fields we need ourselves, handling
/// tabs, spaces, and `=` separators correctly.
struct SshHostConfig {
    hostname: Option<String>,
    port: Option<u16>,
    user: Option<String>,
    proxy_jump: Option<String>,
    identity_files: Vec<String>,
}

fn parse_ssh_host_config(host: &str) -> SshHostConfig {
    let mut cfg = SshHostConfig {
        hostname: None,
        port: None,
        user: None,
        proxy_jump: None,
        identity_files: Vec::new(),
    };

    let home = match std::env::var("HOME") {
        Ok(h) => h,
        Err(_) => return cfg,
    };
    let content = match std::fs::read_to_string(format!("{home}/.ssh/config")) {
        Ok(c) => c,
        Err(_) => return cfg,
    };

    let mut in_matching_block = false;
    let mut seen_proxy_jump = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let (key, value) = match trimmed.split_once(|c: char| c.is_whitespace() || c == '=') {
            Some((k, v)) => (k.trim(), v.trim()),
            None => continue,
        };

        if key.eq_ignore_ascii_case("Host") {
            // Match all patterns in this Host line (supports globs like * and prefix*)
            in_matching_block = value
                .split_whitespace()
                .any(|p| host_pattern_matches(p, host));
        } else if key.eq_ignore_ascii_case("Match") {
            in_matching_block = false;
        } else if in_matching_block {
            if key.eq_ignore_ascii_case("ProxyJump") && !seen_proxy_jump {
                seen_proxy_jump = true;
                if !value.eq_ignore_ascii_case("none") {
                    cfg.proxy_jump = Some(value.to_string());
                }
            } else if key.eq_ignore_ascii_case("HostName") && cfg.hostname.is_none() {
                cfg.hostname = Some(value.to_string());
            } else if key.eq_ignore_ascii_case("User") && cfg.user.is_none() {
                cfg.user = Some(value.to_string());
            } else if key.eq_ignore_ascii_case("Port") && cfg.port.is_none() {
                cfg.port = value.parse().ok();
            } else if key.eq_ignore_ascii_case("IdentityFile") {
                let path = if let Some(rest) = value.strip_prefix("~/") {
                    format!("{home}/{rest}")
                } else {
                    value.to_string()
                };
                cfg.identity_files.push(path);
            }
        }
    }

    cfg
}

/// Basic SSH Host pattern matching (covers `*`, `prefix*`, `*suffix`).
fn host_pattern_matches(pattern: &str, host: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return host.starts_with(prefix);
    }
    if let Some(suffix) = pattern.strip_prefix('*') {
        return host.ends_with(suffix);
    }
    pattern == host
}
