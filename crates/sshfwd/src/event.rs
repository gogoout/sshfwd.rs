use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};

use crate::app::Message;

pub fn crossterm_event_to_message(event: Event) -> Option<Message> {
    match event {
        // Accept Press and Repeat, ignore Release (matches dua-cli's pattern)
        Event::Key(key) if key.kind != KeyEventKind::Release => match key.code {
            KeyCode::Char('q') => Some(Message::Quit),
            KeyCode::Esc => Some(Message::Quit),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(Message::Quit)
            }
            KeyCode::Char('j') | KeyCode::Down => Some(Message::MoveDown),
            KeyCode::Char('k') | KeyCode::Up => Some(Message::MoveUp),
            KeyCode::Char('g') => Some(Message::GoToTop),
            KeyCode::Char('G') => Some(Message::GoToBottom),
            _ => None,
        },
        Event::Resize(w, h) => Some(Message::Resize(w, h)),
        _ => None,
    }
}
