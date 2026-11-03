//! Real service implementations backed by GTK.

use gtk::gdk;
use gtk::glib::prelude::*;
use gtk::prelude::*;

use std::path::PathBuf;

use super::{ClipboardService, FileDialogService, LinkService, LocalBoxFuture, ServiceError};

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

/// Follows the desktop's light/dark preference (F5.R5) through the XDG
/// Settings portal (`org.freedesktop.appearance color-scheme`), which GNOME,
/// KDE and Flatpak all provide. Without a portal, nothing is reported and
/// the theme stays on its default (dark).
#[derive(Debug)]
pub struct SystemColorScheme;

impl SystemColorScheme {
    const BUS_NAME: &str = "org.freedesktop.portal.Desktop";
    const PATH: &str = "/org/freedesktop/portal/desktop";
    const INTERFACE: &str = "org.freedesktop.portal.Settings";
    const NAMESPACE: &str = "org.freedesktop.appearance";
    const KEY: &str = "color-scheme";

    /// Call `on_change(prefers_dark)` now (once known) and whenever the
    /// desktop setting changes.
    pub fn watch(on_change: impl Fn(bool) + 'static) {
        use gtk::gio;
        use gtk::glib;
        let on_change = std::rc::Rc::new(on_change);
        gio::DBusProxy::for_bus(
            gio::BusType::Session,
            gio::DBusProxyFlags::DO_NOT_AUTO_START_AT_CONSTRUCTION,
            None,
            Self::BUS_NAME,
            Self::PATH,
            Self::INTERFACE,
            gio::Cancellable::NONE,
            move |proxy| {
                let proxy = match proxy {
                    Ok(proxy) => proxy,
                    Err(e) => {
                        tracing::debug!(error = %e, "no settings portal; keeping the default theme");
                        return;
                    }
                };
                let changed = on_change.clone();
                proxy.connect_local("g-signal", false, move |args| {
                    let signal = args.get(2).and_then(|v| v.get::<String>().ok());
                    let params = args.get(3).and_then(|v| v.get::<glib::Variant>().ok());
                    if signal.as_deref() == Some("SettingChanged")
                        && let Some(params) = params
                        && let Some((namespace, key, value)) =
                            params.get::<(String, String, glib::Variant)>()
                        && namespace == Self::NAMESPACE
                        && key == Self::KEY
                        && let Some(dark) = prefers_dark(&value)
                    {
                        changed(dark);
                    }
                    None
                });
                proxy.call(
                    "ReadOne",
                    Some(&(Self::NAMESPACE, Self::KEY).to_variant()),
                    gio::DBusCallFlags::NONE,
                    -1,
                    gio::Cancellable::NONE,
                    move |result| match result {
                        Ok(reply) => {
                            if let Some(dark) = reply
                                .child_value(0)
                                .as_variant()
                                .and_then(|v| prefers_dark(&v))
                            {
                                on_change(dark);
                            }
                        }
                        Err(e) => tracing::debug!(error = %e, "could not read the color scheme"),
                    },
                );
            },
        );
    }
}

/// `color-scheme`: 1 = prefer dark, 2 = prefer light, 0 = no preference.
fn prefers_dark(value: &gtk::glib::Variant) -> Option<bool> {
    // Some portals wrap the value in one more variant.
    let value = value.as_variant().unwrap_or_else(|| value.clone());
    match value.get::<u32>()? {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}

/// Native open/save dialogs (F7.R2), through the file chooser portal when
/// sandboxed. The parent window is set once it exists.
#[derive(Debug, Default)]
pub struct GtkFileDialogService {
    parent: std::cell::RefCell<gtk::glib::WeakRef<gtk::Window>>,
}

impl GtkFileDialogService {
    pub fn set_parent(&self, window: &impl IsA<gtk::Window>) {
        self.parent.borrow().set(Some(window.upcast_ref()));
    }

    fn markdown_filter() -> gtk::gio::ListStore {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("Markdown"));
        for pattern in ["*.md", "*.markdown", "*.mdown", "*.txt"] {
            filter.add_pattern(pattern);
        }
        let filters = gtk::gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        filters
    }
}

impl FileDialogService for GtkFileDialogService {
    fn pick_open(&self) -> LocalBoxFuture<'_, Option<PathBuf>> {
        let parent = self.parent.borrow().upgrade();
        Box::pin(async move {
            let dialog = gtk::FileDialog::builder()
                .title("Open Markdown File")
                .modal(true)
                .filters(&Self::markdown_filter())
                .build();
            dialog.open_future(parent.as_ref()).await.ok()?.path()
        })
    }

    fn pick_save<'a>(&'a self, suggested_name: &'a str) -> LocalBoxFuture<'a, Option<PathBuf>> {
        let parent = self.parent.borrow().upgrade();
        Box::pin(async move {
            let dialog = gtk::FileDialog::builder()
                .title("Save As")
                .modal(true)
                .initial_name(suggested_name)
                .build();
            dialog.save_future(parent.as_ref()).await.ok()?.path()
        })
    }
}
