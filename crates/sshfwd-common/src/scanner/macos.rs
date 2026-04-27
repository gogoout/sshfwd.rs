use std::collections::HashMap;
use std::process::Command;

use crate::types::{AgentError, AgentErrorKind, ListeningPort, ProcessInfo, Protocol, ScanResult};

use super::Scanner;

pub struct MacosScanner {
    scan_index: u64,
}

impl MacosScanner {
    pub fn new() -> Self {
        Self { scan_index: 0 }
    }
}

impl Default for MacosScanner {
    fn default() -> Self {
        Self::new()
    }
}

impl Scanner for MacosScanner {
    fn scan(&mut self) -> Result<ScanResult, AgentError> {
        let ports = scan_listening_ports()?;

        let hostname = Command::new("hostname")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .unwrap_or_default()
            .trim()
            .to_string();

        let uid = unsafe { libc::getuid() };
        let username = Command::new("id")
            .args(["-un"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .unwrap_or_else(|| format!("uid:{uid}"))
            .trim()
            .to_string();

        let result = ScanResult {
            agent_version: env!("CARGO_PKG_VERSION").to_string(),
            hostname,
            username,
            is_root: uid == 0,
            ports,
            warnings: vec![],
            scan_index: self.scan_index,
        };
        self.scan_index += 1;
        Ok(result)
    }
}

/// Use `lsof` to enumerate listening TCP/TCP6 sockets.
///
/// Example lsof output line:
///   node  1234 user  18u  IPv4  0x...  0t0  TCP *:3000 (LISTEN)
///   ruby  5678 user  10u  IPv6  0x...  0t0  TCP localhost:4000 (LISTEN)
fn scan_listening_ports() -> Result<Vec<ListeningPort>, AgentError> {
    let output = Command::new("lsof")
        .args(["-nP", "-iTCP", "-sTCP:LISTEN"])
        .output()
        .map_err(|e| AgentError {
            kind: AgentErrorKind::ScanFailed,
            message: format!("lsof failed: {e}"),
        })?;

    let stdout = String::from_utf8_lossy(&output.stdout);

    // pid → ProcessInfo cache so we only read cmdline once per pid
    let mut process_cache: HashMap<u32, ProcessInfo> = HashMap::new();

    let mut ports = Vec::new();
    for line in stdout.lines().skip(1) {
        // skip header
        if let Some(port) = parse_lsof_line(line, &mut process_cache) {
            ports.push(port);
        }
    }

    // Deduplicate: same port+protocol may appear multiple times (IPv4 and IPv6 listeners)
    ports.sort_by_key(|p| (p.port, format!("{:?}", p.protocol)));
    ports.dedup_by_key(|p| p.port);

    Ok(ports)
}

/// Parse one lsof output line into a `ListeningPort`.
///
/// lsof columns (space-separated, variable width):
///   COMMAND  PID  USER  FD  TYPE  DEVICE  SIZE  NODE  NAME
/// NAME looks like: `*:3000`, `127.0.0.1:5432`, `[::1]:8080`
fn parse_lsof_line(
    line: &str,
    process_cache: &mut HashMap<u32, ProcessInfo>,
) -> Option<ListeningPort> {
    let mut fields = line.split_whitespace();
    let command = fields.next()?;
    let pid: u32 = fields.next()?.parse().ok()?;
    let _user = fields.next()?;
    let _fd = fields.next()?;
    let type_field = fields.next()?; // IPv4 / IPv6
    let _device = fields.next()?;
    let _size = fields.next()?;
    let _node = fields.next()?;
    let name = fields.next()?; // e.g. "*:3000" or "127.0.0.1:5432"

    let protocol = match type_field {
        "IPv6" => Protocol::Tcp6,
        _ => Protocol::Tcp,
    };

    // Extract port from the last `:N` segment of the NAME field
    let port: u16 = name.rsplit(':').next()?.parse().ok()?;

    let local_addr = name
        .rsplit_once(':')
        .map(|(addr, _)| addr.trim_matches(['[', ']']).to_string())
        .unwrap_or_default();

    let process = Some(
        process_cache
            .entry(pid)
            .or_insert_with(|| read_process_info(pid, command))
            .clone(),
    );

    Some(ListeningPort {
        protocol,
        local_addr,
        port,
        process,
    })
}

fn read_process_info(pid: u32, command: &str) -> ProcessInfo {
    // Full command line via `ps`
    let cmdline = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default()
        .trim()
        .to_string();

    let uid = unsafe { libc::getuid() };

    ProcessInfo {
        pid,
        name: command.to_string(),
        cmdline: if cmdline.is_empty() {
            command.to_string()
        } else {
            cmdline
        },
        uid,
    }
}
