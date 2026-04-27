use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Paragraph, Row, Table};
use ratatui::Frame;

use crate::app::{AppMode, ConnectionState, Model};
use crate::forward::{ForwardKey, ForwardKind, ForwardStatus};
use crate::ui::header;

const LOGO: &[&str] = &[
    r"              __    ____             __    ",
    r"   __________/ /_  / __/      ______/ /    ",
    r"  / ___/ ___/ __ \/ /_| | /| / / __  /    ",
    r" (__  |__  ) / / / __/| |/ |/ / /_/ /     ",
    r"/____/____/_/ /_/_/   |__/|__/\__,_/      ",
];

const HEADER_STYLE: Style = Style::new()
    .fg(Color::DarkGray)
    .add_modifier(Modifier::BOLD);
const SELECTED_STYLE: Style = Style::new()
    .fg(Color::White)
    .bg(Color::DarkGray)
    .add_modifier(Modifier::BOLD);

#[derive(Debug, Clone, PartialEq)]
pub enum DisplayRow {
    Port(usize),                 // index into model.ports (Forward mode)
    LocalPort(usize),            // index into model.local_ports (Reverse mode)
    InactiveForward(u16),        // remote port of a paused local forward not in current scan
    InactiveReverseForward(u16), // remote bind port (ForwardKey::remote_port) of a paused reverse forward not in local scan
    Separator,
}

pub fn build_display_rows(model: &Model) -> Vec<DisplayRow> {
    match model.mode {
        AppMode::Forward => build_forward_rows(model),
        AppMode::Reverse => build_reverse_rows(model),
    }
}

fn build_forward_rows(model: &Model) -> Vec<DisplayRow> {
    let scan_ports: std::collections::HashSet<u16> = model.ports.iter().map(|p| p.port).collect();

    let mut forwarded = Vec::new();
    let mut non_forwarded = Vec::new();

    for (i, port) in model.ports.iter().enumerate() {
        if model.forwards.contains_key(&ForwardKey::local(port.port)) {
            forwarded.push((port.port, DisplayRow::Port(i)));
        } else {
            non_forwarded.push(DisplayRow::Port(i));
        }
    }

    // Merge inactive forwards with active forwards, sorted together by port
    if model.show_inactive_forwards {
        for (key, entry) in &model.forwards {
            if key.kind == ForwardKind::Local
                && entry.status == ForwardStatus::Paused
                && !scan_ports.contains(&key.remote_port)
            {
                forwarded.push((
                    key.remote_port,
                    DisplayRow::InactiveForward(key.remote_port),
                ));
            }
        }
    }
    forwarded.sort_by(|(port_a, row_a), (port_b, row_b)| {
        port_a.cmp(port_b).then_with(|| match (row_a, row_b) {
            (DisplayRow::Port(i1), DisplayRow::Port(i2)) => i1.cmp(i2),
            (DisplayRow::Port(_), DisplayRow::InactiveForward(_)) => std::cmp::Ordering::Less,
            (DisplayRow::InactiveForward(_), DisplayRow::Port(_)) => std::cmp::Ordering::Greater,
            _ => std::cmp::Ordering::Equal,
        })
    });

    let has_top = !forwarded.is_empty();
    let mut rows = Vec::with_capacity(forwarded.len() + 1 + non_forwarded.len());
    rows.extend(forwarded.into_iter().map(|(_, dr)| dr));
    if has_top && !non_forwarded.is_empty() {
        rows.push(DisplayRow::Separator);
    }
    rows.extend(non_forwarded);
    rows
}

