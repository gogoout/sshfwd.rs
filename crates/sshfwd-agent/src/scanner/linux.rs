use std::collections::HashMap;
use std::fs;

use sshfwd_common::types::{
    AgentError, AgentErrorKind, ListeningPort, ProcessInfo, Protocol, ScanResult,
};

use super::proc_net_tcp::{dedup_entries, parse_proc_net_tcp};
use super::Scanner;

pub struct LinuxScanner {
    scan_index: u64,
}

impl LinuxScanner {
    pub fn new() -> Self {
        Self { scan_index: 0 }
    }
}

impl Scanner for LinuxScanner {
    fn scan(&mut self) -> Result<ScanResult, AgentError> {
        let mut warnings = Vec::new();

        let tcp_content = fs::read_to_string("/proc/net/tcp").map_err(|e| AgentError {
            kind: AgentErrorKind::ScanFailed,
            message: format!("failed to read /proc/net/tcp: {e}"),
        })?;

        let tcp6_content = fs::read_to_string("/proc/net/tcp6").unwrap_or_default();

        let mut entries = parse_proc_net_tcp(&tcp_content, Protocol::Tcp);
        entries.extend(parse_proc_net_tcp(&tcp6_content, Protocol::Tcp6));

        entries = dedup_entries(entries);

        let inode_uid_map: HashMap<u64, u32> = entries.iter().map(|e| (e.inode, e.uid)).collect();

        let inode_to_process = map_inodes_to_processes(&inode_uid_map, &mut warnings);

        let ports: Vec<ListeningPort> = entries
            .into_iter()
            .map(|entry| ListeningPort {
                protocol: entry.protocol,
                local_addr: entry.local_addr,
                port: entry.port,
                process: inode_to_process.get(&entry.inode).cloned(),
            })
            .collect();

        let hostname = fs::read_to_string("/etc/hostname")
            .unwrap_or_default()
            .trim()
            .to_string();
        let uid = unsafe { libc::getuid() };
        let username = get_username(uid);
        let is_root = uid == 0;

        let result = ScanResult {
            agent_version: env!("CARGO_PKG_VERSION").to_string(),
            hostname,
            username,
            is_root,
            ports,
            warnings,
            scan_index: self.scan_index,
        };
        self.scan_index += 1;
        Ok(result)
    }
}

/// Map socket inodes to process information by walking /proc/[pid]/fd/.
fn map_inodes_to_processes(
    inode_uid_map: &HashMap<u64, u32>,
    warnings: &mut Vec<String>,
) -> HashMap<u64, ProcessInfo> {
    let mut result = HashMap::new();

    let proc_dir = match fs::read_dir("/proc") {
        Ok(d) => d,
        Err(e) => {
            warnings.push(format!("cannot read /proc: {e}"));
            return result;
        }
    };

    let target_uids: std::collections::HashSet<u32> = inode_uid_map.values().copied().collect();
    let target_inodes: std::collections::HashSet<u64> = inode_uid_map.keys().copied().collect();

    for entry in proc_dir.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let pid: u32 = match name_str.parse() {
            Ok(v) => v,
            Err(_) => continue,
        };

        let status_path = format!("/proc/{pid}/status");
        let proc_uid = match read_uid_from_status(&status_path) {
            Some(uid) => uid,
            None => continue,
        };

        if !target_uids.contains(&proc_uid) {
            continue;
        }

        let fd_path = format!("/proc/{pid}/fd");
        let fd_dir = match fs::read_dir(&fd_path) {
            Ok(d) => d,
            Err(e) => {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    warnings.push(format!("permission denied reading /proc/{pid}/fd"));
                }
                continue;
            }
        };

        for fd_entry in fd_dir.flatten() {
            let link = match fs::read_link(fd_entry.path()) {
                Ok(l) => l,
                Err(_) => continue,
            };
            let link_str = link.to_string_lossy();
            if let Some(inode_str) = link_str
                .strip_prefix("socket:[")
                .and_then(|s| s.strip_suffix(']'))
            {
                let inode: u64 = match inode_str.parse() {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if target_inodes.contains(&inode) && !result.contains_key(&inode) {
                    let info = read_process_info(pid, proc_uid);
                    result.insert(inode, info);
                }
            }
        }
    }

    result
}

fn read_uid_from_status(path: &str) -> Option<u32> {
    let content = fs::read_to_string(path).ok()?;
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("Uid:") {
            let uid_str = rest.split_whitespace().next()?;
            return uid_str.parse().ok();
        }
    }
    None
}

fn read_process_info(pid: u32, uid: u32) -> ProcessInfo {
    let name = fs::read_to_string(format!("/proc/{pid}/comm"))
        .unwrap_or_default()
        .trim()
        .to_string();

    let cmdline = fs::read_to_string(format!("/proc/{pid}/cmdline"))
        .unwrap_or_default()
        .replace('\0', " ")
        .trim()
        .to_string();

    ProcessInfo {
        pid,
        name,
        cmdline,
        uid,
    }
}

fn get_username(uid: u32) -> String {
    if let Ok(content) = fs::read_to_string("/etc/passwd") {
        for line in content.lines() {
            let fields: Vec<&str> = line.split(':').collect();
            if fields.len() >= 3 {
                if let Ok(file_uid) = fields[2].parse::<u32>() {
                    if file_uid == uid {
                        return fields[0].to_string();
                    }
                }
            }
        }
    }
    format!("uid:{uid}")
}
