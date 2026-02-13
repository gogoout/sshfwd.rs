use std::collections::{HashMap, HashSet};

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

    // Appeared or reactivated ports
    for &port in new_scan_ports {
        if !prev.contains(&port) {
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
    }

    // Disappeared ports
    for &port in prev {
        if !new_scan_ports.contains(&port) {
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
    }

    changes
}

pub fn notify_port_changes(destination: &str, changes: &[PortChange]) {
    if changes.is_empty() {
        return;
    }

    let summary = format!("sshfwd - {destination}");
    let body: String = changes
        .iter()
        .map(|c| {
            let symbol = match c.kind {
                PortChangeKind::Appeared => "+",
                PortChangeKind::Disappeared => "-",
                PortChangeKind::Reactivated => "~",
            };
            let verb = match c.kind {
                PortChangeKind::Appeared => "appeared",
                PortChangeKind::Disappeared => "disappeared",
                PortChangeKind::Reactivated => "reactivated",
            };
            match &c.process_name {
                Some(name) => format!("{symbol} Port {} {verb} ({name})", c.port),
                None => format!("{symbol} Port {} {verb}", c.port),
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

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
