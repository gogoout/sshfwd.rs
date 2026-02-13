pub mod header;
pub mod hotkey_bar;
pub mod modal;
pub mod table;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};

pub const CONNECTED_CHAR: char = '●';
pub const CONNECTING_CHAR: char = '●';
pub const DISCONNECT_CHAR: char = '●';

// Shared hotkey label styles used by hotkey_bar and modal
const BRACKET_STYLE: Style = Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD);
const KEY_STYLE: Style = Style::new().fg(Color::Black).bg(Color::DarkGray);
const DESC_STYLE: Style = Style::new().fg(Color::DarkGray);

/// Build the `<key>desc` span pattern used in hotkey bar and modal.
pub fn hotkey_spans(key: &str, desc: &str) -> [ratatui::text::Span<'static>; 4] {
    use ratatui::text::Span;
    [
        Span::styled("<", BRACKET_STYLE),
        Span::styled(key.to_owned(), KEY_STYLE),
        Span::styled(">", BRACKET_STYLE),
        Span::styled(desc.to_owned(), DESC_STYLE),
    ]
}

pub struct LayoutAreas {
    pub table: Rect,
    pub hotkey_bar: Rect,
}

pub fn layout_areas(area: Rect) -> LayoutAreas {
    let chunks = Layout::vertical([Constraint::Min(3), Constraint::Length(1)]).split(area);
    LayoutAreas {
        table: chunks[0],
        hotkey_bar: chunks[1],
    }
}
