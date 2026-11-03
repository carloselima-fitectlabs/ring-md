//! Real service implementations backed by GTK.

use gtk::gdk;
use gtk::prelude::*;

use super::{ClipboardService, LocalBoxFuture, ServiceError};

/// The system clipboard (F2.R8). Works on Wayland and X11 without external
/// tools such as `xclip` or `wl-copy`.
#[derive(Debug, Clone)]
pub struct GtkClipboardService {
    clipboard: gdk::Clipboard,
}

impl GtkClipboardService {
    pub fn new(display: &gdk::Display) -> Self {
        Self {
            clipboard: display.clipboard(),
        }
    }
}

impl ClipboardService for GtkClipboardService {
    fn read_text(&self) -> LocalBoxFuture<'_, Result<Option<String>, ServiceError>> {
        let clipboard = self.clipboard.clone();
        Box::pin(async move {
            clipboard
                .read_text_future()
                .await
                .map(|text| text.map(Into::into))
                .map_err(|e| ServiceError::Other(e.to_string()))
        })
    }

    fn set_text(&self, text: &str) {
        self.clipboard.set_text(text);
    }
}
