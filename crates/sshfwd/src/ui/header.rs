use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::app::{AppMode, ConnectionState, Model};
use crate::forward::{ForwardKind, ForwardStatus};
use crate::ui::{CONNECTED_CHAR, CONNECTING_CHAR, DISCONNECT_CHAR};

pub fn build_title(model: &Model) -> Line<'static> {
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

    let (mode_label, mode_style) = match model.mode {
        AppMode::Forward => (
            "M:Fwd",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        AppMode::Reverse => (
            "M:Rev",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ),
    };

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
        let fwd_kind_count = model
            .forwards
            .iter()
            .filter(|(k, e)| {
                k.kind == ForwardKind::Local && matches!(e.status, ForwardStatus::Active)
            })
            .count();
        let rev_kind_count = model
            .forwards
            .iter()
            .filter(|(k, e)| {
                k.kind == ForwardKind::Reverse && matches!(e.status, ForwardStatus::Active)
            })
            .count();
        if fwd_kind_count > 0 {
            spans.push(Span::styled(
                format!("│ {} fwd ", fwd_kind_count),
                Style::default().fg(Color::Green),
            ));
        }
        if rev_kind_count > 0 {
            spans.push(Span::styled(
                format!("│ {} rev ", rev_kind_count),
                Style::default().fg(Color::Magenta),
            ));
        }
    }

    spans.push(Span::raw("│ "));
    spans.push(Span::styled(mode_label, mode_style));
    spans.push(Span::raw(" "));

    Line::from(spans)
}
