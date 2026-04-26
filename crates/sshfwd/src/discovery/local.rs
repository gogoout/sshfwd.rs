use tokio::task::JoinHandle;

use sshfwd_common::scanner;

use crate::app::Message;

const LOCAL_SCAN_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

/// Spawns a background task that scans local listening ports every 2 seconds.
/// Returns a JoinHandle that can be aborted to stop the scan.
pub fn spawn_local_scan(tx: crossbeam_channel::Sender<Message>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(LOCAL_SCAN_INTERVAL);
        loop {
            interval.tick().await;
            let result = tokio::task::spawn_blocking(move || {
                let mut scanner = scanner::create_scanner();
                scanner.scan()
            })
            .await;
            match result {
                Ok(Ok(scan)) => {
                    if tx.send(Message::LocalScanReceived(scan)).is_err() {
                        break;
                    }
                }
                Ok(Err(e)) => {
                    if tx
                        .send(Message::LocalScanError(format!(
                            "{}: {}",
                            e.kind, e.message
                        )))
                        .is_err()
                    {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    })
}
