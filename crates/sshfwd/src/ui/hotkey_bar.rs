use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::Frame;

use super::hotkey_spans;
use crate::app::{AppMode, Model};

pub fn render(model: &Model, frame: &mut Frame, area: Rect) {
    let mut spans = vec![Span::raw(" ")];
    spans.extend(hotkey_spans("j/k", "Navigate "));
    spans.extend(hotkey_spans("g/G", "Top/Bottom "));
    match model.mode {
        AppMode::Forward => {
            spans.extend(hotkey_spans("Enter/f", "Forward "));
            spans.extend(hotkey_spans("F", "Custom Port "));
        }
        AppMode::Reverse => {
            spans.extend(hotkey_spans("Enter", "Reverse "));
        }
    }
    spans.extend(hotkey_spans("m", "Mode "));
    spans.extend(hotkey_spans("p", "Inactive "));
    spans.extend(hotkey_spans("q", "Quit"));
    frame.render_widget(Line::from(spans), area);
}
