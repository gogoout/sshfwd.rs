use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};
use ratatui::Frame;

use super::hotkey_spans;
use crate::app::{ModalState, Model};
use crate::forward::ForwardKind;

pub fn render(model: &Model, frame: &mut Frame) {
    let ModalState::PortInput {
        kind,
        remote_port,
        local_port,
        buffer,
        error,
        ..
    } = &model.modal
    else {
        return;
    };

    let area = centered_rect(44, 7, frame.area());

    frame.render_widget(Clear, area);

    let (title, label, border_color) = match kind {
        ForwardKind::Local => (
            format!(" Forward port :{} ", remote_port),
            "Local port: ",
            Color::Cyan,
        ),
        ForwardKind::Reverse => (
            format!(" Reverse :{} → remote ", local_port),
            "Remote bind port: ",
            Color::Magenta,
        ),
    };

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .title(title);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();
    lines.push(Line::raw(""));

    if let Some(err) = error {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(err.as_str(), Style::default().fg(Color::Red)),
        ]));
    } else {
        lines.push(Line::raw(""));
    }

    lines.push(Line::from(vec![
        Span::raw(format!("  {}", label)),
        Span::styled(
            format!("{}\u{2588}", buffer),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    lines.push(Line::raw(""));

    let mut hint_spans = vec![Span::raw("  ")];
    hint_spans.extend(hotkey_spans("Enter", "Confirm  "));
    hint_spans.extend(hotkey_spans("Esc", "Cancel"));
    lines.push(Line::from(hint_spans));

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height),
        Constraint::Fill(1),
    ])
    .split(area);

    let horizontal = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(width),
        Constraint::Fill(1),
    ])
    .split(vertical[1]);

    horizontal[1]
}
