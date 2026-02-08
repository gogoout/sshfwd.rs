// Pure /proc/net/tcp parsing — no OS-specific syscalls, testable on any platform.
#![allow(dead_code)]

use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr};

use sshfwd_common::types::Protocol;

const LISTEN_STATE: &str = "0A";

/// A parsed entry from /proc/net/tcp or /proc/net/tcp6.
#[derive(Debug, Clone)]
pub struct TcpEntry {
    pub protocol: Protocol,
    pub local_addr: String,
    pub port: u16,
    pub uid: u32,
    pub inode: u64,
}

/// Parse /proc/net/tcp or /proc/net/tcp6 content.
/// Accepts the file content as a string for testability.
pub fn parse_proc_net_tcp(content: &str, protocol: Protocol) -> Vec<TcpEntry> {
    let mut entries = Vec::new();
    for line in content.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 12 {
            continue;
        }

        // Field 3 (index 3) is the state
        let state = fields[3];
        if state != LISTEN_STATE {
            continue;
        }

        // Field 1 (index 1) is local_address:port in hex
        let local_address = fields[1];
        let (addr_str, port) = match parse_address(local_address, protocol) {
            Some(v) => v,
            None => continue,
        };

        // Field 7 (index 7) is UID
        let uid: u32 = match fields[7].parse() {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Field 9 (index 9) is inode
        let inode: u64 = match fields[9].parse() {
            Ok(v) => v,
            Err(_) => continue,
        };

        entries.push(TcpEntry {
            protocol,
            local_addr: addr_str,
            port,
            uid,
            inode,
        });
    }
    entries
}

/// Parse a hex address:port pair from /proc/net/tcp.
fn parse_address(addr_port: &str, protocol: Protocol) -> Option<(String, u16)> {
    let (addr_hex, port_hex) = addr_port.split_once(':')?;
    let port = u16::from_str_radix(port_hex, 16).ok()?;

    let addr_str = match protocol {
        Protocol::Tcp => {
            if addr_hex.len() != 8 {
                return None;
            }
            let addr_u32 = u32::from_str_radix(addr_hex, 16).ok()?;
            // /proc/net/tcp stores addresses in host byte order (little-endian on x86)
            let ip = Ipv4Addr::from(addr_u32.swap_bytes());
            ip.to_string()
        }
        Protocol::Tcp6 => {
            if addr_hex.len() != 32 {
                return None;
            }
            parse_ipv6_addr(addr_hex)?
        }
    };

    Some((addr_str, port))
}

/// Parse a 32-char hex IPv6 address from /proc/net/tcp6.
/// Format: four 32-bit words, each in host byte order (little-endian on x86).
fn parse_ipv6_addr(hex: &str) -> Option<String> {
    if hex.len() != 32 {
        return None;
    }

    let mut octets = [0u8; 16];
    for i in 0..4 {
        let word_hex = &hex[i * 8..(i + 1) * 8];
        let word = u32::from_str_radix(word_hex, 16).ok()?;
        let bytes = word.swap_bytes().to_be_bytes();
        octets[i * 4..i * 4 + 4].copy_from_slice(&bytes);
    }

    let ip = Ipv6Addr::from(octets);
    Some(ip.to_string())
}

/// Normalize an address for deduplication.
/// Maps IPv4-mapped IPv6 (::ffff:x.x.x.x) to its IPv4 form.
fn normalize_addr(addr: &str) -> String {
    if let Some(v4) = addr.strip_prefix("::ffff:") {
        return v4.to_string();
    }
    addr.to_string()
}

