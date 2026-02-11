use std::collections::HashMap;
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use sshfwd_common::types::{Protocol, ScanResult};

use crate::error::DiscoveryError;
use crate::forward::{ForwardCommand, ForwardEntry, ForwardEvent, ForwardStatus};
use crate::ui::table::{build_display_rows, DisplayRow};

const STALENESS_THRESHOLD_SECS: u64 = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    Connecting,
    Connected,
    Disconnected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModalState {
    None,
    PortInput {
        remote_port: u16,
        buffer: String,
        remote_host: String,
        error: Option<String>,
    },
}

#[allow(dead_code)]
pub enum Message {
    // Discovery events
    ScanReceived(ScanResult),
    DiscoveryWarning(String),
    DiscoveryError(DiscoveryError),
    StreamEnded,
    // Keyboard
    Key(KeyEvent),
    // Forwarding
    ForwardEvent(ForwardEvent),
    // Timer
    Tick,
    Resize(u16, u16),
}

pub struct Model {
    pub destination: String,
    pub hostname: Option<String>,
    pub username: Option<String>,
    pub ports: Vec<sshfwd_common::types::ListeningPort>,
    pub scan_index: u64,
    pub selected_index: usize,
    pub connection_state: ConnectionState,
    pub last_scan_at: Option<Instant>,
    pub running: bool,
    pub needs_render: bool,
    pub forwards: HashMap<u16, ForwardEntry>,
    pub modal: ModalState,
    pub started_at: Instant,
}

impl Model {
    pub fn new(destination: String) -> Self {
        Self {
            destination,
            hostname: None,
            username: None,
            ports: Vec::new(),
            scan_index: 0,
            selected_index: 0,
            connection_state: ConnectionState::Connecting,
            last_scan_at: None,
            running: true,
            needs_render: true,
            forwards: HashMap::new(),
            modal: ModalState::None,
            started_at: Instant::now(),
        }
    }

    fn selected_port(&self) -> Option<u16> {
        let display_rows = build_display_rows(self);
        match display_rows.get(self.selected_index) {
            Some(DisplayRow::Port(i)) => Some(self.ports[*i].port),
            _ => None,
        }
    }

    fn remote_host(&self) -> String {
        self.hostname
            .clone()
            .unwrap_or_else(|| self.destination.clone())
    }
}

/// Recompute selected_index after display rows change (forward added/removed, scan update).
/// Tries to keep `target_port` selected; falls back to clamping.
fn adjust_selection(model: &mut Model, target_port: Option<u16>) {
    let display_rows = build_display_rows(model);
    if display_rows.is_empty() {
        model.selected_index = 0;
        return;
    }
    if let Some(port) = target_port {
        if let Some(pos) = display_rows
            .iter()
            .position(|dr| matches!(dr, DisplayRow::Port(i) if model.ports[*i].port == port))
        {
            model.selected_index = pos;
            return;
        }
    }
    // Clamp to last port row
    let last_port = display_rows
        .iter()
        .rposition(|dr| matches!(dr, DisplayRow::Port(_)))
        .unwrap_or(0);
    if model.selected_index > last_port {
        model.selected_index = last_port;
    }
    if matches!(
        display_rows.get(model.selected_index),
        Some(DisplayRow::Separator)
    ) {
        model.selected_index = model.selected_index.saturating_sub(1);
    }
}

