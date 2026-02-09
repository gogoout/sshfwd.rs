use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use crate::app::{ConnectionState, Model};
use crate::ui::{DISCONNECT_CHAR, SPINNER_CHARS};

/// Builds the title Line used in the table block border.
pub fn build_title(model: &Model) -> Line<'_> {
    let (indicator, indicator_style) = match model.connection_state {
        ConnectionState::Connecting => (
            SPINNER_CHARS[model.spinner_frame],
            Style::default().fg(Color::Yellow),
        ),
        ConnectionState::Connected => (
            SPINNER_CHARS[model.spinner_frame],
            Style::default().fg(Color::Green),
        ),
        ConnectionState::Disconnected => (DISCONNECT_CHAR, Style::default().fg(Color::Red)),
    };

    let conn_str = match (&model.username, &model.hostname) {
        (Some(u), Some(h)) => format!("{u}@{h}"),
        _ => model.destination.clone(),
    };

    let port_count = model.ports.len();

    let spans = vec![
        Span::raw(" "),
        Span::styled(indicator.to_string(), indicator_style),
        Span::raw(" "),
        Span::styled(conn_str, Style::default().fg(Color::Cyan)),
        Span::styled(
            format!(" │ {} ports ", port_count),
            Style::default().fg(Color::DarkGray),
        ),
    ];

    Line::from(spans)
}
