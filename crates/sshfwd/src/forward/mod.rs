pub mod persistence;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::task::JoinSet;

use crate::ssh::session::Session;

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
        remote_port: u16,
        local_port: u16,
        remote_host: String,
    },
    Stop {
        remote_port: u16,
    },
    Reactivate {
        remote_port: u16,
        local_port: u16,
        remote_host: String,
    },
    Pause {
        remote_port: u16,
    },
}

#[derive(Debug)]
pub enum ForwardEvent {
    Started { remote_port: u16, local_port: u16 },
    Stopped { remote_port: u16 },
    Paused { remote_port: u16 },
    BindError { remote_port: u16, message: String },
    ConnectionCountChanged { remote_port: u16, count: u32 },
}

struct ListenerHandle {
    local_port: u16,
    remote_host: String,
    abort_handle: tokio::task::AbortHandle,
}

pub struct ForwardManager {
    session: Session,
    cmd_rx: mpsc::UnboundedReceiver<ForwardCommand>,
    event_tx: crossbeam_channel::Sender<crate::app::Message>,
    listeners: HashMap<u16, ListenerHandle>,
}

impl ForwardManager {
    pub fn new(
        session: Session,
        cmd_rx: mpsc::UnboundedReceiver<ForwardCommand>,
        event_tx: crossbeam_channel::Sender<crate::app::Message>,
    ) -> Self {
        Self {
            session,
            cmd_rx,
            event_tx,
            listeners: HashMap::new(),
        }
    }

    pub async fn run(mut self) {
        while let Some(cmd) = self.cmd_rx.recv().await {
            match cmd {
                ForwardCommand::Start {
                    remote_port,
                    local_port,
                    remote_host,
                } => self.handle_start(remote_port, local_port, remote_host),
                ForwardCommand::Stop { remote_port } => self.handle_stop(remote_port),
                ForwardCommand::Reactivate {
                    remote_port,
                    local_port,
                    remote_host,
                } => {
                    let port = self
                        .listeners
                        .get(&remote_port)
                        .map_or(local_port, |h| h.local_port);
                    self.handle_start(remote_port, port, remote_host);
                }
                ForwardCommand::Pause { remote_port } => self.handle_pause(remote_port),
            }
        }
    }

    fn handle_start(&mut self, remote_port: u16, local_port: u16, remote_host: String) {
        // Stop existing listener if any
        if let Some(handle) = self.listeners.remove(&remote_port) {
            handle.abort_handle.abort();
        }

        let session = self.session.clone();
        let event_tx = self.event_tx.clone();
        let host = remote_host.clone();

        let join_handle = tokio::spawn(async move {
            let listener = match TcpListener::bind(("127.0.0.1", local_port)).await {
                Ok(l) => l,
                Err(e) => {
                    let _ =
                        event_tx.send(crate::app::Message::ForwardEvent(ForwardEvent::BindError {
                            remote_port,
                            message: e.to_string(),
                        }));
                    return;
                }
            };

            let actual_port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
            let _ = event_tx.send(crate::app::Message::ForwardEvent(ForwardEvent::Started {
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
            remote_port,
            ListenerHandle {
                local_port,
                remote_host,
                abort_handle,
            },
        );
    }

    fn handle_stop(&mut self, remote_port: u16) {
        if let Some(handle) = self.listeners.remove(&remote_port) {
            handle.abort_handle.abort();
        }
        let _ = self
            .event_tx
            .send(crate::app::Message::ForwardEvent(ForwardEvent::Stopped {
                remote_port,
            }));
    }

    fn handle_pause(&mut self, remote_port: u16) {
        if let Some(handle) = self.listeners.remove(&remote_port) {
            handle.abort_handle.abort();
            // Re-insert without abort handle — just remember the port mapping
            self.listeners.insert(
                remote_port,
                ListenerHandle {
                    local_port: handle.local_port,
                    remote_host: handle.remote_host,
                    abort_handle: handle.abort_handle, // already aborted
                },
            );
        }
        let _ = self
            .event_tx
            .send(crate::app::Message::ForwardEvent(ForwardEvent::Paused {
                remote_port,
            }));
    }
}

/// Compare current scan ports against tracked forwards and produce
/// Pause/Reactivate commands. Also updates entry statuses in-place.
pub fn reconcile_forwards(
    forwards: &mut HashMap<u16, ForwardEntry>,
    current_remote_ports: &HashSet<u16>,
    remote_host: &str,
) -> Vec<ForwardCommand> {
    let mut commands = Vec::new();

    for (&remote_port, entry) in forwards.iter() {
        match entry.status {
            ForwardStatus::Active | ForwardStatus::Starting => {
                if !current_remote_ports.contains(&remote_port) {
                    commands.push(ForwardCommand::Pause { remote_port });
                }
            }
            ForwardStatus::Paused => {
                if current_remote_ports.contains(&remote_port) {
                    commands.push(ForwardCommand::Reactivate {
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
            ForwardCommand::Pause { remote_port } => {
                if let Some(entry) = forwards.get_mut(remote_port) {
                    entry.status = ForwardStatus::Paused;
                }
            }
            ForwardCommand::Reactivate { remote_port, .. } => {
                if let Some(entry) = forwards.get_mut(remote_port) {
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
