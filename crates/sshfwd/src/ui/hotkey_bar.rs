use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::Frame;

use super::hotkey_spans;
use crate::app::Model;

pub fn render(_model: &Model, frame: &mut Frame, area: Rect) {
    let mut spans = vec![Span::raw(" ")];
    spans.extend(hotkey_spans("j/k", "Navigate "));
    spans.extend(hotkey_spans("g/G", "Top/Bottom "));
    spans.extend(hotkey_spans("Enter/f", "Forward "));
    spans.extend(hotkey_spans("F", "Custom Port "));
    spans.extend(hotkey_spans("p", "Inactive "));
    spans.extend(hotkey_spans("q", "Quit"));
    frame.render_widget(Line::from(spans), area);
}
