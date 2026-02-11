use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum SshError {
    #[error("failed to connect to {destination}: {source}")]
    Connection {
        destination: String,
        source: russh::Error,
    },

    #[error("authentication failed for {destination}: {message}")]
    Auth {
        destination: String,
        message: String,
    },

    #[error("SSH config error: {0}")]
    Config(String),

    #[error("remote command failed: {0}")]
    Remote(russh::Error),

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
}
