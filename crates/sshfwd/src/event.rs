use crossterm::event::{Event, KeyEventKind};

use crate::app::Message;

pub fn crossterm_event_to_message(event: Event) -> Option<Message> {
    match event {
        Event::Key(key) if key.kind != KeyEventKind::Release => Some(Message::Key(key)),
        Event::Mouse(mouse) => Some(Message::Mouse(mouse)),
        Event::Resize(w, h) => Some(Message::Resize(w, h)),
        _ => None,
    }
}
