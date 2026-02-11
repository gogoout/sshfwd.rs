pub mod header;
pub mod hotkey_bar;
pub mod modal;
pub mod table;

use ratatui::layout::{Constraint, Layout, Rect};

pub const CONNECTED_CHAR: char = '●';
pub const CONNECTING_CHAR: char = '●';
pub const DISCONNECT_CHAR: char = '●';

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
