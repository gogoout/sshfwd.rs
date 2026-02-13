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
pub const BRACKET_STYLE: Style = Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD);
pub const KEY_STYLE: Style = Style::new().fg(Color::Black).bg(Color::DarkGray);
pub const DESC_STYLE: Style = Style::new().fg(Color::DarkGray);

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