pub fn update(model: &mut Model, msg: Message) -> Vec<ForwardCommand> {
    let mut commands = Vec::new();

    match msg {
        Message::ScanReceived(scan) => {
            // Remember selected port before any state changes
            let prev_selected = model.selected_port();

            model.hostname = Some(scan.hostname);
            model.username = Some(scan.username);
            model.scan_index = scan.scan_index;
            model.last_scan_at = Some(Instant::now());

            let was_connecting = model.connection_state == ConnectionState::Connecting;
            model.connection_state = ConnectionState::Connected;

            let mut ports = scan.ports;
            ports.sort_by(|a, b| {
                a.port
                    .cmp(&b.port)
                    .then_with(|| {
                        let pid_a = a.process.as_ref().map_or(0, |p| p.pid);
                        let pid_b = b.process.as_ref().map_or(0, |p| p.pid);
                        pid_a.cmp(&pid_b)
                    })
                    .then_with(|| {
                        let proto_ord = |p: &Protocol| match p {
                            Protocol::Tcp => 0u8,
                            Protocol::Tcp6 => 1,
                        };
                        proto_ord(&a.protocol).cmp(&proto_ord(&b.protocol))
                    })
            });

            // Reconcile forwards with current scan
            let current_remote_ports: std::collections::HashSet<u16> =
                ports.iter().map(|p| p.port).collect();

            for (&remote_port, entry) in &model.forwards {
                match entry.status {
                    ForwardStatus::Active | ForwardStatus::Starting => {
                        if !current_remote_ports.contains(&remote_port) {
                            commands.push(ForwardCommand::Pause { remote_port });
                        }
                    }
                    ForwardStatus::Paused => {
                        if current_remote_ports.contains(&remote_port) {
                            commands.push(ForwardCommand::Reactivate {
                                remote_port,
                                local_port: entry.local_port,
                                remote_host: model.remote_host(),
                            });
                        }
                    }
                }
            }

            // Update status for commands we just issued
            for cmd in &commands {
                match cmd {
                    ForwardCommand::Pause { remote_port } => {
                        if let Some(entry) = model.forwards.get_mut(remote_port) {
                            entry.status = ForwardStatus::Paused;
                        }
                    }
                    ForwardCommand::Reactivate { remote_port, .. } => {
                        if let Some(entry) = model.forwards.get_mut(remote_port) {
                            entry.status = ForwardStatus::Starting;
                        }
                    }
                    _ => {}
                }
            }

            if ports != model.ports {
                model.ports = ports;
                adjust_selection(model, prev_selected);
                model.needs_render = true;
            } else if was_connecting || !commands.is_empty() {
                model.needs_render = true;
            }
        }
        Message::DiscoveryWarning(_) => {}
        Message::DiscoveryError(_) => {
            model.connection_state = ConnectionState::Disconnected;
            model.running = false;
            model.needs_render = true;
        }
        Message::StreamEnded => {
            model.connection_state = ConnectionState::Disconnected;
            model.running = false;
            model.needs_render = true;
        }
        Message::Key(key) => match &model.modal {
            ModalState::None => {
                commands = handle_normal_key(model, key);
            }
            ModalState::PortInput { .. } => {
                commands = handle_port_input_key(model, key);
            }
        },
        Message::ForwardEvent(evt) => {
            match evt {
                ForwardEvent::Started {
                    remote_port,
                    local_port,
                } => {
                    if let Some(entry) = model.forwards.get_mut(&remote_port) {
                        entry.local_port = local_port;
                        entry.status = ForwardStatus::Active;
                    }
                    save_forwards(model);
                }
                ForwardEvent::Stopped { remote_port } => {
                    model.forwards.remove(&remote_port);
                    save_forwards(model);
                    adjust_selection(model, Some(remote_port));
                }
                ForwardEvent::Paused { remote_port } => {
                    if let Some(entry) = model.forwards.get_mut(&remote_port) {
                        entry.status = ForwardStatus::Paused;
                    }
                }
                ForwardEvent::BindError {
                    remote_port,
                    message,
                } => {
                    let local_port_str = model
                        .forwards
                        .get(&remote_port)
                        .map(|e| format!("{}", e.local_port))
                        .unwrap_or_else(|| format!("{}", remote_port));
                    model.forwards.remove(&remote_port);
                    model.modal = ModalState::PortInput {
                        remote_port,
                        buffer: local_port_str,
                        remote_host: model.remote_host(),
                        error: Some(message),
                    };
                    adjust_selection(model, Some(remote_port));
                }
                ForwardEvent::ConnectionCountChanged { remote_port, count } => {
                    if let Some(entry) = model.forwards.get_mut(&remote_port) {
                        entry.active_connections = count;
                    }
                }
            }
            model.needs_render = true;
        }
        Message::Tick => {
            // Re-render during splash so the transition to table happens on time
            if model.started_at.elapsed().as_secs() < 2 {
                model.needs_render = true;
            }
            if let Some(last) = model.last_scan_at {
                if last.elapsed().as_secs() >= STALENESS_THRESHOLD_SECS
                    && model.connection_state == ConnectionState::Connected
                {
                    model.connection_state = ConnectionState::Disconnected;
                    model.needs_render = true;
                }
            }
        }
        Message::Resize(_, _) => {
            model.needs_render = true;
        }
    }

    commands
}

