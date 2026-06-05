use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::Frame;

use super::hotkey_spans;
use crate::app::{AppMode, Model};
use crate::paste::TransientStatusKind;

pub fn render(model: &Model, frame: &mut Frame, area: Rect) {
    if let Some(status) = model.transient_status.as_ref().filter(|s| !s.is_expired()) {
        let color = match status.kind {
            TransientStatusKind::Ok => Color::Green,
            TransientStatusKind::Pending => Color::Yellow,
            TransientStatusKind::Err => Color::Red,
        };
        let line = Line::from(vec![
            Span::raw(" "),
            Span::styled(
                status.text.clone(),
                Style::new().fg(color).add_modifier(Modifier::BOLD),
            ),
        ]);
        frame.render_widget(line, area);
        return;
    }

    let mut spans = vec![Span::raw(" ")];
    spans.extend(hotkey_spans("j/k", "Navigate "));
    spans.extend(hotkey_spans("g/G", "Top/Bottom "));
    match model.mode {
        AppMode::Forward => {
            spans.extend(hotkey_spans("Enter/f", "Forward "));
            spans.extend(hotkey_spans("F", "Custom Port "));
        }
        AppMode::Reverse => {
            spans.extend(hotkey_spans("Enter/f", "Reverse "));
        }
    }
    spans.extend(hotkey_spans("m", "Mode "));
    spans.extend(hotkey_spans("p", "Inactive "));
    spans.extend(hotkey_spans("C-v", "Paste image "));
    spans.extend(hotkey_spans("q", "Quit"));
    frame.render_widget(Line::from(spans), area);
}
