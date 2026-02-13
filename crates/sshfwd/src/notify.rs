use std::collections::{HashMap, HashSet};
use std::time::Instant;

use sshfwd_common::types::ListeningPort;

use crate::forward::{ForwardEntry, ForwardStatus};

pub struct PortChange {
    pub port: u16,
    pub kind: PortChangeKind,
    pub process_name: Option<String>,
}

pub enum PortChangeKind {
    Appeared,
    Disappeared,
    Reactivated,
}

/// Detect port changes between two scans for notification purposes.
/// Returns an empty vec on the first scan (no baseline).
pub fn detect_port_changes(
    prev_scan_ports: Option<&HashSet<u16>>,
    new_scan_ports: &HashSet<u16>,
    forwards: &HashMap<u16, ForwardEntry>,
    new_ports: &[ListeningPort],
    old_ports: &[ListeningPort],
) -> Vec<PortChange> {
    let prev = match prev_scan_ports {
        Some(prev) => prev,
        None => return Vec::new(),
    };

    let mut changes = Vec::new();

    // Appeared or reactivated ports (sorted for deterministic notification order)
    let mut appeared: Vec<u16> = new_scan_ports.difference(prev).copied().collect();
    appeared.sort();
    for port in appeared {
        let kind = if forwards
            .get(&port)
            .is_some_and(|e| e.status == ForwardStatus::Starting)
        {
            PortChangeKind::Reactivated
        } else {
            PortChangeKind::Appeared
        };
        let process_name = new_ports
            .iter()
            .find(|p| p.port == port)
            .and_then(|p| p.process.as_ref())
            .map(|p| p.cmdline.clone());
        changes.push(PortChange {
            port,
            kind,
            process_name,
        });
    }

    // Disappeared ports (sorted for deterministic notification order)
    let mut disappeared: Vec<u16> = prev.difference(new_scan_ports).copied().collect();
    disappeared.sort();
    for port in disappeared {
        let process_name = old_ports
            .iter()
            .find(|p| p.port == port)
            .and_then(|p| p.process.as_ref())
            .map(|p| p.cmdline.clone());
        changes.push(PortChange {
            port,
            kind: PortChangeKind::Disappeared,
            process_name,
        });
    }

    changes
}

/// Batches port change notifications across scans, flushing after a quiet period.
const NOTIFY_DEBOUNCE_SECS: u64 = 2;

pub struct NotifyBatch {
    pending: Vec<PortChange>,
    last_change_at: Option<Instant>,
}

impl NotifyBatch {
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
            last_change_at: None,
        }
    }

    /// Add new changes to the pending batch.
    pub fn extend(&mut self, changes: Vec<PortChange>) {
        if !changes.is_empty() {
            self.pending.extend(changes);
            self.last_change_at = Some(Instant::now());
        }
    }

    /// Flush pending changes if the debounce window has elapsed.
    /// Returns true if a notification was sent.
    pub fn flush_if_ready(&mut self, destination: &str) -> bool {
        let ready = match self.last_change_at {
            Some(t) => t.elapsed().as_secs() >= NOTIFY_DEBOUNCE_SECS,
            None => false,
        };
        if ready && !self.pending.is_empty() {
            let changes = std::mem::take(&mut self.pending);
            self.last_change_at = None;
            notify_port_changes(destination, &changes);
            return true;
        }
        false
    }
}

fn notify_port_changes(destination: &str, changes: &[PortChange]) {
    if changes.is_empty() {
        return;
    }

    let summary = format!("sshfwd - {destination}");
    let body = format_notification_body(changes);

    // Fire-and-forget to avoid blocking the main loop
    std::thread::spawn(move || {
        #[cfg(target_os = "macos")]
        let _ = notify_rust::set_application("com.apple.Terminal");

        let mut n = notify_rust::Notification::new();
        n.summary(&summary)
            .body(&body)
            .timeout(notify_rust::Timeout::Milliseconds(5000));

        #[cfg(not(target_os = "macos"))]
        n.icon("utilities-terminal");

        let _ = n.show();
    });
}

