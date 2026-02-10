use std::time::Instant;

use sshfwd_common::types::{Protocol, ScanResult};

use crate::error::DiscoveryError;

const STALENESS_THRESHOLD_SECS: u64 = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    Connecting,
    Connected,
    Disconnected,
}

#[allow(dead_code)]
pub enum Message {
    // Discovery events
    ScanReceived(ScanResult),
    DiscoveryWarning(String),
    DiscoveryError(DiscoveryError),
    StreamEnded,
    // Keyboard
    MoveDown,
    MoveUp,
    GoToTop,
    GoToBottom,
    Quit,
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
        }
    }
}

pub fn update(model: &mut Model, msg: Message) {
    match msg {
        Message::ScanReceived(scan) => {
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

            if ports != model.ports {
                model.ports = ports;
                // Clamp selection
                if !model.ports.is_empty() && model.selected_index >= model.ports.len() {
                    model.selected_index = model.ports.len() - 1;
                }
                model.needs_render = true;
            } else if was_connecting {
                model.needs_render = true; // render green dot on first scan
            }
        }
        Message::DiscoveryWarning(_) => {
            // Warnings don't change visible state for now
        }
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
        Message::MoveDown => {
            if !model.ports.is_empty() && model.selected_index < model.ports.len() - 1 {
                model.selected_index += 1;
                model.needs_render = true;
            }
        }
        Message::MoveUp => {
            if model.selected_index > 0 {
                model.selected_index -= 1;
                model.needs_render = true;
            }
        }
        Message::GoToTop => {
            if model.selected_index != 0 {
                model.selected_index = 0;
                model.needs_render = true;
            }
        }
        Message::GoToBottom => {
            if !model.ports.is_empty() {
                let last = model.ports.len() - 1;
                if model.selected_index != last {
                    model.selected_index = last;
                    model.needs_render = true;
                }
            }
        }
        Message::Quit => {
            model.running = false;
        }
        Message::Tick => {
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
}

pub fn view(model: &Model, frame: &mut ratatui::Frame) {
    let areas = crate::ui::layout_areas(frame.area());
    crate::ui::table::render(model, frame, areas.table);
    crate::ui::hotkey_bar::render(frame, areas.hotkey_bar);
}
