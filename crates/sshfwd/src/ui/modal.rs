use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};
use ratatui::Frame;

use crate::app::{ModalState, Model};

const BRACKET_STYLE: Style = Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD);
const KEY_STYLE: Style = Style::new().fg(Color::Black).bg(Color::DarkGray);
const DESC_STYLE: Style = Style::new().fg(Color::DarkGray);

pub fn render(model: &Model, frame: &mut Frame) {
    let ModalState::PortInput {
        remote_port,
        buffer,
        error,
        ..
    } = &model.modal
    else {
        return;
    };

    let area = centered_rect(40, 7, frame.area());

    frame.render_widget(Clear, area);

    let title = format!(" Forward port {} ", remote_port);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan))
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
        Span::raw("  Local port: "),
        Span::styled(
            format!("{}\u{2588}", buffer),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    lines.push(Line::raw(""));

    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("<", BRACKET_STYLE),
        Span::styled("Enter", KEY_STYLE),
        Span::styled(">", BRACKET_STYLE),
        Span::styled("Confirm  ", DESC_STYLE),
        Span::styled("<", BRACKET_STYLE),
        Span::styled("Esc", KEY_STYLE),
        Span::styled(">", BRACKET_STYLE),
        Span::styled("Cancel", DESC_STYLE),
    ]));

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
