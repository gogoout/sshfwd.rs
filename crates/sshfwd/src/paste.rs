use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crossbeam_channel::Sender;
use tokio::sync::mpsc::UnboundedSender;

use crate::app::Message;
use crate::forward::{ForwardCommand, ForwardEvent};

pub const TRANSIENT_STATUS_TTL: Duration = Duration::from_secs(1);

pub struct TransientStatus {
    pub text: String,
    pub expires_at: Instant,
    pub is_error: bool,
}

impl TransientStatus {
    pub fn ok(text: String) -> Self {
        Self {
            text,
            expires_at: Instant::now() + TRANSIENT_STATUS_TTL,
            is_error: false,
        }
    }

    pub fn err(text: String) -> Self {
        Self {
            text,
            expires_at: Instant::now() + TRANSIENT_STATUS_TTL,
            is_error: true,
        }
    }

    pub fn is_expired(&self) -> bool {
        Instant::now() >= self.expires_at
    }
}

/// Read the local clipboard image, encode it to PNG, and dispatch an
/// `UploadImage` command. Runs on a background thread — never blocks the UI.
/// On failure (no image, encode error, channel closed) emits an
/// `ImageUploadFailed` event via `bg_tx`.
pub fn spawn_paste(fwd_tx: UnboundedSender<ForwardCommand>, bg_tx: Sender<Message>) {
    std::thread::spawn(move || {
        let send_err = |error: String| {
            let _ = bg_tx.send(Message::ForwardEvent(ForwardEvent::ImageUploadFailed {
                error,
            }));
        };

        let mut clipboard = match arboard::Clipboard::new() {
            Ok(cb) => cb,
            Err(e) => {
                send_err(format!("clipboard: {e}"));
                return;
            }
        };

        let img = match clipboard.get_image() {
            Ok(img) => img,
            Err(_) => {
                send_err("no image on clipboard".to_string());
                return;
            }
        };

        let png_bytes = match encode_rgba_png(&img.bytes, img.width as u32, img.height as u32) {
            Ok(b) => b,
            Err(e) => {
                send_err(format!("png encode: {e}"));
                return;
            }
        };

        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let path = format!("/tmp/sshfwd-{ms}.png");

        if fwd_tx
            .send(ForwardCommand::UploadImage {
                path,
                bytes: png_bytes,
            })
            .is_err()
        {
            send_err("session unavailable".to_string());
        }
    });
}

/// Replace the local clipboard contents with `path`. Fire-and-forget.
pub fn set_clipboard_text(path: String) {
    std::thread::spawn(move || {
        if let Ok(mut cb) = arboard::Clipboard::new() {
            let _ = cb.set_text(path);
        }
    });
}

fn encode_rgba_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, png::EncodingError> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(rgba)?;
    }
    Ok(out)
}
