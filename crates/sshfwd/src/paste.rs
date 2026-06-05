use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crossbeam_channel::Sender;
use tokio::sync::mpsc::UnboundedSender;

use crate::app::Message;
use crate::forward::{ForwardCommand, ForwardEvent};

pub const TRANSIENT_STATUS_TTL: Duration = Duration::from_secs(2);
/// Upper bound — pending statuses are normally replaced by the result event
/// long before this fires. Acts as a safety net if the upload silently hangs.
pub const PENDING_STATUS_TTL: Duration = Duration::from_secs(30);

/// Reject paste-uploads larger than this so a stray huge clipboard image
/// doesn't block ForwardManager (every other forward command queues behind
/// the SSH stdin pipe during the upload).
const MAX_UPLOAD_BYTES: usize = 10_000_000;

pub enum TransientStatusKind {
    Ok,
    Pending,
    Err,
}

pub struct TransientStatus {
    pub text: String,
    pub expires_at: Instant,
    pub kind: TransientStatusKind,
}

impl TransientStatus {
    pub fn ok(text: String) -> Self {
        Self {
            text,
            expires_at: Instant::now() + TRANSIENT_STATUS_TTL,
            kind: TransientStatusKind::Ok,
        }
    }

    pub fn pending(text: String) -> Self {
        Self {
            text,
            expires_at: Instant::now() + PENDING_STATUS_TTL,
            kind: TransientStatusKind::Pending,
        }
    }

    pub fn err(text: String) -> Self {
        Self {
            text,
            expires_at: Instant::now() + TRANSIENT_STATUS_TTL,
            kind: TransientStatusKind::Err,
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

        if png_bytes.len() > MAX_UPLOAD_BYTES {
            let mb = png_bytes.len() as f64 / 1_000_000.0;
            send_err(format!("image too large ({mb:.1} MB)"));
            return;
        }

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

/// Replace the local clipboard contents with `path`. Fire-and-forget for the
/// success path; if the clipboard set fails the user is told via the footer so
/// they don't see "Uploaded" and then silently get the wrong clipboard.
pub fn set_clipboard_text(path: String, bg_tx: Sender<Message>) {
    std::thread::spawn(move || {
        let result =
            arboard::Clipboard::new().and_then(|mut cb| cb.set_text(path.as_str().to_owned()));
        if let Err(e) = result {
            let _ = bg_tx.send(Message::ForwardEvent(ForwardEvent::ImageUploadFailed {
                error: format!("uploaded to {path}, but clipboard set failed: {e}"),
            }));
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
