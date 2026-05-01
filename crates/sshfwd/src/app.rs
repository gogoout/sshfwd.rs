use std::collections::{HashMap, HashSet};
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use sshfwd_common::types::{Protocol, ScanResult};

use crate::error::DiscoveryError;
use crate::forward::{
    ForwardCommand, ForwardEntry, ForwardEvent, ForwardKey, ForwardKind, ForwardStatus,
};
use crate::ui::table::{build_display_rows, DisplayRow};

const STALENESS_THRESHOLD_SECS: u64 = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    Connecting,
    Connected,
    Reconnecting,
    Disconnected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppMode {
    #[default]
    Forward,
    Reverse,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModalState {
    None,
    PortInput {
        kind: ForwardKind,
        remote_port: u16,
        local_port: u16,
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
    // Reconnect lifecycle
    ConnectionLost,
    Reconnecting,
    Reconnected,
    // Local port scan
    LocalScanReceived(ScanResult),
    LocalScanError(String),
    // Keyboard
    Key(KeyEvent),
    // Mouse
    Mouse(crossterm::event::MouseEvent),
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
    pub local_ports: Vec<sshfwd_common::types::ListeningPort>,
    pub scan_index: u64,
    pub selected_index: usize,
    pub connection_state: ConnectionState,
    pub last_scan_at: Option<Instant>,
    pub running: bool,
    pub needs_render: bool,
    pub forwards: HashMap<ForwardKey, ForwardEntry>,
    pub modal: ModalState,
    pub mode: AppMode,
    pub started_at: Instant,
    pub show_inactive_forwards: bool,
    pub notifications_enabled: bool,
    pub prev_scan_ports: Option<HashSet<u16>>,
    pub notify_batch: crate::notify::NotifyBatch,
    pub table_state: ratatui::widgets::TableState,
    pub table_content_area: Option<ratatui::layout::Rect>,
}

impl Model {
    pub fn new(destination: String) -> Self {
        Self {
            destination,
            hostname: None,
            username: None,
            ports: Vec::new(),
            local_ports: Vec::new(),
            scan_index: 0,
            selected_index: 0,
            connection_state: ConnectionState::Connecting,
            last_scan_at: None,
            running: true,
            needs_render: true,
            forwards: HashMap::new(),
            modal: ModalState::None,
            mode: AppMode::Forward,
            started_at: Instant::now(),
            show_inactive_forwards: false,
            notifications_enabled: true,
            prev_scan_ports: None,
            notify_batch: crate::notify::NotifyBatch::default(),
            table_state: ratatui::widgets::TableState::default(),
            table_content_area: None,
        }
    }

    fn selected_port(&self) -> Option<u16> {
        let display_rows = build_display_rows(self);
        match display_rows.get(self.selected_index) {
            Some(DisplayRow::Port(i)) => Some(self.ports[*i].port),
            Some(DisplayRow::LocalPort(i)) => Some(self.local_ports[*i].port),
            Some(DisplayRow::InactiveForward(rp)) => Some(*rp),
            Some(DisplayRow::InactiveReverseForward(rp)) => Some(*rp),
            _ => None,
        }
    }

    fn remote_host(&self) -> String {
        self.hostname
            .clone()
            .unwrap_or_else(|| self.destination.clone())
    }
}

/// Move selection down by one, skipping separator rows.
fn move_selection_down(model: &mut Model) {
    let display_rows = build_display_rows(model);
    let last = display_rows.len().saturating_sub(1);
    if model.selected_index < last {
        let next = model.selected_index + 1;
        model.selected_index = if matches!(display_rows.get(next), Some(DisplayRow::Separator)) {
            (next + 1).min(last)
        } else {
            next
        };
        model.needs_render = true;
    }
}

/// Move selection up by one, skipping separator rows.
fn move_selection_up(model: &mut Model) {
    let display_rows = build_display_rows(model);
    if model.selected_index > 0 {
        let prev = model.selected_index - 1;
        model.selected_index = if matches!(display_rows.get(prev), Some(DisplayRow::Separator)) {
            prev.saturating_sub(1)
        } else {
            prev
        };
        model.needs_render = true;
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
        if let Some(pos) = display_rows.iter().position(|dr| match dr {
            DisplayRow::Port(i) => model.ports[*i].port == port,
            DisplayRow::LocalPort(i) => model.local_ports[*i].port == port,
            DisplayRow::InactiveForward(rp) => *rp == port,
            DisplayRow::InactiveReverseForward(rp) => *rp == port,
            DisplayRow::Separator => false,
        }) {
            model.selected_index = pos;
            return;
        }
    }
    // Clamp to last selectable row
    let last_selectable = display_rows
        .iter()
        .rposition(|dr| dr.is_selectable())
        .unwrap_or(0);
    if model.selected_index > last_selectable {
        model.selected_index = last_selectable;
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
            let current_remote_ports: HashSet<u16> = ports.iter().map(|p| p.port).collect();
            let remote_host = model.remote_host();
            commands = crate::forward::reconcile_forwards(
                &mut model.forwards,
                &current_remote_ports,
                &remote_host,
            );

            // Detect port changes for notifications
            let port_changes = crate::notify::detect_port_changes(
                model.prev_scan_ports.as_ref(),
                &current_remote_ports,
                &model.forwards,
                &ports,
                &model.ports,
            );
            model.prev_scan_ports = Some(current_remote_ports);

            if ports != model.ports {
                model.ports = ports;
                adjust_selection(model, prev_selected);
                model.needs_render = true;
            } else if was_connecting || !commands.is_empty() {
                model.needs_render = true;
            }

            if model.notifications_enabled {
                model.notify_batch.extend(port_changes);
            }
        }
        Message::DiscoveryWarning(_) => {}
        Message::DiscoveryError(_) | Message::StreamEnded => {
            // Reconnect loop handles recovery — do not exit.
        }
        Message::ConnectionLost => {
            model.connection_state = ConnectionState::Reconnecting;
            for entry in model.forwards.values_mut() {
                entry.status = ForwardStatus::Paused;
            }
            model.needs_render = true;
        }
        Message::Reconnecting => {
            model.needs_render = true;
        }
        Message::Reconnected => {
            model.connection_state = ConnectionState::Connecting;
            model.needs_render = true;
            // Reactivate reverse forwards immediately — local forwards reactivate via
            // reconcile_forwards on the next ScanReceived.
            commands = model
                .forwards
                .iter()
                .filter(|(k, _)| k.kind == ForwardKind::Reverse)
                .map(|(k, e)| ForwardCommand::Reactivate {
                    kind: ForwardKind::Reverse,
                    remote_port: k.remote_port,
                    local_port: e.local_port,
                    remote_host: "127.0.0.1".to_string(),
                })
                .collect();
        }
        Message::LocalScanReceived(scan) => {
            let prev_selected = model.selected_port();
            model.local_ports = scan.ports;
            adjust_selection(model, prev_selected);
            model.needs_render = true;
        }
        Message::LocalScanError(_) => {
            // Silently ignore local scan errors for now
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
                    kind,
                    remote_port,
                    local_port,
                } => {
                    if let Some(entry) = model.forwards.get_mut(&ForwardKey { kind, remote_port }) {
                        entry.local_port = local_port;
                        entry.status = ForwardStatus::Active;
                    }
                    save_forwards(model);
                }
                ForwardEvent::Stopped { kind, remote_port } => {
                    let local_port = model
                        .forwards
                        .get(&ForwardKey { kind, remote_port })
                        .map(|e| e.local_port);
                    model.forwards.remove(&ForwardKey { kind, remote_port });
                    save_forwards(model);
                    let hint = if kind == ForwardKind::Reverse {
                        local_port.unwrap_or(remote_port)
                    } else {
                        remote_port
                    };
                    adjust_selection(model, Some(hint));
                }
                ForwardEvent::Paused { kind, remote_port } => {
                    if let Some(entry) = model.forwards.get_mut(&ForwardKey { kind, remote_port }) {
                        entry.status = ForwardStatus::Paused;
                    }
                }
                ForwardEvent::BindError {
                    kind,
                    remote_port,
                    message,
                } => {
                    let failed_local_port = model
                        .forwards
                        .get(&ForwardKey { kind, remote_port })
                        .map(|e| e.local_port)
                        .unwrap_or(remote_port);
                    model.forwards.remove(&ForwardKey { kind, remote_port });
                    model.modal = ModalState::PortInput {
                        kind,
                        remote_port,
                        local_port: failed_local_port,
                        buffer: failed_local_port.to_string(),
                        remote_host: if kind == ForwardKind::Reverse {
                            "127.0.0.1".to_string()
                        } else {
                            model.remote_host()
                        },
                        error: Some(message),
                    };
                    let hint = if kind == ForwardKind::Reverse {
                        failed_local_port
                    } else {
                        remote_port
                    };
                    adjust_selection(model, Some(hint));
                }
                ForwardEvent::ConnectionCountChanged {
                    kind,
                    remote_port,
                    count,
                } => {
                    if let Some(entry) = model.forwards.get_mut(&ForwardKey { kind, remote_port }) {
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
                    // Mark stale-connected as disconnected; reconnect loop will
                    // send ConnectionLost / Reconnecting separately.
                    model.connection_state = ConnectionState::Disconnected;
                    model.needs_render = true;
                }
            }
            // Flush batched notifications after debounce window
            model.notify_batch.flush_if_ready(&model.destination);
        }
        Message::Resize(_, _) => {
            model.needs_render = true;
        }
        Message::Mouse(mouse) => {
            if model.modal == ModalState::None {
                use crossterm::event::{MouseButton, MouseEventKind};
                match mouse.kind {
                    MouseEventKind::ScrollUp => {
                        move_selection_up(model);
                    }
                    MouseEventKind::ScrollDown => {
                        move_selection_down(model);
                    }
                    MouseEventKind::Down(MouseButton::Left) => {
                        if let Some(content) = model.table_content_area {
                            if mouse.row >= content.y && mouse.row < content.y + content.height {
                                let visual_row = (mouse.row - content.y) as usize;
                                let display_idx = model.table_state.offset() + visual_row;
                                let display_rows = build_display_rows(model);
                                if display_idx < display_rows.len()
                                    && !matches!(display_rows[display_idx], DisplayRow::Separator)
                                {
                                    model.selected_index = display_idx;
                                    model.needs_render = true;
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
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
            move_selection_down(model);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            move_selection_up(model);
        }
        KeyCode::Char('g') if model.selected_index != 0 => {
            model.selected_index = 0;
            model.needs_render = true;
        }
        KeyCode::Char('G') => {
            let display_rows = build_display_rows(model);
            if let Some(last) = display_rows.iter().rposition(|dr| dr.is_selectable()) {
                if model.selected_index != last {
                    model.selected_index = last;
                    model.needs_render = true;
                }
            }
        }
        KeyCode::Char('m') => {
            model.mode = match model.mode {
                AppMode::Forward => AppMode::Reverse,
                AppMode::Reverse => AppMode::Forward,
            };
            model.selected_index = 0;
            model.needs_render = true;
        }
        KeyCode::Char('p') => {
            let selected_port = model.selected_port();
            model.show_inactive_forwards = !model.show_inactive_forwards;
            adjust_selection(model, selected_port);
            model.needs_render = true;
        }
        KeyCode::Enter
            if key.modifiers.contains(KeyModifiers::SHIFT) && model.mode == AppMode::Forward =>
        {
            open_local_forward_modal(model);
        }
        KeyCode::Enter | KeyCode::Char('f') => match model.mode {
            AppMode::Forward => {
                commands = handle_forward_action(model);
            }
            AppMode::Reverse => {
                commands = handle_reverse_action(model);
            }
        },
        KeyCode::Char('F') if model.mode == AppMode::Forward => {
            open_local_forward_modal(model);
        }
        _ => {}
    }

    commands
}

fn handle_forward_action(model: &mut Model) -> Vec<ForwardCommand> {
    let mut commands = Vec::new();
    if let Some(remote_port) = model.selected_port() {
        let key = ForwardKey::local(remote_port);
        if let std::collections::hash_map::Entry::Vacant(e) = model.forwards.entry(key) {
            e.insert(ForwardEntry {
                local_port: remote_port,
                status: ForwardStatus::Starting,
                active_connections: 0,
            });
            commands.push(ForwardCommand::Start {
                kind: ForwardKind::Local,
                remote_port,
                local_port: remote_port,
                remote_host: model.remote_host(),
            });
            adjust_selection(model, Some(remote_port));
            model.needs_render = true;
        } else {
            commands.push(ForwardCommand::Stop {
                kind: ForwardKind::Local,
                remote_port,
            });
        }
    }
    commands
}

fn open_local_forward_modal(model: &mut Model) {
    if let Some(remote_port) = model.selected_port() {
        if !model.forwards.contains_key(&ForwardKey::local(remote_port)) {
            model.modal = ModalState::PortInput {
                kind: ForwardKind::Local,
                remote_port,
                local_port: remote_port,
                buffer: remote_port.to_string(),
                remote_host: model.remote_host(),
                error: None,
            };
            model.needs_render = true;
        }
    }
}

fn handle_reverse_action(model: &mut Model) -> Vec<ForwardCommand> {
    let mut commands = Vec::new();
    let display_rows = build_display_rows(model);
    match display_rows.get(model.selected_index) {
        Some(DisplayRow::InactiveReverseForward(rp)) => {
            // Pressing Enter on an inactive reverse forward stops it (removes persistence)
            let remote_port = *rp;
            commands.push(ForwardCommand::Stop {
                kind: ForwardKind::Reverse,
                remote_port,
            });
        }
        Some(DisplayRow::LocalPort(i)) => {
            let local_port = model.local_ports[*i].port;
            // Reverse forwards are keyed by remote bind port; search by entry.local_port.
            let existing_key = model
                .forwards
                .iter()
                .find(|(k, e)| k.kind == ForwardKind::Reverse && e.local_port == local_port)
                .map(|(k, _)| *k);
            if let Some(key) = existing_key {
                // Active or Starting — toggle it off.
                commands.push(ForwardCommand::Stop {
                    kind: ForwardKind::Reverse,
                    remote_port: key.remote_port,
                });
            } else {
                model.modal = ModalState::PortInput {
                    kind: ForwardKind::Reverse,
                    remote_port: local_port,
                    local_port,
                    buffer: local_port.to_string(),
                    remote_host: "127.0.0.1".to_string(),
                    error: None,
                };
                model.needs_render = true;
            }
        }
        _ => {}
    }
    commands
}

fn handle_port_input_key(model: &mut Model, key: KeyEvent) -> Vec<ForwardCommand> {
    let mut commands = Vec::new();

    let (kind, remote_port, local_port, remote_host, buffer) = match &model.modal {
        ModalState::PortInput {
            kind,
            remote_port,
            local_port,
            remote_host,
            buffer,
            ..
        } => (
            *kind,
            *remote_port,
            *local_port,
            remote_host.clone(),
            buffer.clone(),
        ),
        ModalState::None => return commands,
    };

    match key.code {
        KeyCode::Esc => {
            model.modal = ModalState::None;
            model.needs_render = true;
        }
        KeyCode::Enter => {
            if let Ok(parsed_port) = buffer.parse::<u16>() {
                if parsed_port > 0 {
                    match kind {
                        ForwardKind::Local => {
                            let fwd_key = ForwardKey::local(remote_port);
                            if model.forwards.contains_key(&fwd_key) {
                                commands.push(ForwardCommand::Stop {
                                    kind: ForwardKind::Local,
                                    remote_port,
                                });
                            }
                            model.forwards.insert(
                                fwd_key,
                                ForwardEntry {
                                    local_port: parsed_port,
                                    status: ForwardStatus::Starting,
                                    active_connections: 0,
                                },
                            );
                            commands.push(ForwardCommand::Start {
                                kind: ForwardKind::Local,
                                remote_port,
                                local_port: parsed_port,
                                remote_host,
                            });
                        }
                        ForwardKind::Reverse => {
                            // buffer holds the remote bind port; local_port is fixed
                            let fwd_key = ForwardKey::reverse(parsed_port);
                            if model.forwards.contains_key(&fwd_key) {
                                commands.push(ForwardCommand::Stop {
                                    kind: ForwardKind::Reverse,
                                    remote_port: parsed_port,
                                });
                            }
                            model.forwards.insert(
                                fwd_key,
                                ForwardEntry {
                                    local_port,
                                    status: ForwardStatus::Starting,
                                    active_connections: 0,
                                },
                            );
                            commands.push(ForwardCommand::Start {
                                kind: ForwardKind::Reverse,
                                remote_port: parsed_port,
                                local_port,
                                remote_host,
                            });
                        }
                    }
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
                    kind,
                    remote_port,
                    local_port,
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
                kind,
                remote_port,
                local_port,
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
        .map(|(key, entry)| PersistedForward {
            remote_port: key.remote_port,
            local_port: entry.local_port,
            kind: key.kind,
        })
        .collect();

    persistence::save_forwards(&model.destination, &forwards);
}

pub fn view(model: &mut Model, frame: &mut ratatui::Frame) {
    let areas = crate::ui::layout_areas(frame.area());
    crate::ui::table::render(model, frame, areas.table);
    crate::ui::hotkey_bar::render(model, frame, areas.hotkey_bar);
    if model.modal != ModalState::None {
        crate::ui::modal::render(model, frame);
    }
}
