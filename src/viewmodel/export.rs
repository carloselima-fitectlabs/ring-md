//! PDF export flow (T5.3, T5.4): choose the output path, fetch the buffer
//! from Neovim, render it with the print stylesheet and hand it to the PDF
//! service, without blocking editing (F6.R6).

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::OnceLock;

use gtk::glib;
use gtk::glib::prelude::*;
use gtk::glib::subclass::prelude::*;

use crate::model::nvim::NvimApi;
use crate::model::render::{self, MarkdownRenderer};
use crate::services::{FileDialogService, PdfOptions, PdfService};

/// Signal emitted when an export ends: `(success: bool, message: String)`.
pub const SIGNAL_FINISHED: &str = "finished";

/// Services the export needs.
pub struct ExportServices {
    pub pdf: Rc<dyn PdfService>,
    pub dialogs: Rc<dyn FileDialogService>,
    pub options: PdfOptions,
}

impl std::fmt::Debug for ExportServices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExportServices")
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

mod imp {
    use gtk::glib::subclass::Signal;

    use super::*;

    #[derive(Default, glib::Properties)]
    #[properties(wrapper_type = super::ExportViewModel)]
    pub struct ExportViewModel {
        /// Whether a PDF is being written (after any file chooser).
        #[property(get)]
        pub(super) is_exporting: Cell<bool>,

        /// Whether an export (including its file chooser) is in progress.
        pub(super) busy: Cell<bool>,

        pub(super) services: RefCell<Option<ExportServices>>,
        pub(super) nvim: RefCell<Option<Rc<dyn NvimApi>>>,
    }

    impl std::fmt::Debug for ExportViewModel {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("ExportViewModel")
                .field("is_exporting", &self.is_exporting.get())
                .finish_non_exhaustive()
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ExportViewModel {
        const NAME: &'static str = "RingmdExportViewModel";
        type Type = super::ExportViewModel;
    }

    #[glib::derived_properties]
    impl ObjectImpl for ExportViewModel {
        fn signals() -> &'static [Signal] {
            static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| {
                vec![
                    Signal::builder(SIGNAL_FINISHED)
                        .param_types([bool::static_type(), String::static_type()])
                        .build(),
                ]
            })
        }
    }
}

glib::wrapper! {
    /// ViewModel for PDF export. See `docs/SPEC.md` §3.1.
    pub struct ExportViewModel(ObjectSubclass<imp::ExportViewModel>);
}

/// Where a PDF goes when no path is given (F6.R1): next to the document,
/// same name with `.pdf`. `None` for an unnamed buffer.
pub fn default_output(document: Option<&Path>) -> Option<PathBuf> {
    document.map(|path| path.with_extension("pdf"))
}

impl ExportViewModel {
    pub fn new(services: ExportServices) -> Self {
        let vm: Self = glib::Object::new();
        vm.imp().services.replace(Some(services));
        vm
    }

    /// The Neovim session to read the buffer from.
    pub fn attach_neovim(&self, nvim: Rc<dyn NvimApi>) {
        self.imp().nvim.replace(Some(nvim));
    }

