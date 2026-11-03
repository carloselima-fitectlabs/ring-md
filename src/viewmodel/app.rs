//! Application-level state: window title, preview visibility, fullscreen and
//! error reporting.

use std::path::PathBuf;

use gtk::glib;
use gtk::glib::prelude::*;
use gtk::glib::subclass::prelude::*;

use crate::model::Document;

/// Name of the signal emitted by [`AppViewModel::report_error`].
pub const SIGNAL_ERROR_RAISED: &str = "error-raised";

mod imp {
    use std::cell::{Cell, RefCell};
    use std::sync::OnceLock;

    use gtk::glib::subclass::Signal;

    use super::*;

    #[derive(Debug, glib::Properties)]
    #[properties(wrapper_type = super::AppViewModel)]
    pub struct AppViewModel {
        /// `<filename>[+] — RingMD` (F1.R4).
        #[property(name = "title", get = Self::title, type = String)]
        /// Whether the buffer has unsaved changes.
        #[property(name = "modified", get = Self::modified, type = bool)]
        /// Absolute or relative path of the file, if any.
        #[property(name = "file-path", get = Self::file_path, type = Option<String>)]
        pub(super) document: RefCell<Document>,

        /// Whether the preview pane is shown (F4.R9).
        #[property(get, set = Self::set_preview_visible, explicit_notify)]
        preview_visible: Cell<bool>,

        /// Whether the window is fullscreen (§5, F11).
        #[property(get, set = Self::set_fullscreen, explicit_notify)]
        fullscreen: Cell<bool>,
    }

    impl Default for AppViewModel {
        fn default() -> Self {
            Self {
                document: RefCell::default(),
                preview_visible: Cell::new(true),
                fullscreen: Cell::new(false),
            }
        }
    }

    impl AppViewModel {
        fn title(&self) -> String {
            self.document.borrow().window_title()
        }

        fn modified(&self) -> bool {
            self.document.borrow().is_modified()
        }

        fn file_path(&self) -> Option<String> {
            self.document
                .borrow()
                .path()
                .map(|path| path.to_string_lossy().into_owned())
        }

        fn set_preview_visible(&self, visible: bool) {
            if self.preview_visible.replace(visible) != visible {
                self.obj().notify_preview_visible();
            }
        }

        fn set_fullscreen(&self, fullscreen: bool) {
            if self.fullscreen.replace(fullscreen) != fullscreen {
                self.obj().notify_fullscreen();
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AppViewModel {
        const NAME: &'static str = "RingmdAppViewModel";
        type Type = super::AppViewModel;
    }

    #[glib::derived_properties]
    impl ObjectImpl for AppViewModel {
        fn signals() -> &'static [Signal] {
            static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| {
                vec![
                    Signal::builder(SIGNAL_ERROR_RAISED)
                        .param_types([String::static_type(), String::static_type()])
                        .build(),
                ]
            })
        }
    }
}

glib::wrapper! {
    /// ViewModel for the main window. See `docs/SPEC.md` §3.1.
    pub struct AppViewModel(ObjectSubclass<imp::AppViewModel>);
}

impl AppViewModel {
    pub fn new(document: Document) -> Self {
        let vm: Self = glib::Object::new();
        vm.imp().document.replace(document);
        vm
    }

    /// Snapshot of the current document state.
    pub fn document(&self) -> Document {
        self.imp().document.borrow().clone()
    }

    /// Update the file path (e.g. after `:saveas` or `:edit`).
    pub fn set_document_path(&self, path: Option<PathBuf>) {
        {
            let mut doc = self.imp().document.borrow_mut();
            if doc.path() == path.as_deref() {
                return;
            }
            doc.set_path(path);
        }
        self.notify_file_path();
        self.notify_title();
    }

    /// Update the modified flag (mirrors Neovim's `&modified`).
    pub fn set_modified(&self, modified: bool) {
        {
            let mut doc = self.imp().document.borrow_mut();
            if doc.is_modified() == modified {
                return;
            }
            doc.set_modified(modified);
        }
        self.notify_modified();
        self.notify_title();
    }

