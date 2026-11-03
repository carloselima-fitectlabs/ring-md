//! Real service implementations backed by GTK.

use gtk::gdk;
use gtk::prelude::*;

use super::{ClipboardService, LinkService, LocalBoxFuture, ServiceError};

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

/// Opens URIs with the desktop's default handler (works in Flatpak through
/// the OpenURI portal).
#[derive(Debug, Clone, Default)]
pub struct GtkLinkService {
    parent: Option<gtk::Window>,
}

impl GtkLinkService {
    pub fn new(parent: Option<gtk::Window>) -> Self {
        Self { parent }
    }
}

impl LinkService for GtkLinkService {
    fn open(&self, uri: &str) {
        let uri = uri.to_owned();
        gtk::UriLauncher::new(&uri).launch(
            self.parent.as_ref(),
            gtk::gio::Cancellable::NONE,
            move |result| {
                if let Err(e) = result {
                    tracing::warn!(%uri, error = %e, "could not open link");
                }
            },
        );
    }
}
