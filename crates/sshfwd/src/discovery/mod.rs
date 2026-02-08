use std::path::Path;
use std::time::Duration;

use openssh::{Child, ChildStdout, Session};
use tokio::io::{AsyncBufReadExt, BufReader, Lines};

use sshfwd_common::types::{AgentResponse, ScanResult};

use crate::error::DiscoveryError;
use crate::ssh::agent::AgentManager;

const STALENESS_TIMEOUT: Duration = Duration::from_secs(6);
const MAX_CONSECUTIVE_TIMEOUTS: usize = 3;

/// Events produced by the discovery stream.
#[derive(Debug)]
pub enum DiscoveryEvent {
    Scan(ScanResult),
    Warning(String),
    Error(DiscoveryError),
}

/// Active discovery session — reads agent stdout line by line.
pub struct DiscoveryStream<'s> {
    lines: Lines<BufReader<ChildStdout>>,
    _child: Child<&'s Session>,
    consecutive_timeouts: usize,
}

impl<'s> DiscoveryStream<'s> {
    /// Deploy the agent and start the discovery stream.
    ///
    /// If `local_agent_path` is provided, uses that binary directly (development override).
    /// Otherwise, uses embedded or prebuilt binaries.
    pub async fn start(
        session: &'s Session,
        local_agent_path: Option<&Path>,
    ) -> Result<Self, DiscoveryError> {
        let manager = AgentManager::new(session);

        let mut child = manager
            .deploy_and_spawn(local_agent_path)
            .await
            .map_err(DiscoveryError::Ssh)?;

        let stdout = child.stdout().take().ok_or(DiscoveryError::StreamEnded)?;

        let reader = BufReader::new(stdout);
        let lines = reader.lines();

        Ok(Self {
            lines,
            _child: child,
            consecutive_timeouts: 0,
        })
    }

    /// Read the next event from the agent stream.
    /// Returns None when the stream is exhausted.
    pub async fn next_event(&mut self) -> Option<DiscoveryEvent> {
        let line_result = tokio::time::timeout(STALENESS_TIMEOUT, self.lines.next_line()).await;

        match line_result {
            Ok(Ok(Some(line))) => {
                self.consecutive_timeouts = 0;
                match serde_json::from_str::<AgentResponse>(&line) {
                    Ok(AgentResponse::Ok(scan)) => Some(DiscoveryEvent::Scan(scan)),
                    Ok(AgentResponse::Error(e)) => {
                        let msg = format!("agent error ({}): {}", e.kind, e.message);
                        Some(DiscoveryEvent::Warning(msg))
                    }
                    Err(e) => Some(DiscoveryEvent::Error(DiscoveryError::Parse(format!(
                        "{e}: {line}"
                    )))),
                }
            }
            Ok(Ok(None)) => Some(DiscoveryEvent::Error(DiscoveryError::StreamEnded)),
            Ok(Err(e)) => Some(DiscoveryEvent::Error(DiscoveryError::Parse(format!(
                "I/O error: {e}"
            )))),
            Err(_) => {
                self.consecutive_timeouts += 1;
                if self.consecutive_timeouts >= MAX_CONSECUTIVE_TIMEOUTS {
                    Some(DiscoveryEvent::Error(DiscoveryError::Timeout {
                        timeout_secs: STALENESS_TIMEOUT.as_secs(),
                        consecutive: self.consecutive_timeouts,
                    }))
                } else {
                    Some(DiscoveryEvent::Warning(format!(
                        "agent timeout ({}/{}): no response within {}s",
                        self.consecutive_timeouts,
                        MAX_CONSECUTIVE_TIMEOUTS,
                        STALENESS_TIMEOUT.as_secs(),
                    )))
                }
            }
        }
    }
}
