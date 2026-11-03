//! Live preview state (T3.4, T3.5, T3.6, T3.7).
//!
//! The bundled config sends the buffer text (already debounced) and the
//! cursor line. Rendering runs on a worker thread so a large document never
//! stalls the editor; while one render runs, only the newest text waits.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;

use gtk::glib;
use gtk::glib::prelude::*;
use gtk::glib::subclass::prelude::*;
use rmpv::Value;

use crate::model::render::MarkdownRenderer;
use crate::services::LinkService;

/// URI schemes a clicked link may open (F4.R7). Anything else is dropped.
const OPENABLE_SCHEMES: &[&str] = &["http", "https", "mailto", "file"];

mod imp {
    use super::*;

    #[derive(Debug, glib::Properties)]
    #[properties(wrapper_type = super::PreviewViewModel)]
    pub struct PreviewViewModel {
        /// Rendered, sanitized HTML for the preview's `<article>`.
        #[property(get)]
        pub(super) html: RefCell<String>,

        /// `file://` URI of the document's directory, ending in `/`, so
        /// relative images resolve next to the file (F4.R6). Empty when
        /// the buffer has no file.
        #[property(get)]
        pub(super) base_uri: RefCell<String>,

        /// Theme colours for the page (CSS custom properties, F5.R2).
        #[property(get, set = Self::set_theme_css, explicit_notify)]
        pub(super) theme_css: RefCell<String>,

        /// Page background (`#rrggbb`) shown before the page has loaded.
        #[property(get, set = Self::set_background, explicit_notify)]
        pub(super) background: RefCell<String>,

        /// 1-based source line the preview should keep in view (F4.R4).
        #[property(get)]
        pub(super) scroll_target_line: Cell<u32>,

        /// Whether the preview is shown. Hidden previews do not render;
        /// the latest text is rendered when it becomes visible again.
        #[property(get, set = Self::set_active, explicit_notify, default = true)]
        pub(super) active: Cell<bool>,

        pub(super) pending: RefCell<Option<String>>,
        pub(super) rendering: Cell<bool>,
        pub(super) links: RefCell<Option<Rc<dyn LinkService>>>,
    }

    impl Default for PreviewViewModel {
        fn default() -> Self {
            Self {
                html: RefCell::default(),
                base_uri: RefCell::default(),
                theme_css: RefCell::default(),
                background: RefCell::new("#282828".into()),
                scroll_target_line: Cell::new(1),
                active: Cell::new(true),
                pending: RefCell::default(),
                rendering: Cell::new(false),
                links: RefCell::default(),
            }
        }
    }

    impl std::fmt::Debug for dyn LinkService {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("LinkService")
        }
    }

    impl PreviewViewModel {
        fn set_theme_css(&self, css: String) {
            if *self.theme_css.borrow() != css {
                self.theme_css.replace(css);
                self.obj().notify_theme_css();
            }
        }

        fn set_background(&self, color: String) {
            if *self.background.borrow() != color {
                self.background.replace(color);
                self.obj().notify_background();
            }
        }

        fn set_active(&self, active: bool) {
            if self.active.replace(active) != active {
                self.obj().notify_active();
                if active {
                    self.obj().render_pending();
                }
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PreviewViewModel {
        const NAME: &'static str = "RingmdPreviewViewModel";
        type Type = super::PreviewViewModel;
    }

    #[glib::derived_properties]
    impl ObjectImpl for PreviewViewModel {}
}

glib::wrapper! {
    /// ViewModel for the preview pane. See `docs/SPEC.md` §3.1.
    pub struct PreviewViewModel(ObjectSubclass<imp::PreviewViewModel>);
}

impl PreviewViewModel {
    pub fn new(links: Rc<dyn LinkService>) -> Self {
        let vm: Self = glib::Object::new();
        vm.imp().links.replace(Some(links));
        vm
    }

    /// Command: new buffer text to show.
    pub fn set_text(&self, text: &str) {
        self.imp().pending.replace(Some(text.to_owned()));
        self.render_pending();
    }

    /// Handle `ringmd_text` from the bundled config: `{buf, text}`.
    pub fn apply_text_event(&self, params: &[Value]) {
        let text = params
            .first()
            .and_then(Value::as_map)
            .and_then(|map| map.iter().find(|(k, _)| k.as_str() == Some("text")))
            .and_then(|(_, v)| v.as_str());
        match text {
            Some(text) => self.set_text(text),
            None => tracing::warn!(?params, "malformed ringmd_text"),
        }
    }

    /// Command: the editor cursor moved to `line` (1-based).
    pub fn set_cursor_line(&self, line: u32) {
        let line = line.max(1);
        if self.imp().scroll_target_line.replace(line) != line {
            self.notify_scroll_target_line();
        }
    }

    /// Command: the document's file changed; relative images now resolve
    /// next to `path` (F4.R6).
    pub fn set_document_path(&self, path: Option<&Path>) {
        let uri = path.and_then(base_uri_for).unwrap_or_default();
        if *self.imp().base_uri.borrow() != uri {
            self.imp().base_uri.replace(uri);
            self.notify_base_uri();
        }
    }

    /// Command: the user clicked a link in the preview (F4.R7).
    pub fn link_activated(&self, uri: &str) {
        let scheme = glib::Uri::peek_scheme(uri).map(|s| s.to_string());
        if !scheme
            .as_deref()
            .is_some_and(|s| OPENABLE_SCHEMES.contains(&s))
        {
            tracing::info!(%uri, "not opening link with unsupported scheme");
            return;
        }
        if let Some(links) = self.imp().links.borrow().as_ref() {
            links.open(uri);
        }
    }

    /// Start rendering the newest pending text, unless a render is already
    /// running (it picks the text up when done) or the preview is hidden.
    fn render_pending(&self) {
        let imp = self.imp();
        if imp.rendering.get() || !imp.active.get() {
            return;
        }
        let Some(text) = imp.pending.take() else {
            return;
        };
        imp.rendering.set(true);

        let (tx, rx) = async_channel::bounded(1);
        let spawned = std::thread::Builder::new()
            .name("markdown-render".into())
            .spawn(move || {
                let _ = tx.send_blocking(MarkdownRenderer::new().render(&text));
            });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "could not start the render thread");
            imp.rendering.set(false);
            return;
        }

        let vm = self.downgrade();
        glib::spawn_future_local(async move {
            let html = rx.recv().await;
            let Some(vm) = vm.upgrade() else { return };
            vm.imp().rendering.set(false);
            if let Ok(html) = html {
                vm.set_html(html);
            }
            vm.render_pending();
        });
    }

    fn set_html(&self, html: String) {
        if *self.imp().html.borrow() != html {
            self.imp().html.replace(html);
            self.notify_html();
        }
    }
}

