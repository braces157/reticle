use arboard::{Clipboard, ImageData};
use std::borrow::Cow;
use std::thread;
use std::time::Duration;
use thiserror::Error;

const CLIPBOARD_RETRIES: usize = 3;
const CLIPBOARD_BACKOFF: Duration = Duration::from_millis(25);

#[derive(Debug, Error)]
pub enum ClipboardError {
    #[error("invalid image buffer ({width}x{height}, {actual_len} bytes; expected {expected_len})")]
    InvalidImageBuffer {
        width: usize,
        height: usize,
        actual_len: usize,
        expected_len: usize,
    },
    #[error("Windows clipboard is busy or unavailable: {0}")]
    AccessFailed(String),
}

fn with_retry<T, F>(mut op: F) -> Result<T, ClipboardError>
where
    F: FnMut() -> Result<T, arboard::Error>,
{
    let mut last_err = None;
    for attempt in 0..CLIPBOARD_RETRIES {
        match op() {
            Ok(v) => return Ok(v),
            Err(err) => {
                last_err = Some(err.to_string());
                if attempt + 1 < CLIPBOARD_RETRIES {
                    thread::sleep(CLIPBOARD_BACKOFF);
                }
            }
        }
    }
    Err(ClipboardError::AccessFailed(
        last_err.unwrap_or_else(|| "unknown clipboard error".to_string()),
    ))
}

pub fn copy_text(text: &str) -> Result<(), ClipboardError> {
    with_retry(|| {
        let mut cb = Clipboard::new()?;
        cb.set_text(text.to_string())
    })
}

pub fn copy_rgba_image(width: usize, height: usize, rgba: &[u8]) -> Result<(), ClipboardError> {
    let expected_len = width.saturating_mul(height).saturating_mul(4);
    if width == 0 || height == 0 || rgba.len() != expected_len {
        return Err(ClipboardError::InvalidImageBuffer {
            width,
            height,
            actual_len: rgba.len(),
            expected_len,
        });
    }

    with_retry(|| {
        let mut cb = Clipboard::new()?;
        cb.set_image(ImageData {
            width,
            height,
            bytes: Cow::Borrowed(rgba),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_zero_or_mismatched_image_buffers() {
        let err = copy_rgba_image(0, 10, &[]).expect_err("zero width must fail");
        assert!(matches!(err, ClipboardError::InvalidImageBuffer { .. }));

        let err2 = copy_rgba_image(4, 4, &[0u8; 10]).expect_err("short buffer must fail");
        assert!(matches!(
            err2,
            ClipboardError::InvalidImageBuffer {
                width: 4,
                height: 4,
                actual_len: 10,
                expected_len: 64
            }
        ));
    }
}