fn build_reverse_rows(model: &Model) -> Vec<DisplayRow> {
    let local_scan_ports: std::collections::HashSet<u16> =
        model.local_ports.iter().map(|p| p.port).collect();

    // Reverse forwards are keyed by remote bind port; build a local_port → key map
    // so we can match scan rows (organized by local port) to active forwards.
    let reverse_by_local: std::collections::HashMap<u16, ForwardKey> = model
        .forwards
        .iter()
        .filter(|(k, _)| k.kind == ForwardKind::Reverse)
        .map(|(k, e)| (e.local_port, *k))
        .collect();

    let mut reverse_forwarded = Vec::new();
    let mut non_reverse_forwarded = Vec::new();

    for (i, port) in model.local_ports.iter().enumerate() {
        if reverse_by_local.contains_key(&port.port) {
            reverse_forwarded.push((port.port, DisplayRow::LocalPort(i)));
        } else {
            non_reverse_forwarded.push(DisplayRow::LocalPort(i));
        }
    }

    // Merge inactive reverse forwards (paused, whose local port is not in local scan)
    if model.show_inactive_forwards {
        for (key, entry) in &model.forwards {
            if key.kind == ForwardKind::Reverse
                && entry.status == ForwardStatus::Paused
                && !local_scan_ports.contains(&entry.local_port)
            {
                reverse_forwarded.push((
                    entry.local_port,
                    DisplayRow::InactiveReverseForward(key.remote_port),
                ));
            }
        }
    }
    reverse_forwarded.sort_by_key(|(port, _)| *port);

    let has_top = !reverse_forwarded.is_empty();
    let mut rows = Vec::with_capacity(reverse_forwarded.len() + 1 + non_reverse_forwarded.len());
    rows.extend(reverse_forwarded.into_iter().map(|(_, dr)| dr));
    if has_top && !non_reverse_forwarded.is_empty() {
        rows.push(DisplayRow::Separator);
    }
    rows.extend(non_reverse_forwarded);
    rows
}

pub fn render(model: &mut Model, frame: &mut Frame, area: Rect) {
    let title = header::build_title(model);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(title);

    let show_splash = match model.mode {
        AppMode::Forward => model.ports.is_empty() || model.started_at.elapsed().as_secs() < 1,
        AppMode::Reverse => {
            model.local_ports.is_empty() || model.started_at.elapsed().as_secs() < 1
        }
    };
    if show_splash {
        render_splash(model, frame, area, block);
        return;
    }

    let header_row = Row::new(["FWD", "PORT", "PROTO", "PID", "COMMAND"]).style(HEADER_STYLE);

    let widths = [
        Constraint::Length(9),
        Constraint::Length(8),
        Constraint::Length(7),
        Constraint::Length(9),
        Constraint::Min(20),
    ];

    let display_rows = build_display_rows(model);

    let inactive_style = Style::default()
        .fg(Color::DarkGray)
        .add_modifier(Modifier::DIM);

    let rows: Vec<Row> = display_rows
        .iter()
        .map(|dr| match dr {
            DisplayRow::Port(i) => {
                let port = &model.ports[*i];
                let fwd_cell = format_local_fwd(model, port.port);
                let proto = format!("{:?}", port.protocol).to_lowercase();
                let (pid, cmd) = match &port.process {
                    Some(p) => (format!("{}", p.pid), p.cmdline.clone()),
                    None => ("-".to_string(), "-".to_string()),
                };
                Row::new([fwd_cell.0, format!("{}", port.port), proto, pid, cmd])
                    .style(fwd_cell.1.unwrap_or_default())
            }
            DisplayRow::LocalPort(i) => {
                let port = &model.local_ports[*i];
                let fwd_cell = format_reverse_fwd(model, port.port);
                let proto = format!("{:?}", port.protocol).to_lowercase();
                let (pid, cmd) = match &port.process {
                    Some(p) => (format!("{}", p.pid), p.cmdline.clone()),
                    None => ("-".to_string(), "-".to_string()),
                };
                Row::new([fwd_cell.0, format!("{}", port.port), proto, pid, cmd])
                    .style(fwd_cell.1.unwrap_or_default())
            }
            DisplayRow::InactiveForward(remote_port) => {
                let local_port = model
                    .forwards
                    .get(&ForwardKey::local(*remote_port))
                    .map_or(*remote_port, |e| e.local_port);
                Row::new([
                    format!("||:{}", local_port),
                    format!("{}", remote_port),
                    "-".to_string(),
                    "-".to_string(),
                    "(inactive)".to_string(),
                ])
                .style(inactive_style)
            }
            DisplayRow::InactiveReverseForward(remote_port) => {
                // remote_port is the ForwardKey's remote_port (the remote bind port)
                let entry = model.forwards.get(&ForwardKey {
                    kind: ForwardKind::Reverse,
                    remote_port: *remote_port,
                });
                let local_port = entry.map_or(*remote_port, |e| e.local_port);
                Row::new([
                    format!("||<-:{}", remote_port),
                    format!("{}", local_port),
                    "-".to_string(),
                    "-".to_string(),
                    "(inactive)".to_string(),
                ])
                .style(inactive_style)
            }
            DisplayRow::Separator => {
                let sep = "─".repeat(20);
                Row::new([
                    sep.clone(),
                    sep.clone(),
                    sep.clone(),
                    sep.clone(),
                    sep.clone(),
                ])
                .style(Style::default().fg(Color::DarkGray))
            }
        })
        .collect();

    // Compute inner area before block is consumed by Table.
    let inner = block.inner(area);

    let table = Table::new(rows, widths)
        .block(block)
        .header(header_row)
        .row_highlight_style(SELECTED_STYLE)
        .highlight_symbol("▶ ")
        .column_spacing(2);

    if !display_rows.is_empty() {
        model.table_state.select(Some(model.selected_index));
    }

    frame.render_stateful_widget(table, area, &mut model.table_state);

    // Store content area for mouse hit-testing.
    // +1 row for the header row gives us where data rows start.
    model.table_content_area = Some(Rect {
        x: inner.x,
        y: inner.y + 1, // skip header row
        width: inner.width,
        height: inner.height.saturating_sub(1),
    });
}