/// `file:///dir/` for the directory holding `path`.
fn base_uri_for(path: &Path) -> Option<String> {
    let absolute = std::path::absolute(path).ok()?;
    let dir = absolute.parent()?;
    let uri = glib::filename_to_uri(dir, None).ok()?;
    Some(if uri.ends_with('/') {
        uri.to_string()
    } else {
        format!("{uri}/")
    })
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::services::fake::FakeLinkService;

    fn setup() -> (PreviewViewModel, Rc<FakeLinkService>) {
        let links = Rc::new(FakeLinkService::default());
        (PreviewViewModel::new(links.clone()), links)
    }

    /// Run `test` with its own thread-default main context: tests run in
    /// parallel, and render results are delivered on that context.
    fn with_context(test: impl FnOnce()) {
        glib::MainContext::new().with_thread_default(test).unwrap();
    }

    /// Run the main context until `done` holds (renders finish on it).
    fn wait_until(done: impl Fn() -> bool) {
        let context = glib::MainContext::ref_thread_default();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done() {
            assert!(Instant::now() < deadline, "timed out");
            context.iteration(false);
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn text_is_rendered() {
        with_context(|| {
            let (vm, _) = setup();
            vm.set_text("# Hello\n");
            wait_until(|| !vm.html().is_empty());
            assert!(vm.html().contains(">Hello</h1>"), "{}", vm.html());
        });
    }

    #[test]
    fn text_events_are_decoded() {
        with_context(|| {
            let (vm, _) = setup();
            let event = Value::Map(vec![
                ("buf".into(), 1.into()),
                ("text".into(), "*hi*\n".into()),
            ]);
            vm.apply_text_event(&[event]);
            wait_until(|| !vm.html().is_empty());
            assert!(vm.html().contains(">hi</em>"), "{}", vm.html());
        });
    }

    #[test]
    fn only_the_newest_text_wins() {
        with_context(|| {
            let (vm, _) = setup();
            for i in 0..20 {
                vm.set_text(&format!("version {i}\n"));
            }
            wait_until(|| vm.html().contains("version 19"));
            // Let any stray render finish; it must not replace the newest.
            wait_until(|| !vm.imp().rendering.get());
            assert!(vm.html().contains("version 19"), "{}", vm.html());
        });
    }

    #[test]
    fn hidden_preview_renders_when_shown() {
        with_context(|| {
            let (vm, _) = setup();
            vm.set_active(false);
            vm.set_text("# Later\n");
            for _ in 0..50 {
                glib::MainContext::ref_thread_default().iteration(false);
            }
            assert!(vm.html().is_empty());
            vm.set_active(true);
            wait_until(|| vm.html().contains("Later"));
        });
    }

    #[test]
    fn cursor_line_notifies_on_change() {
        let (vm, _) = setup();
        let count = Rc::new(Cell::new(0));
        let c = count.clone();
        vm.connect_scroll_target_line_notify(move |_| c.set(c.get() + 1));
        vm.set_cursor_line(12);
        vm.set_cursor_line(12);
        vm.set_cursor_line(0);
        assert_eq!(vm.scroll_target_line(), 1);
        assert_eq!(count.get(), 2);
    }

    #[test]
    fn base_uri_is_the_document_directory() {
        let (vm, _) = setup();
        assert_eq!(vm.base_uri(), "");
        vm.set_document_path(Some(Path::new("/home/me/my notes/todo.md")));
        assert_eq!(vm.base_uri(), "file:///home/me/my%20notes/");
        vm.set_document_path(None);
        assert_eq!(vm.base_uri(), "");
    }

    #[test]
    fn links_open_only_for_safe_schemes() {
        let (vm, links) = setup();
        vm.link_activated("https://example.org/a");
        vm.link_activated("mailto:me@example.org");
        vm.link_activated("javascript:alert(1)");
        vm.link_activated("data:text/html,x");
        vm.link_activated("not a uri");
        assert_eq!(
            *links.opened.borrow(),
            ["https://example.org/a", "mailto:me@example.org"]
        );
    }
}
