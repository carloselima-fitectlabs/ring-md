//! In-memory service fakes for headless ViewModel tests (rule A6).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use super::{
    ClipboardService, FileDialogService, LinkService, LocalBoxFuture, PdfOptions, PdfService,
    ServiceError, SettingsStore,
};
use crate::model::config::Setting;

/// One recorded call to [`FakePdfService::export`].
#[derive(Debug, Clone, PartialEq)]
pub struct PdfExportCall {
    pub html: String,
    pub base_uri: Option<String>,
    pub options: PdfOptions,
    pub output: PathBuf,
}

/// Records export calls and returns a configurable result.
#[derive(Debug, Default)]
pub struct FakePdfService {
    pub calls: RefCell<Vec<PdfExportCall>>,
    pub next_result: RefCell<Option<ServiceError>>,
}

impl FakePdfService {
    /// Make the next export fail with `error`.
    pub fn fail_next(&self, error: ServiceError) {
        *self.next_result.borrow_mut() = Some(error);
    }
}

impl PdfService for FakePdfService {
    fn export<'a>(
        &'a self,
        html: &'a str,
        base_uri: Option<&'a str>,
        options: &'a PdfOptions,
        output: &'a Path,
    ) -> LocalBoxFuture<'a, Result<(), ServiceError>> {
        self.calls.borrow_mut().push(PdfExportCall {
            html: html.to_owned(),
            base_uri: base_uri.map(Into::into),
            options: options.clone(),
            output: output.to_owned(),
        });
        let result = self.next_result.borrow_mut().take();
        Box::pin(async move { result.map_or(Ok(()), Err) })
    }
}

/// A clipboard that lives in memory.
#[derive(Debug, Default)]
pub struct FakeClipboardService {
    pub text: RefCell<Option<String>>,
}

impl ClipboardService for FakeClipboardService {
    fn read_text(&self) -> LocalBoxFuture<'_, Result<Option<String>, ServiceError>> {
        let text = self.text.borrow().clone();
        Box::pin(async move { Ok(text) })
    }

    fn set_text(&self, text: &str) {
        *self.text.borrow_mut() = Some(text.to_owned());
    }
}

/// Returns queued answers; an empty queue behaves like the user cancelling.
#[derive(Debug, Default)]
pub struct FakeFileDialogService {
    pub open_answers: RefCell<VecDeque<PathBuf>>,
    pub save_answers: RefCell<VecDeque<PathBuf>>,
    pub save_suggestions: RefCell<Vec<String>>,
}

impl FileDialogService for FakeFileDialogService {
    fn pick_open(&self) -> LocalBoxFuture<'_, Option<PathBuf>> {
        let answer = self.open_answers.borrow_mut().pop_front();
        Box::pin(async move { answer })
    }

    fn pick_save<'a>(&'a self, suggested_name: &'a str) -> LocalBoxFuture<'a, Option<PathBuf>> {
        self.save_suggestions
            .borrow_mut()
            .push(suggested_name.to_owned());
        let answer = self.save_answers.borrow_mut().pop_front();
        Box::pin(async move { answer })
    }
}

/// Records opened URIs.
#[derive(Debug, Default)]
pub struct FakeLinkService {
    pub opened: RefCell<Vec<String>>,
}

impl LinkService for FakeLinkService {
    fn open(&self, uri: &str) {
        self.opened.borrow_mut().push(uri.to_owned());
    }
}

/// Records saved settings.
#[derive(Debug, Default)]
pub struct FakeSettingsStore {
    pub saved: RefCell<Vec<Setting>>,
}

impl SettingsStore for FakeSettingsStore {
    fn save(&self, setting: &Setting) {
        self.saved.borrow_mut().push(setting.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk::glib;

    /// Tests run on parallel threads, so each one uses its own main context.
    fn block_on<T>(future: LocalBoxFuture<'_, T>) -> T {
        glib::MainContext::new().block_on(future)
    }

    #[test]
    fn pdf_fake_records_calls_and_fails_on_demand() {
        let pdf = FakePdfService::default();
        let options = PdfOptions::default();
        assert_eq!(
            block_on(pdf.export("<p>x</p>", None, &options, Path::new("a.pdf"))),
            Ok(())
        );

        pdf.fail_next(ServiceError::Io("disk full".into()));
        assert_eq!(
            block_on(pdf.export("<p>y</p>", None, &options, Path::new("b.pdf"))),
            Err(ServiceError::Io("disk full".into()))
        );
        assert_eq!(pdf.calls.borrow().len(), 2);
        assert_eq!(pdf.calls.borrow()[1].output, PathBuf::from("b.pdf"));
    }

    #[test]
    fn clipboard_fake_round_trips() {
        let clipboard = FakeClipboardService::default();
        assert_eq!(block_on(clipboard.read_text()), Ok(None));
        clipboard.set_text("hello");
        assert_eq!(block_on(clipboard.read_text()), Ok(Some("hello".into())));
    }

    #[test]
    fn file_dialog_fake_cancels_when_empty() {
        let dialogs = FakeFileDialogService::default();
        assert_eq!(block_on(dialogs.pick_open()), None);
        dialogs.save_answers.borrow_mut().push_back("out.md".into());
        assert_eq!(
            block_on(dialogs.pick_save("Untitled.md")),
            Some("out.md".into())
        );
        assert_eq!(*dialogs.save_suggestions.borrow(), vec!["Untitled.md"]);
    }
}
