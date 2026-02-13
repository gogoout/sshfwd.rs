use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::Frame;

use crate::app::Model;

const KEY_STYLE: Style = Style::new().fg(Color::Black).bg(Color::DarkGray);
const BRACKET_STYLE: Style = Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD);
const DESC_STYLE: Style = Style::new().fg(Color::DarkGray);

pub fn render(_model: &Model, frame: &mut Frame, area: Rect) {
    let line = Line::from(vec![
        Span::raw(" "),
        Span::styled("<", BRACKET_STYLE),
        Span::styled("j/k", KEY_STYLE),
        Span::styled(">", BRACKET_STYLE),
        Span::styled("Navigate ", DESC_STYLE),
        Span::styled("<", BRACKET_STYLE),
        Span::styled("g/G", KEY_STYLE),
        Span::styled(">", BRACKET_STYLE),
        Span::styled("Top/Bottom ", DESC_STYLE),
        Span::styled("<", BRACKET_STYLE),
        Span::styled("Enter/f", KEY_STYLE),
        Span::styled(">", BRACKET_STYLE),
        Span::styled("Forward ", DESC_STYLE),
        Span::styled("<", BRACKET_STYLE),
        Span::styled("F", KEY_STYLE),
        Span::styled(">", BRACKET_STYLE),
        Span::styled("Custom Port ", DESC_STYLE),
        Span::styled("<", BRACKET_STYLE),
        Span::styled("p", KEY_STYLE),
        Span::styled(">", BRACKET_STYLE),
        Span::styled("Inactive ", DESC_STYLE),
        Span::styled("<", BRACKET_STYLE),
        Span::styled("q", KEY_STYLE),
        Span::styled(">", BRACKET_STYLE),
        Span::styled("Quit", DESC_STYLE),
    ]);
    frame.render_widget(line, area);
}