/// Deduplicate entries by (port, normalized_address).
pub fn dedup_entries(entries: Vec<TcpEntry>) -> Vec<TcpEntry> {
    let mut seen: HashMap<(u16, String), TcpEntry> = HashMap::new();
    for entry in entries {
        let key = (entry.port, normalize_addr(&entry.local_addr));
        seen.entry(key).or_insert(entry);
    }
    seen.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_TCP: &str = "\
  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:0539 00000000:0000 0A 00000000:00000000 00:00000000 00000000   108        0 12345 1 0000000000000000 100 0 0 10 0
   1: 00000000:0050 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 67890 1 0000000000000000 100 0 0 10 0
   2: 0100007F:1F90 AC10000A:D904 01 00000000:00000000 00:00000000 00000000  1000        0 11111 1 0000000000000000 100 0 0 10 0
";

    const SAMPLE_TCP6: &str = "\
  sl  local_address                         remote_address                        st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 00000000000000000000000000000000:1F90 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 22222 1 0000000000000000 100 0 0 10 0
   1: 00000000000000000000000001000000:0539 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000   108        0 33333 1 0000000000000000 100 0 0 10 0
";

    #[test]
    fn parse_tcp_listen_entries() {
        let entries = parse_proc_net_tcp(SAMPLE_TCP, Protocol::Tcp);
        assert_eq!(entries.len(), 2);

        assert_eq!(entries[0].local_addr, "127.0.0.1");
        assert_eq!(entries[0].port, 1337);
        assert_eq!(entries[0].uid, 108);
        assert_eq!(entries[0].inode, 12345);

        assert_eq!(entries[1].local_addr, "0.0.0.0");
        assert_eq!(entries[1].port, 80);
        assert_eq!(entries[1].uid, 0);
        assert_eq!(entries[1].inode, 67890);
    }

    #[test]
    fn parse_tcp6_listen_entries() {
        let entries = parse_proc_net_tcp(SAMPLE_TCP6, Protocol::Tcp6);
        assert_eq!(entries.len(), 2);

        assert_eq!(entries[0].local_addr, "::");
        assert_eq!(entries[0].port, 8080);
        assert_eq!(entries[0].uid, 1000);
        assert_eq!(entries[0].inode, 22222);

        assert_eq!(entries[1].local_addr, "::1");
        assert_eq!(entries[1].port, 1337);
    }

    #[test]
    fn parse_ipv4_mapped_ipv6() {
        // ::ffff:127.0.0.1 in /proc/net/tcp6 format
        let hex = "0000000000000000FFFF00000100007F";
        let addr = parse_ipv6_addr(hex).unwrap();
        assert_eq!(addr, "::ffff:127.0.0.1");
    }

    #[test]
    fn dedup_keeps_different_bind_addresses() {
        let entries = vec![
            TcpEntry {
                protocol: Protocol::Tcp,
                local_addr: "0.0.0.0".to_string(),
                port: 8080,
                uid: 1000,
                inode: 111,
            },
            TcpEntry {
                protocol: Protocol::Tcp6,
                local_addr: "::".to_string(),
                port: 8080,
                uid: 1000,
                inode: 222,
            },
        ];
        let deduped = dedup_entries(entries);
        assert_eq!(deduped.len(), 2);
    }

    #[test]
    fn dedup_removes_ipv4_mapped_duplicate() {
        let entries = vec![
            TcpEntry {
                protocol: Protocol::Tcp,
                local_addr: "127.0.0.1".to_string(),
                port: 1337,
                uid: 108,
                inode: 111,
            },
            TcpEntry {
                protocol: Protocol::Tcp6,
                local_addr: "::ffff:127.0.0.1".to_string(),
                port: 1337,
                uid: 108,
                inode: 222,
            },
        ];
        let deduped = dedup_entries(entries);
        assert_eq!(deduped.len(), 1);
    }

    #[test]
    fn normalize_addr_strips_ipv4_mapped() {
        assert_eq!(normalize_addr("::ffff:192.168.1.1"), "192.168.1.1");
        assert_eq!(normalize_addr("127.0.0.1"), "127.0.0.1");
        assert_eq!(normalize_addr("::"), "::");
    }

    #[test]
    fn parse_empty_content() {
        let entries = parse_proc_net_tcp("", Protocol::Tcp);
        assert!(entries.is_empty());
    }

    #[test]
    fn parse_header_only() {
        let content = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n";
        let entries = parse_proc_net_tcp(content, Protocol::Tcp);
        assert!(entries.is_empty());
    }

    #[test]
    fn ipv6_loopback_encoding() {
        let hex = "00000000000000000000000001000000";
        let addr = parse_ipv6_addr(hex).unwrap();
        assert_eq!(addr, "::1");
    }

    #[test]
    fn ipv6_all_zeros() {
        let hex = "00000000000000000000000000000000";
        let addr = parse_ipv6_addr(hex).unwrap();
        assert_eq!(addr, "::");
    }

    #[test]
    fn parse_address_ipv4() {
        let (addr, port) = parse_address("0100007F:0539", Protocol::Tcp).unwrap();
        assert_eq!(addr, "127.0.0.1");
        assert_eq!(port, 1337);
    }

    #[test]
    fn parse_address_ipv4_all_zeros() {
        let (addr, port) = parse_address("00000000:0050", Protocol::Tcp).unwrap();
        assert_eq!(addr, "0.0.0.0");
        assert_eq!(port, 80);
    }
}