    /// Command: show/hide the preview pane (`:Preview`, `Ctrl+P`).
    pub fn toggle_preview(&self) {
        self.set_preview_visible(!self.preview_visible());
    }

    /// Command: enter/leave fullscreen (`F11`).
    pub fn toggle_fullscreen(&self) {
        self.set_fullscreen(!self.fullscreen());
    }

    /// Log an error and ask the View to show it (F2.R3, F2.R7).
    pub fn report_error(&self, title: &str, detail: &str) {
        tracing::error!(%title, %detail, "error reported to user");
        self.emit_by_name::<()>(SIGNAL_ERROR_RAISED, &[&title, &detail]);
    }

    /// Subscribe to errors raised with [`Self::report_error`].
    pub fn connect_error_raised<F: Fn(&Self, &str, &str) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            SIGNAL_ERROR_RAISED,
            false,
            glib::closure_local!(move |vm: &Self, title: &str, detail: &str| {
                f(vm, title, detail);
            }),
        )
    }
}

impl Default for AppViewModel {
    fn default() -> Self {
        Self::new(Document::new())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    /// Collects the names of properties notified on `vm`.
    fn record_notifications(vm: &AppViewModel) -> Rc<RefCell<Vec<String>>> {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let sink = seen.clone();
        vm.connect_notify_local(None, move |_, pspec| {
            sink.borrow_mut().push(pspec.name().to_owned());
        });
        seen
    }

    #[test]
    fn defaults() {
        let vm = AppViewModel::default();
        assert_eq!(vm.title(), "[No Name] — RingMD");
        assert!(!vm.modified());
        assert_eq!(vm.file_path(), None);
        assert!(vm.preview_visible());
        assert!(!vm.fullscreen());
    }

    #[test]
    fn title_follows_document() {
        let vm = AppViewModel::new(Document::with_path("/tmp/notes.md"));
        assert_eq!(vm.title(), "notes.md — RingMD");
        assert_eq!(vm.file_path().as_deref(), Some("/tmp/notes.md"));

        let seen = record_notifications(&vm);
        vm.set_modified(true);
        assert_eq!(vm.title(), "notes.md+ — RingMD");
        assert_eq!(*seen.borrow(), ["modified", "title"]);

        seen.borrow_mut().clear();
        vm.set_document_path(Some("other.md".into()));
        assert_eq!(vm.title(), "other.md+ — RingMD");
        assert_eq!(*seen.borrow(), ["file-path", "title"]);
    }

    #[test]
    fn unchanged_values_do_not_notify() {
        let vm = AppViewModel::new(Document::with_path("a.md"));
        let seen = record_notifications(&vm);
        vm.set_modified(false);
        vm.set_document_path(Some("a.md".into()));
        vm.set_preview_visible(true);
        vm.set_fullscreen(false);
        assert!(seen.borrow().is_empty(), "unexpected: {:?}", seen.borrow());
    }

    #[test]
    fn toggle_commands() {
        let vm = AppViewModel::default();
        let seen = record_notifications(&vm);
        vm.toggle_preview();
        assert!(!vm.preview_visible());
        vm.toggle_fullscreen();
        assert!(vm.fullscreen());
        assert_eq!(*seen.borrow(), ["preview-visible", "fullscreen"]);
    }

    #[test]
    fn report_error_emits_signal() {
        let vm = AppViewModel::default();
        let got = Rc::new(RefCell::new(None));
        let sink = got.clone();
        vm.connect_error_raised(move |_, title, detail| {
            *sink.borrow_mut() = Some((title.to_owned(), detail.to_owned()));
        });
        vm.report_error("Neovim not found", "Install nvim 0.10+");
        assert_eq!(
            *got.borrow(),
            Some(("Neovim not found".into(), "Install nvim 0.10+".into()))
        );
    }
}