fn render_splash(model: &Model, frame: &mut Frame, area: Rect, block: Block) {
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let logo_height = LOGO.len() as u16;
    let status_line = match model.connection_state {
        ConnectionState::Connecting => "Connecting...",
        ConnectionState::Connected => "Waiting for ports...",
        ConnectionState::Reconnecting => "Reconnecting...",
        ConnectionState::Disconnected => "Disconnected",
    };
    // logo + blank + status = total content height
    let content_height = logo_height + 2;

    let vertical = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(content_height),
        Constraint::Fill(1),
    ])
    .split(inner);

    let content_area = vertical[1];

    let mut lines: Vec<Line> = LOGO
        .iter()
        .map(|line| Line::from(Span::styled(*line, Style::default().fg(Color::Cyan))))
        .collect();
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        status_line,
        Style::default().fg(Color::DarkGray),
    )));

    let paragraph = Paragraph::new(lines).centered();
    frame.render_widget(paragraph, content_area);
}

/// Returns (display_text, optional_style_override) for the FWD column — local forward mode.
fn format_local_fwd(model: &Model, remote_port: u16) -> (String, Option<Style>) {
    match model.forwards.get(&ForwardKey::local(remote_port)) {
        Some(entry) => match &entry.status {
            ForwardStatus::Active => (
                format!("->:{}", entry.local_port),
                Some(Style::default().fg(Color::Green)),
            ),
            ForwardStatus::Paused => (
                format!("||:{}", entry.local_port),
                Some(Style::default().fg(Color::Yellow)),
            ),
            ForwardStatus::Starting => {
                ("...".to_string(), Some(Style::default().fg(Color::Yellow)))
            }
        },
        None => (String::new(), None),
    }
}

/// Returns (display_text, optional_style_override) for the FWD column — reverse forward mode.
/// `local_port` is the local port from the scan. Reverse forwards are keyed by remote bind
/// port, so we search by `entry.local_port` to find the matching forward (if any).
fn format_reverse_fwd(model: &Model, local_port: u16) -> (String, Option<Style>) {
    let found = model
        .forwards
        .iter()
        .find(|(k, e)| k.kind == ForwardKind::Reverse && e.local_port == local_port);
    match found {
        Some((key, entry)) => match &entry.status {
            ForwardStatus::Active => (
                format!("<-:{}", key.remote_port),
                Some(Style::default().fg(Color::Green)),
            ),
            ForwardStatus::Paused => (
                format!("||<-:{}", key.remote_port),
                Some(Style::default().fg(Color::Yellow)),
            ),
            ForwardStatus::Starting => {
                ("...".to_string(), Some(Style::default().fg(Color::Yellow)))
            }
        },
        None => (String::new(), None),
    }
}
