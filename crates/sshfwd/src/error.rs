use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum SshError {
    #[error("failed to connect to {destination}: {source}")]
    Connection {
        destination: String,
        source: openssh::Error,
    },

    #[error("remote command failed: {0}")]
    Remote(openssh::Error),

    #[error("agent deployment failed: {0}")]
    AgentDeploy(String),

    #[error("local I/O error for {path}: {source}")]
    LocalIo {
        path: PathBuf,
        source: std::io::Error,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    #[error("SSH error: {0}")]
    Ssh(#[from] SshError),

    #[error("agent stream ended unexpectedly")]
    StreamEnded,

    #[error("failed to parse agent response: {0}")]
    Parse(String),

    #[error("agent timeout: no response after {consecutive} consecutive timeouts ({timeout_secs}s each)")]
    Timeout {
        timeout_secs: u64,
        consecutive: usize,
    },
}
