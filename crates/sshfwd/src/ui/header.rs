use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use crate::app::{ConnectionState, Model};
use crate::forward::ForwardStatus;
use crate::ui::{CONNECTED_CHAR, CONNECTING_CHAR, DISCONNECT_CHAR};

pub fn build_title(model: &Model) -> Line<'_> {
    let (indicator, indicator_style) = match model.connection_state {
        ConnectionState::Connecting => (CONNECTING_CHAR, Style::default().fg(Color::Yellow)),
        ConnectionState::Connected => (CONNECTED_CHAR, Style::default().fg(Color::Green)),
        ConnectionState::Disconnected => (DISCONNECT_CHAR, Style::default().fg(Color::Red)),
    };

    let conn_str = match (&model.username, &model.hostname) {
        (Some(u), Some(h)) => format!("{u}@{h}"),
        _ => model.destination.clone(),
    };

    let port_count = model.ports.len();
    let fwd_count = model
        .forwards
        .values()
        .filter(|e| matches!(e.status, ForwardStatus::Active))
        .count();

    let mut spans = vec![
        Span::raw(" "),
        Span::styled(indicator.to_string(), indicator_style),
        Span::raw(" "),
        Span::styled(conn_str, Style::default().fg(Color::Cyan)),
        Span::styled(
            format!(" │ {} ports ", port_count),
            Style::default().fg(Color::DarkGray),
        ),
    ];

    if fwd_count > 0 {
        spans.push(Span::styled(
            format!("│ {} fwd ", fwd_count),
            Style::default().fg(Color::Green),
        ));
    }

    Line::from(spans)
}
