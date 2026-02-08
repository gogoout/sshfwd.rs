use openssh::{KnownHosts, Session};

use crate::error::SshError;

/// Connect to a remote host via SSH, respecting ~/.ssh/config.
pub async fn connect(destination: &str) -> Result<Session, SshError> {
    let session = Session::connect(destination, KnownHosts::Accept)
        .await
        .map_err(|e| SshError::Connection {
            destination: destination.to_string(),
            source: e,
        })?;
    Ok(session)
}