/// Format changes grouped by kind into compact lines.
///
/// Single change:  `+ 8080 (node)`
/// Multiple:       `+ 80, 443 · - 3000 (app) · ~ 5432`
fn format_notification_body(changes: &[PortChange]) -> String {
    let mut appeared = Vec::new();
    let mut disappeared = Vec::new();
    let mut reactivated = Vec::new();

    for c in changes {
        let entry = match &c.process_name {
            Some(name) => format!("{} ({})", c.port, name),
            None => format!("{}", c.port),
        };
        match c.kind {
            PortChangeKind::Appeared => appeared.push(entry),
            PortChangeKind::Disappeared => disappeared.push(entry),
            PortChangeKind::Reactivated => reactivated.push(entry),
        }
    }

    let mut groups = Vec::new();
    if !appeared.is_empty() {
        groups.push(format!("+ {}", appeared.join(", ")));
    }
    if !reactivated.is_empty() {
        groups.push(format!("~ {}", reactivated.join(", ")));
    }
    if !disappeared.is_empty() {
        groups.push(format!("- {}", disappeared.join(", ")));
    }

    groups.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use sshfwd_common::types::{ListeningPort, ProcessInfo, Protocol};

    fn make_port(port: u16, name: &str) -> ListeningPort {
        ListeningPort {
            protocol: Protocol::Tcp,
            local_addr: "0.0.0.0".to_string(),
            port,
            process: Some(ProcessInfo {
                pid: port as u32,
                name: name.to_string(),
                cmdline: name.to_string(),
                uid: 1000,
            }),
        }
    }

    #[test]
    fn first_scan_returns_empty() {
        let new: HashSet<u16> = [80, 443].into();
        let forwards = HashMap::new();
        let ports = vec![make_port(80, "nginx"), make_port(443, "nginx")];

        let changes = detect_port_changes(None, &new, &forwards, &ports, &[]);
        assert!(changes.is_empty());
    }

    #[test]
    fn detects_appeared_ports() {
        let prev: HashSet<u16> = [80].into();
        let new: HashSet<u16> = [80, 443, 8080].into();
        let forwards = HashMap::new();
        let new_ports = vec![
            make_port(80, "nginx"),
            make_port(443, "nginx"),
            make_port(8080, "node"),
        ];

        let changes = detect_port_changes(Some(&prev), &new, &forwards, &new_ports, &[]);
        assert_eq!(changes.len(), 2);
        // Should be sorted by port
        assert_eq!(changes[0].port, 443);
        assert!(matches!(changes[0].kind, PortChangeKind::Appeared));
        assert_eq!(changes[1].port, 8080);
        assert!(matches!(changes[1].kind, PortChangeKind::Appeared));
    }

    #[test]
    fn detects_disappeared_ports() {
        let prev: HashSet<u16> = [80, 443, 8080].into();
        let new: HashSet<u16> = [80].into();
        let forwards = HashMap::new();
        let old_ports = vec![
            make_port(80, "nginx"),
            make_port(443, "nginx"),
            make_port(8080, "node"),
        ];

        let changes = detect_port_changes(Some(&prev), &new, &forwards, &[], &old_ports);
        assert_eq!(changes.len(), 2);
        // Should be sorted by port
        assert_eq!(changes[0].port, 443);
        assert!(matches!(changes[0].kind, PortChangeKind::Disappeared));
        assert_eq!(changes[1].port, 8080);
        assert!(matches!(changes[1].kind, PortChangeKind::Disappeared));
    }

    #[test]
    fn detects_reactivated_forward() {
        let prev: HashSet<u16> = [80].into();
        let new: HashSet<u16> = [80, 5432].into();
        let mut forwards = HashMap::new();
        forwards.insert(
            5432,
            ForwardEntry {
                local_port: 5432,
                status: ForwardStatus::Starting,
                active_connections: 0,
            },
        );
        let new_ports = vec![make_port(80, "nginx"), make_port(5432, "postgres")];

        let changes = detect_port_changes(Some(&prev), &new, &forwards, &new_ports, &[]);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].port, 5432);
        assert!(matches!(changes[0].kind, PortChangeKind::Reactivated));
        assert_eq!(changes[0].process_name.as_deref(), Some("postgres"));
    }

    #[test]
    fn no_changes_when_same() {
        let prev: HashSet<u16> = [80, 443].into();
        let new: HashSet<u16> = [80, 443].into();
        let forwards = HashMap::new();

        let changes = detect_port_changes(Some(&prev), &new, &forwards, &[], &[]);
        assert!(changes.is_empty());
    }

    #[test]
    fn format_single_change() {
        let changes = vec![PortChange {
            port: 8080,
            kind: PortChangeKind::Appeared,
            process_name: Some("node".to_string()),
        }];
        assert_eq!(format_notification_body(&changes), "+ 8080 (node)");
    }

    #[test]
    fn format_grouped_changes() {
        let changes = vec![
            PortChange {
                port: 80,
                kind: PortChangeKind::Appeared,
                process_name: None,
            },
            PortChange {
                port: 443,
                kind: PortChangeKind::Appeared,
                process_name: Some("nginx".to_string()),
            },
            PortChange {
                port: 3000,
                kind: PortChangeKind::Disappeared,
                process_name: Some("app".to_string()),
            },
        ];
        assert_eq!(
            format_notification_body(&changes),
            "+ 80, 443 (nginx) · - 3000 (app)"
        );
    }
}
