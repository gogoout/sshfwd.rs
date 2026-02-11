use std::path::Path;

use russh::client::Msg;
use russh::ChannelStream;
use tokio::io::{AsyncBufReadExt, BufReader, Lines};

use sshfwd_common::types::{AgentResponse, ScanResult};

use crate::error::DiscoveryError;
use crate::ssh::agent::AgentManager;
use crate::ssh::session::Session;

/// Events produced by the discovery stream.
#[derive(Debug)]
pub enum DiscoveryEvent {
    Scan(ScanResult),
    Warning(String),
    Error(DiscoveryError),
}

/// Active discovery session — reads agent stdout line by line.
pub struct DiscoveryStream {
    lines: Lines<BufReader<ChannelStream<Msg>>>,
    _session: Session, // Keep the SSH connection alive
}

impl DiscoveryStream {
    /// Deploy the agent and start the discovery stream.
    ///
    /// If `local_agent_path` is provided, uses that binary directly (development override).
    /// Otherwise, uses embedded or prebuilt binaries.
    pub async fn start(
        session: Session,
        local_agent_path: Option<&Path>,
    ) -> Result<Self, DiscoveryError> {
        let manager = AgentManager::new(session.clone());

        let stream = manager
            .deploy_and_spawn(local_agent_path)
            .await
            .map_err(DiscoveryError::Ssh)?;

        let reader = BufReader::new(stream);
        let lines = reader.lines();

        Ok(Self {
            lines,
            _session: session,
        })
    }

    /// Read the next event from the agent stream.
    /// Returns None when the stream is exhausted.
    pub async fn next_event(&mut self) -> Option<DiscoveryEvent> {
        match self.lines.next_line().await {
            Ok(Some(line)) => match serde_json::from_str::<AgentResponse>(&line) {
                Ok(AgentResponse::Ok(scan)) => Some(DiscoveryEvent::Scan(scan)),
                Ok(AgentResponse::Error(e)) => {
                    let msg = format!("agent error ({}): {}", e.kind, e.message);
                    Some(DiscoveryEvent::Warning(msg))
                }
                Err(e) => Some(DiscoveryEvent::Error(DiscoveryError::Parse(format!(
                    "{e}: {line}"
                )))),
            },
            Ok(None) => Some(DiscoveryEvent::Error(DiscoveryError::StreamEnded)),
            Err(e) => Some(DiscoveryEvent::Error(DiscoveryError::Parse(format!(
                "I/O error: {e}"
            )))),
        }
    }
}