    /// Subscribe to finished exports.
    pub fn connect_finished<F: Fn(&Self, bool, &str) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            SIGNAL_FINISHED,
            false,
            glib::closure_local!(move |vm: &Self, ok: bool, message: &str| f(vm, ok, message)),
        )
    }

    /// Command: `:ExportPdf [path]` / `Ctrl+E`. `requested` is the path the
    /// user typed; `document` is the open file, if any.
    pub async fn export(&self, requested: Option<PathBuf>, document: Option<PathBuf>) {
        if self.imp().busy.replace(true) {
            self.finish(false, "An export is already running".into());
            return;
        }
        let result = self.run(requested, document.as_deref()).await;
        self.set_exporting(false);
        self.imp().busy.set(false);
        match result {
            Ok(Some(path)) => self.finish(true, format!("Exported to {}", path.display())),
            Ok(None) => tracing::debug!("export cancelled"),
            Err(message) => self.finish(false, format!("PDF export failed: {message}")),
        }
    }

    /// `Ok(None)` when the user cancelled the file chooser.
    async fn run(
        &self,
        requested: Option<PathBuf>,
        document: Option<&Path>,
    ) -> Result<Option<PathBuf>, String> {
        let (pdf, dialogs, options) = {
            let services = self.imp().services.borrow();
            let services = services.as_ref().ok_or("export is not set up")?;
            (
                services.pdf.clone(),
                services.dialogs.clone(),
                services.options.clone(),
            )
        };
        let output = match requested.or_else(|| default_output(document)) {
            Some(path) => path,
            None => match dialogs.pick_save("Untitled.pdf").await {
                Some(path) => path,
                None => return Ok(None),
            },
        };

        self.set_exporting(true);
        let markdown = self.buffer_text().await?;
        let page = render::pdf_page(&MarkdownRenderer::new().render(&markdown));
        let base = document.and_then(render::base_uri_for);
        pdf.export(&page, base.as_deref(), &options, &output)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Some(output))
    }

    /// The current buffer's text, straight from Neovim.
    async fn buffer_text(&self) -> Result<String, String> {
        let nvim = self
            .imp()
            .nvim
            .borrow()
            .clone()
            .ok_or("the editor is not running")?;
        let lines = nvim
            .request(
                "nvim_buf_get_lines",
                vec![0.into(), 0.into(), (-1).into(), false.into()],
            )
            .await
            .map_err(|e| e.to_string())?;
        let lines = lines
            .as_array()
            .ok_or("unexpected reply from Neovim")?
            .iter()
            .map(|line| line.as_str().unwrap_or_default())
            .collect::<Vec<_>>();
        Ok(lines.join("\n") + "\n")
    }

    fn set_exporting(&self, exporting: bool) {
        if self.imp().is_exporting.replace(exporting) != exporting {
            self.notify_is_exporting();
        }
    }

    fn finish(&self, ok: bool, message: String) {
        if ok {
            tracing::info!("{message}");
        } else {
            tracing::warn!("{message}");
        }
        self.emit_by_name::<()>(SIGNAL_FINISHED, &[&ok, &message]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::nvim::fake::FakeNvim;
    use crate::services::ServiceError;
    use crate::services::fake::{FakeFileDialogService, FakePdfService};
    use rmpv::Value;

    /// Lines as `nvim_buf_get_lines` returns them.
    fn lines_value(lines: &[&str]) -> Value {
        Value::Array(lines.iter().map(|&l| l.into()).collect())
    }

    struct Setup {
        vm: ExportViewModel,
        pdf: Rc<FakePdfService>,
        dialogs: Rc<FakeFileDialogService>,
        results: Rc<RefCell<Vec<(bool, String)>>>,
    }

    fn setup() -> Setup {
        let pdf = Rc::new(FakePdfService::default());
        let dialogs = Rc::new(FakeFileDialogService::default());
        let vm = ExportViewModel::new(ExportServices {
            pdf: pdf.clone(),
            dialogs: dialogs.clone(),
            options: PdfOptions::default(),
        });
        let nvim = Rc::new(FakeNvim::new());
        nvim.set_response(lines_value(&["# Title", "", "text"]));
        vm.attach_neovim(nvim);
        let results = Rc::new(RefCell::new(Vec::new()));
        let sink = results.clone();
        vm.connect_finished(move |_, ok, message| sink.borrow_mut().push((ok, message.into())));
        Setup {
            vm,
            pdf,
            dialogs,
            results,
        }
    }

    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        glib::MainContext::new().block_on(future)
    }

    #[test]
    fn default_output_sits_next_to_the_document() {
        assert_eq!(
            default_output(Some(Path::new("/notes/todo.md"))),
            Some(PathBuf::from("/notes/todo.pdf"))
        );
        assert_eq!(default_output(None), None);
    }

    #[test]
    fn exports_the_buffer_next_to_the_file() {
        let s = setup();
        block_on(s.vm.export(None, Some("/notes/todo.md".into())));
        let calls = s.pdf.calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].output, PathBuf::from("/notes/todo.pdf"));
        assert_eq!(calls[0].base_uri.as_deref(), Some("file:///notes/"));
        assert!(calls[0].html.contains(">Title</h1>"), "{}", calls[0].html);
        assert!(calls[0].html.contains("background: #ffffff"));
        assert_eq!(
            *s.results.borrow(),
            [(true, "Exported to /notes/todo.pdf".to_owned())]
        );
        assert!(!s.vm.is_exporting());
    }

    #[test]
    fn requested_path_wins() {
        let s = setup();
        block_on(s.vm.export(Some("/tmp/out.pdf".into()), Some("/notes/todo.md".into())));
        assert_eq!(
            s.pdf.calls.borrow()[0].output,
            PathBuf::from("/tmp/out.pdf")
        );
    }

    #[test]
    fn unnamed_buffer_asks_for_a_path() {
        let s = setup();
        s.dialogs
            .save_answers
            .borrow_mut()
            .push_back("/home/me/doc.pdf".into());
        block_on(s.vm.export(None, None));
        assert_eq!(
            s.pdf.calls.borrow()[0].output,
            PathBuf::from("/home/me/doc.pdf")
        );
        assert_eq!(*s.dialogs.save_suggestions.borrow(), ["Untitled.pdf"]);

        // Cancelling the chooser (no answer) exports and reports nothing.
        block_on(s.vm.export(None, None));
        assert_eq!(s.pdf.calls.borrow().len(), 1);
        assert_eq!(s.results.borrow().len(), 1);
    }

    #[test]
    fn failures_are_reported() {
        let s = setup();
        s.pdf.fail_next(ServiceError::Io("disk full".into()));
        block_on(s.vm.export(Some("/tmp/x.pdf".into()), None));
        assert_eq!(
            *s.results.borrow(),
            [(false, "PDF export failed: I/O error: disk full".to_owned())]
        );
    }
}
