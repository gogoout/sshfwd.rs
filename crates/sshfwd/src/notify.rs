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
