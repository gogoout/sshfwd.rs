use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, BorderType, Row, Table, TableState};
use ratatui::Frame;

use crate::app::Model;
use crate::forward::ForwardStatus;
use crate::ui::header;

const HEADER_STYLE: Style = Style::new()
    .fg(Color::DarkGray)
    .add_modifier(Modifier::BOLD);
const SELECTED_STYLE: Style = Style::new()
    .fg(Color::White)
    .bg(Color::DarkGray)
    .add_modifier(Modifier::BOLD);

#[derive(Debug, Clone, PartialEq)]
pub enum DisplayRow {
    Port(usize), // index into model.ports
    Separator,
}

pub fn build_display_rows(model: &Model) -> Vec<DisplayRow> {
    // model.ports is already sorted by port→pid→proto,
    // so subsets preserve that order.
    let mut forwarded = Vec::new();
    let mut non_forwarded = Vec::new();

    for (i, port) in model.ports.iter().enumerate() {
        if model.forwards.contains_key(&port.port) {
            forwarded.push(DisplayRow::Port(i));
        } else {
            non_forwarded.push(DisplayRow::Port(i));
        }
    }

    let mut rows = Vec::with_capacity(forwarded.len() + 1 + non_forwarded.len());
    rows.extend(forwarded.iter().cloned());
    if !forwarded.is_empty() && !non_forwarded.is_empty() {
        rows.push(DisplayRow::Separator);
    }
    rows.extend(non_forwarded);
    rows
}

pub fn render(model: &Model, frame: &mut Frame, area: Rect) {
    let title = header::build_title(model);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(title);

    let header_row = Row::new(["FWD", "PORT", "PROTO", "PID", "COMMAND"]).style(HEADER_STYLE);

    let widths = [
        Constraint::Length(9),
        Constraint::Length(8),
        Constraint::Length(7),
        Constraint::Length(9),
        Constraint::Min(20),
    ];

    let display_rows = build_display_rows(model);

    let rows: Vec<Row> = display_rows
        .iter()
        .map(|dr| match dr {
            DisplayRow::Port(i) => {
                let port = &model.ports[*i];
                let fwd_cell = format_fwd(model, port.port);
                let proto = format!("{:?}", port.protocol).to_lowercase();
                let (pid, cmd) = match &port.process {
                    Some(p) => (format!("{}", p.pid), p.cmdline.clone()),
                    None => ("-".to_string(), "-".to_string()),
                };
                Row::new([fwd_cell.0, format!("{}", port.port), proto, pid, cmd])
                    .style(fwd_cell.1.unwrap_or_default())
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

    let table = Table::new(rows, widths)
        .block(block)
        .header(header_row)
        .row_highlight_style(SELECTED_STYLE)
        .highlight_symbol("▶ ")
        .column_spacing(2);

    let mut table_state = TableState::default();
    if !display_rows.is_empty() {
        table_state.select(Some(model.selected_index));
    }

    frame.render_stateful_widget(table, area, &mut table_state);
}

/// Returns (display_text, optional_style_override) for the FWD column.
fn format_fwd(model: &Model, remote_port: u16) -> (String, Option<Style>) {
    match model.forwards.get(&remote_port) {
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
