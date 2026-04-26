pub mod persistence;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::task::JoinSet;

use crate::ssh::session::{IncomingForward, Session};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ForwardKind {
    #[default]
    Local,
    Reverse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ForwardKey {
    pub kind: ForwardKind,
    pub remote_port: u16,
}

impl ForwardKey {
    pub fn local(remote_port: u16) -> Self {
        Self {
            kind: ForwardKind::Local,
            remote_port,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForwardStatus {
    Active,
    Paused,
    Starting,
}

#[derive(Debug, Clone)]
pub struct ForwardEntry {
    pub local_port: u16,
    pub status: ForwardStatus,
    pub active_connections: u32,
}

pub enum ForwardCommand {
    Start {
        kind: ForwardKind,
        remote_port: u16,
        local_port: u16,
        remote_host: String,
    },
    Stop {
        kind: ForwardKind,
        remote_port: u16,
    },
    Reactivate {
        kind: ForwardKind,
        remote_port: u16,
        local_port: u16,
        remote_host: String,
    },
    Pause {
        kind: ForwardKind,
        remote_port: u16,
    },
}

#[derive(Debug)]
pub enum ForwardEvent {
    Started {
        kind: ForwardKind,
        remote_port: u16,
        local_port: u16,
    },
    Stopped {
        kind: ForwardKind,
        remote_port: u16,
    },
    Paused {
        kind: ForwardKind,
        remote_port: u16,
    },
    BindError {
        kind: ForwardKind,
        remote_port: u16,
        message: String,
    },
    ConnectionCountChanged {
        kind: ForwardKind,
        remote_port: u16,
        count: u32,
    },
}

struct ListenerHandle {
    local_port: u16,
    remote_host: String,
    abort_handle: tokio::task::AbortHandle,
}

pub struct ForwardManager {
    session: Session,
    event_tx: crossbeam_channel::Sender<crate::app::Message>,
    listeners: HashMap<ForwardKey, ListenerHandle>,
    /// Maps remote_port → local_port for active reverse forwards.
    reverse_map: HashMap<u16, u16>,
}

impl ForwardManager {
    pub fn new(session: Session, event_tx: crossbeam_channel::Sender<crate::app::Message>) -> Self {
        Self {
            session,
            event_tx,
            listeners: HashMap::new(),
            reverse_map: HashMap::new(),
        }
    }

    /// Drive the forward manager for one session lifetime.
    ///
    /// `cmd_rx` and `forwarded_rx` are borrowed so the sidecar can reuse them
    /// across reconnect cycles without recreating the command channel.
    /// `shutdown_rx` fires when the sidecar wants a graceful teardown (e.g.
    /// the discovery stream ended).  On shutdown all local listener tasks are
    /// aborted so their ports are released before the next session binds them.
    pub async fn run(
        mut self,
        cmd_rx: &mut mpsc::UnboundedReceiver<ForwardCommand>,
        forwarded_rx: &mut mpsc::UnboundedReceiver<IncomingForward>,
        mut shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    ) {
        loop {
            tokio::select! {
                biased;
                _ = &mut shutdown_rx => {
                    // Abort all local listeners so ports are released for the next session.
                    for (_, handle) in self.listeners.drain() {
                        handle.abort_handle.abort();
                    }
                    break;
                }
                cmd_opt = cmd_rx.recv() => {
                    match cmd_opt {
                        Some(cmd) => self.handle_command(cmd).await,
                        None => break,
                    }
                }
                inc_opt = forwarded_rx.recv() => {
                    if let Some(inc) = inc_opt {
                        self.handle_incoming(inc);
                    }
                }
            }
        }
    }

    async fn handle_command(&mut self, cmd: ForwardCommand) {
        match cmd {
            ForwardCommand::Start {
                kind,
                remote_port,
                local_port,
                remote_host,
            } => {
                let key = ForwardKey { kind, remote_port };
                match kind {
                    ForwardKind::Local => self.handle_start_local(key, local_port, remote_host),
                    ForwardKind::Reverse => self.handle_start_reverse(key, local_port).await,
                }
            }
            ForwardCommand::Stop { kind, remote_port } => {
                let key = ForwardKey { kind, remote_port };
                match kind {
                    ForwardKind::Local => self.handle_stop_local(key),
                    ForwardKind::Reverse => self.handle_stop_reverse(key).await,
                }
            }
            ForwardCommand::Reactivate {
                kind,
                remote_port,
                local_port,
                remote_host,
            } => {
                let key = ForwardKey { kind, remote_port };
                match kind {
                    ForwardKind::Local => {
                        let port = self
                            .listeners
                            .get(&key)
                            .map_or(local_port, |h| h.local_port);
                        self.handle_start_local(key, port, remote_host);
                    }
                    ForwardKind::Reverse => self.handle_start_reverse(key, local_port).await,
                }
            }
            ForwardCommand::Pause { kind, remote_port } => {
                let key = ForwardKey { kind, remote_port };
                match kind {
                    ForwardKind::Local => self.handle_pause_local(key),
                    ForwardKind::Reverse => {
                        unreachable!("reconcile_forwards never pauses Reverse forwards")
                    }
                }
            }
        }
    }

    fn handle_start_local(&mut self, key: ForwardKey, local_port: u16, remote_host: String) {
        // Stop existing listener if any
        if let Some(handle) = self.listeners.remove(&key) {
            handle.abort_handle.abort();
        }

        let remote_port = key.remote_port;
        let kind = key.kind;
        let session = self.session.clone();
        let event_tx = self.event_tx.clone();
        let host = remote_host.clone();

        let join_handle = tokio::spawn(async move {
            let listener = match TcpListener::bind(("127.0.0.1", local_port)).await {
                Ok(l) => l,
                Err(e) => {
                    let _ =
                        event_tx.send(crate::app::Message::ForwardEvent(ForwardEvent::BindError {
                            kind,
                            remote_port,
                            message: e.to_string(),
                        }));
                    return;
                }
            };

            let actual_port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
            let _ = event_tx.send(crate::app::Message::ForwardEvent(ForwardEvent::Started {
                kind,
                remote_port,
                local_port: actual_port,
            }));

            let conn_count = Arc::new(std::sync::atomic::AtomicU32::new(0));
            let mut connections = JoinSet::new();

            loop {
                tokio::select! {
                    accept_result = listener.accept() => {
                        match accept_result {
                            Ok((local_stream, _)) => {
                                let session = session.clone();
                                let host = host.clone();
                                let event_tx = event_tx.clone();
                                let conn_count = conn_count.clone();

                                let count = conn_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                                let _ = event_tx.send(crate::app::Message::ForwardEvent(
                                    ForwardEvent::ConnectionCountChanged {
                                        kind,
                                        remote_port,
                                        count,
                                    },
                                ));

                                connections.spawn(async move {
                                    let result = tunnel_connection(
                                        local_stream,
                                        &session,
                                        &host,
                                        remote_port,
                                    )
                                    .await;

                                    let count = conn_count.fetch_sub(1, std::sync::atomic::Ordering::Relaxed) - 1;
                                    let _ = event_tx.send(crate::app::Message::ForwardEvent(
                                        ForwardEvent::ConnectionCountChanged {
                                            kind,
                                            remote_port,
                                            count,
                                        },
                                    ));

                                    result
                                });
                            }
                            Err(_) => break,
                        }
                    }
                    Some(_) = connections.join_next() => {
                        // Connection finished, count already updated in task
                    }
                }
            }
        });

        let abort_handle = join_handle.abort_handle();
        self.listeners.insert(
            key,
            ListenerHandle {
                local_port,
                remote_host,
                abort_handle,
            },
        );
    }

    fn handle_stop_local(&mut self, key: ForwardKey) {
        if let Some(handle) = self.listeners.remove(&key) {
            handle.abort_handle.abort();
        }
        let _ = self
            .event_tx
            .send(crate::app::Message::ForwardEvent(ForwardEvent::Stopped {
                kind: key.kind,
                remote_port: key.remote_port,
            }));
    }

    fn handle_pause_local(&mut self, key: ForwardKey) {
        if let Some(handle) = self.listeners.remove(&key) {
            handle.abort_handle.abort();
            // Re-insert to preserve local_port/remote_host for the Reactivate path.
            // abort_handle is already aborted (abort() on it is a no-op); it is kept
            // only because ListenerHandle requires one — handle_start_local will abort
            // it again harmlessly when reactivating.
            self.listeners.insert(
                key,
                ListenerHandle {
                    local_port: handle.local_port,
                    remote_host: handle.remote_host,
                    abort_handle: handle.abort_handle,
                },
            );
        }
        let _ = self
            .event_tx
            .send(crate::app::Message::ForwardEvent(ForwardEvent::Paused {
                kind: key.kind,
                remote_port: key.remote_port,
            }));
    }

    async fn handle_start_reverse(&mut self, key: ForwardKey, local_port: u16) {
        let remote_port = key.remote_port;

        match self.session.tcpip_forward(remote_port).await {
            Ok(bound_port) => {
                // bound_port may differ from remote_port if remote_port was 0
                self.reverse_map.insert(bound_port, local_port);
                let _ =
                    self.event_tx
                        .send(crate::app::Message::ForwardEvent(ForwardEvent::Started {
                            kind: ForwardKind::Reverse,
                            remote_port: bound_port,
                            local_port,
                        }));
            }
            Err(e) => {
                let _ = self.event_tx.send(crate::app::Message::ForwardEvent(
                    ForwardEvent::BindError {
                        kind: ForwardKind::Reverse,
                        remote_port,
                        message: e.to_string(),
                    },
                ));
            }
        }
    }

    async fn handle_stop_reverse(&mut self, key: ForwardKey) {
        let remote_port = key.remote_port;
        self.reverse_map.remove(&remote_port);
        // Best-effort cancel — don't fail the stop if the session is dead
        let _ = self.session.cancel_tcpip_forward(remote_port).await;
        let _ = self
            .event_tx
            .send(crate::app::Message::ForwardEvent(ForwardEvent::Stopped {
                kind: ForwardKind::Reverse,
                remote_port,
            }));
    }

    fn handle_incoming(&self, inc: IncomingForward) {
        let Some(&local_port) = self.reverse_map.get(&inc.remote_port) else {
            return; // Unknown port — ignore
        };

        tokio::spawn(async move {
            let local_stream = match tokio::net::TcpStream::connect(("127.0.0.1", local_port)).await
            {
                Ok(s) => s,
                Err(_) => return,
            };

            let channel_stream = inc.channel.into_stream();
            let (mut ssh_r, mut ssh_w) = tokio::io::split(channel_stream);
            let (mut local_r, mut local_w) = tokio::io::split(local_stream);
            tokio::select! {
                _ = tokio::io::copy(&mut local_r, &mut ssh_w) => {}
                _ = tokio::io::copy(&mut ssh_r, &mut local_w) => {}
            }
        });
    }
}

/// Compare current scan ports against tracked forwards and produce
/// Pause/Reactivate commands. Also updates entry statuses in-place.
pub fn reconcile_forwards(
    forwards: &mut HashMap<ForwardKey, ForwardEntry>,
    current_remote_ports: &HashSet<u16>,
    remote_host: &str,
) -> Vec<ForwardCommand> {
    let mut commands = Vec::new();

    for (key, entry) in forwards.iter() {
        if key.kind != ForwardKind::Local {
            continue;
        }
        let remote_port = key.remote_port;
        match entry.status {
            ForwardStatus::Active | ForwardStatus::Starting => {
                if !current_remote_ports.contains(&remote_port) {
                    commands.push(ForwardCommand::Pause {
                        kind: key.kind,
                        remote_port,
                    });
                }
            }
            ForwardStatus::Paused => {
                if current_remote_ports.contains(&remote_port) {
                    commands.push(ForwardCommand::Reactivate {
                        kind: key.kind,
                        remote_port,
                        local_port: entry.local_port,
                        remote_host: remote_host.to_string(),
                    });
                }
            }
        }
    }

    // Update statuses for the commands we just produced
    for cmd in &commands {
        match cmd {
            ForwardCommand::Pause { kind, remote_port } => {
                if let Some(entry) = forwards.get_mut(&ForwardKey {
                    kind: *kind,
                    remote_port: *remote_port,
                }) {
                    entry.status = ForwardStatus::Paused;
                }
            }
            ForwardCommand::Reactivate {
                kind, remote_port, ..
            } => {
                if let Some(entry) = forwards.get_mut(&ForwardKey {
                    kind: *kind,
                    remote_port: *remote_port,
                }) {
                    entry.status = ForwardStatus::Starting;
                }
            }
            _ => {}
        }
    }

    commands
}

async fn tunnel_connection(
    local_stream: tokio::net::TcpStream,
    session: &Session,
    remote_host: &str,
    remote_port: u16,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let channel_stream = session.open_direct_tcpip(remote_host, remote_port).await?;

    let (mut ssh_reader, mut ssh_writer) = tokio::io::split(channel_stream);
    let (mut local_reader, mut local_writer) = tokio::io::split(local_stream);

    tokio::select! {
        r = tokio::io::copy(&mut local_reader, &mut ssh_writer) => { r?; }
        r = tokio::io::copy(&mut ssh_reader, &mut local_writer) => { r?; }
    }

    Ok(())
}
