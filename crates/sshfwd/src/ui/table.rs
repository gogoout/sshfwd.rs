use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, BorderType, Row, Table, TableState};
use ratatui::Frame;

use crate::app::Model;
use crate::ui::header;

const HEADER_STYLE: Style = Style::new()
    .fg(Color::DarkGray)
    .add_modifier(Modifier::BOLD);
const SELECTED_STYLE: Style = Style::new()
    .fg(Color::White)
    .bg(Color::DarkGray)
    .add_modifier(Modifier::BOLD);

pub fn render(model: &Model, frame: &mut Frame, area: Rect) {
    let title = header::build_title(model);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(title);

    let header_row = Row::new(["PORT", "PROTO", "PID", "COMMAND"]).style(HEADER_STYLE);

    let widths = [
        Constraint::Length(8),
        Constraint::Length(7),
        Constraint::Length(9),
        Constraint::Min(20),
    ];

    let rows: Vec<Row> = model
        .ports
        .iter()
        .map(|port| {
            let proto = format!("{:?}", port.protocol).to_lowercase();
            let (pid, cmd) = match &port.process {
                Some(p) => (format!("{}", p.pid), p.cmdline.clone()),
                None => ("-".to_string(), "-".to_string()),
            };
            Row::new([format!("{}", port.port), proto, pid, cmd])
        })
        .collect();

    let table = Table::new(rows, widths)
        .block(block)
        .header(header_row)
        .row_highlight_style(SELECTED_STYLE)
        .highlight_symbol("▶ ")
        .column_spacing(2);

    let mut table_state = TableState::default();
    if !model.ports.is_empty() {
        table_state.select(Some(model.selected_index));
    }

    frame.render_stateful_widget(table, area, &mut table_state);
}