fn handle_normal_key(model: &mut Model, key: KeyEvent) -> Vec<ForwardCommand> {
    let mut commands = Vec::new();

    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => {
            model.running = false;
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            model.running = false;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            let display_rows = build_display_rows(model);
            let last = display_rows.len().saturating_sub(1);
            if model.selected_index < last {
                let next = model.selected_index + 1;
                model.selected_index =
                    if matches!(display_rows.get(next), Some(DisplayRow::Separator)) {
                        (next + 1).min(last)
                    } else {
                        next
                    };
                model.needs_render = true;
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            let display_rows = build_display_rows(model);
            if model.selected_index > 0 {
                let prev = model.selected_index - 1;
                model.selected_index =
                    if matches!(display_rows.get(prev), Some(DisplayRow::Separator)) {
                        prev.saturating_sub(1)
                    } else {
                        prev
                    };
                model.needs_render = true;
            }
        }
        KeyCode::Char('g') => {
            if model.selected_index != 0 {
                model.selected_index = 0;
                model.needs_render = true;
            }
        }
        KeyCode::Char('G') => {
            let display_rows = build_display_rows(model);
            if let Some(last) = display_rows
                .iter()
                .rposition(|dr| matches!(dr, DisplayRow::Port(_)))
            {
                if model.selected_index != last {
                    model.selected_index = last;
                    model.needs_render = true;
                }
            }
        }
        KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
            if let Some(remote_port) = model.selected_port() {
                if !model.forwards.contains_key(&remote_port) {
                    model.modal = ModalState::PortInput {
                        remote_port,
                        buffer: format!("{}", remote_port),
                        remote_host: model.remote_host(),
                        error: None,
                    };
                    model.needs_render = true;
                }
            }
        }
        KeyCode::Enter | KeyCode::Char('f') => {
            if let Some(remote_port) = model.selected_port() {
                if let std::collections::hash_map::Entry::Vacant(e) =
                    model.forwards.entry(remote_port)
                {
                    e.insert(ForwardEntry {
                        local_port: remote_port,
                        status: ForwardStatus::Starting,
                        active_connections: 0,
                    });
                    commands.push(ForwardCommand::Start {
                        remote_port,
                        local_port: remote_port,
                        remote_host: model.remote_host(),
                    });
                    adjust_selection(model, Some(remote_port));
                    model.needs_render = true;
                } else {
                    commands.push(ForwardCommand::Stop { remote_port });
                }
            }
        }
        KeyCode::Char('F') => {
            if let Some(remote_port) = model.selected_port() {
                if !model.forwards.contains_key(&remote_port) {
                    model.modal = ModalState::PortInput {
                        remote_port,
                        buffer: format!("{}", remote_port),
                        remote_host: model.remote_host(),
                        error: None,
                    };
                    model.needs_render = true;
                }
            }
        }
        _ => {}
    }

    commands
}

fn handle_port_input_key(model: &mut Model, key: KeyEvent) -> Vec<ForwardCommand> {
    let mut commands = Vec::new();

    let (remote_port, remote_host, buffer) = match &model.modal {
        ModalState::PortInput {
            remote_port,
            remote_host,
            buffer,
            ..
        } => (*remote_port, remote_host.clone(), buffer.clone()),
        ModalState::None => return commands,
    };

    match key.code {
        KeyCode::Esc => {
            model.modal = ModalState::None;
            model.needs_render = true;
        }
        KeyCode::Enter => {
            if let Ok(local_port) = buffer.parse::<u16>() {
                if local_port > 0 {
                    // Stop existing forward if any
                    if model.forwards.contains_key(&remote_port) {
                        commands.push(ForwardCommand::Stop { remote_port });
                    }
                    model.forwards.insert(
                        remote_port,
                        ForwardEntry {
                            local_port,
                            status: ForwardStatus::Starting,
                            active_connections: 0,
                        },
                    );
                    commands.push(ForwardCommand::Start {
                        remote_port,
                        local_port,
                        remote_host,
                    });
                }
            }
            model.modal = ModalState::None;
            adjust_selection(model, Some(remote_port));
            model.needs_render = true;
        }
        KeyCode::Char(c) if c.is_ascii_digit() => {
            let mut new_buffer = buffer;
            if new_buffer.len() < 5 {
                new_buffer.push(c);
                model.modal = ModalState::PortInput {
                    remote_port,
                    buffer: new_buffer,
                    remote_host,
                    error: None,
                };
                model.needs_render = true;
            }
        }
        KeyCode::Backspace => {
            let mut new_buffer = buffer;
            new_buffer.pop();
            model.modal = ModalState::PortInput {
                remote_port,
                buffer: new_buffer,
                remote_host,
                error: None,
            };
            model.needs_render = true;
        }
        _ => {}
    }

    commands
}

fn save_forwards(model: &Model) {
    use crate::forward::persistence::{self, PersistedForward};

    let forwards: Vec<PersistedForward> = model
        .forwards
        .iter()
        .filter(|(_, entry)| {
            matches!(
                entry.status,
                ForwardStatus::Active | ForwardStatus::Starting | ForwardStatus::Paused
            )
        })
        .map(|(&remote_port, entry)| PersistedForward {
            remote_port,
            local_port: entry.local_port,
        })
        .collect();

    persistence::save_forwards(&model.destination, &forwards);
}

pub fn view(model: &Model, frame: &mut ratatui::Frame) {
    let areas = crate::ui::layout_areas(frame.area());
    crate::ui::table::render(model, frame, areas.table);
    crate::ui::hotkey_bar::render(model, frame, areas.hotkey_bar);
    if !matches!(model.modal, ModalState::None) {
        crate::ui::modal::render(model, frame);
    }
}
