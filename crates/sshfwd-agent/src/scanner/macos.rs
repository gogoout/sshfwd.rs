use sshfwd_common::types::{AgentError, ScanResult};

use super::Scanner;

pub struct MacosScanner {
    scan_index: u64,
}

impl MacosScanner {
    pub fn new() -> Self {
        Self { scan_index: 0 }
    }
}

impl Scanner for MacosScanner {
    fn scan(&mut self) -> Result<ScanResult, AgentError> {
        let hostname = get_hostname();
        let uid = unsafe { libc::getuid() };
        let username = get_username(uid);
        let is_root = uid == 0;

        let result = ScanResult {
            agent_version: env!("CARGO_PKG_VERSION").to_string(),
            hostname,
            username,
            is_root,
            ports: vec![],
            warnings: vec![
                "macOS scanner not yet implemented; returning empty port list".to_string(),
            ],
            scan_index: self.scan_index,
        };
        self.scan_index += 1;
        Ok(result)
    }
}

fn get_hostname() -> String {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn get_username(uid: u32) -> String {
    std::process::Command::new("id")
        .args(["-un"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_else(|| format!("uid:{uid}"))
        .trim()
        .to_string()
}
