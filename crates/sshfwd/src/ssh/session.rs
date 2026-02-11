use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use russh::client::{self, Msg};
use russh::{ChannelMsg, ChannelStream};

use super::config;
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
            let (explicit_user, host) = config::parse_destination(destination);
            let cfg = config::resolve_host_config(&host);

            let user = explicit_user
                .or(cfg.user)
                .unwrap_or_else(|| std::env::var("USER").unwrap_or_else(|_| "root".into()));
            let resolved_host = cfg.hostname.unwrap_or_else(|| host.to_string());
            let resolved_port = cfg.port.unwrap_or(22);

            let (mut handle, jump_session) = if let Some(ref jump_dest) = cfg.proxy_jump {
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
                // Direct TCP connection
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
            if !authenticate(&mut handle, &user, &cfg.identity_files).await? {
                return Err(SshError::Auth {
                    destination: destination.to_string(),
                    message: format!(
                        "all authentication methods failed for {user}@{resolved_host}"
                    ),
                });
            }

            Ok(Self {
                handle: Arc::new(handle),
                _jump_session: jump_session,
            })
        })
    }

    /// Open a direct-tcpip channel for port forwarding.
    pub async fn open_direct_tcpip(
        &self,
        host: &str,
        port: u16,
    ) -> Result<ChannelStream<Msg>, SshError> {
        let channel = self
            .handle
            .channel_open_direct_tcpip(host.to_string(), port as u32, "127.0.0.1", 0)
            .await
            .map_err(SshError::Remote)?;
        Ok(channel.into_stream())
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
    identity_files: &[PathBuf],
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
    for path in identity_files {
        if try_key_file(handle, user, rsa_hash, path).await? {
            return Ok(true);
        }
    }

    // 3. Try default key files
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let default_keys = [
        PathBuf::from(format!("{home}/.ssh/id_ed25519")),
        PathBuf::from(format!("{home}/.ssh/id_rsa")),
        PathBuf::from(format!("{home}/.ssh/id_ecdsa")),
    ];

    for path in &default_keys {
        if try_key_file(handle, user, rsa_hash, path).await? {
            return Ok(true);
        }
    }

    Ok(false)
}

async fn try_key_file(
    handle: &mut client::Handle<ClientHandler>,
    user: &str,
    rsa_hash: Option<russh::keys::HashAlg>,
    path: &Path,
) -> Result<bool, SshError> {
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
