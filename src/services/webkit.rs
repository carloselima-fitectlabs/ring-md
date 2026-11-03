//! PDF export through WebKitGTK (T5.2, F6.R7): the page is loaded in an
//! offscreen WebView and printed with GTK's "Print to File" backend, so no
//! external tool is needed and links stay clickable (F6.R5).

use std::path::Path;
use std::time::Duration;

use gtk::glib;
use webkit6::prelude::*;

use super::{LocalBoxFuture, PageSize, PdfOptions, PdfService, ServiceError};

/// How long the page may take to load (remote images included).
const LOAD_TIMEOUT: Duration = Duration::from_secs(30);
/// How long printing may take.
const PRINT_TIMEOUT: Duration = Duration::from_secs(120);

/// The real [`PdfService`].
#[derive(Debug, Default, Clone, Copy)]
pub struct WebKitPdfService;

impl PdfService for WebKitPdfService {
    fn export<'a>(
        &'a self,
        html: &'a str,
        base_uri: Option<&'a str>,
        options: &'a PdfOptions,
        output: &'a Path,
    ) -> LocalBoxFuture<'a, Result<(), ServiceError>> {
        Box::pin(export(html, base_uri, options, output))
    }
}

async fn export(
    html: &str,
    base_uri: Option<&str>,
    options: &PdfOptions,
    output: &Path,
) -> Result<(), ServiceError> {
    let output = std::path::absolute(output).map_err(|e| ServiceError::Io(e.to_string()))?;
    let output_uri = glib::filename_to_uri(&output, None)
        .map_err(|e| ServiceError::Other(format!("bad output path: {e}")))?;

    let web_view = webkit6::WebView::new();
    if let Some(settings) = WebViewExt::settings(&web_view) {
        settings.set_enable_javascript_markup(false);
        settings.set_print_backgrounds(true);
    }

    // Load the page and wait until it (and its images) finished loading.
    let (loaded_tx, loaded_rx) = async_channel::bounded::<Result<(), String>>(1);
    let failed_tx = loaded_tx.clone();
    web_view.connect_load_changed(move |_, event| {
        if event == webkit6::LoadEvent::Finished {
            let _ = loaded_tx.try_send(Ok(()));
        }
    });
    web_view.connect_load_failed(move |_, _, uri, error| {
        let _ = failed_tx.try_send(Err(format!("{uri}: {error}")));
        false
    });
    web_view.load_html(html, base_uri);
    match glib::future_with_timeout(LOAD_TIMEOUT, loaded_rx.recv()).await {
        Ok(Ok(Ok(()))) => {}
        Ok(Ok(Err(e))) => return Err(ServiceError::Other(format!("could not load the page: {e}"))),
        Ok(Err(_)) => return Err(ServiceError::Other("the page never loaded".into())),
        Err(_) => return Err(ServiceError::Other("timed out loading the page".into())),
    }

    let operation = webkit6::PrintOperation::new(&web_view);
    operation.set_print_settings(&print_settings(options, &output_uri));
    operation.set_page_setup(&page_setup(options));

    let (done_tx, done_rx) = async_channel::bounded::<Result<(), String>>(1);
    let failed_tx = done_tx.clone();
    operation.connect_failed(move |_, error| {
        let _ = failed_tx.try_send(Err(error.to_string()));
    });
    operation.connect_finished(move |_| {
        // `finished` also follows `failed`; the first message wins.
        let _ = done_tx.try_send(Ok(()));
    });
    operation.print();

    let result = match glib::future_with_timeout(PRINT_TIMEOUT, done_rx.recv()).await {
        Ok(Ok(Ok(()))) => Ok(()),
        Ok(Ok(Err(e))) => Err(ServiceError::Other(e)),
        Ok(Err(_)) => Err(ServiceError::Other("printing stopped unexpectedly".into())),
        Err(_) => Err(ServiceError::Other("timed out printing".into())),
    };
    // The print backend writes the file asynchronously; make sure it exists.
    result.and_then(|()| match std::fs::metadata(&output) {
        Ok(meta) if meta.len() > 0 => Ok(()),
        _ => Err(ServiceError::Io(format!(
            "{} was not written",
            output.display()
        ))),
    })
}

fn paper_size(size: PageSize) -> gtk::PaperSize {
    let name = match size {
        PageSize::A4 => gtk::PAPER_NAME_A4,
        PageSize::Letter => gtk::PAPER_NAME_LETTER,
    };
    gtk::PaperSize::new(Some(name))
}

fn print_settings(options: &PdfOptions, output_uri: &str) -> gtk::PrintSettings {
    let settings = gtk::PrintSettings::new();
    settings.set_printer("Print to File");
    settings.set(gtk::PRINT_SETTINGS_OUTPUT_FILE_FORMAT, Some("pdf"));
    settings.set(gtk::PRINT_SETTINGS_OUTPUT_URI, Some(output_uri));
    settings.set_paper_size(&paper_size(options.page_size));
    settings
}

fn page_setup(options: &PdfOptions) -> gtk::PageSetup {
    let setup = gtk::PageSetup::new();
    setup.set_paper_size(&paper_size(options.page_size));
    let margin = options.margin_mm;
    setup.set_top_margin(margin, gtk::Unit::Mm);
    setup.set_bottom_margin(margin, gtk::Unit::Mm);
    setup.set_left_margin(margin, gtk::Unit::Mm);
    setup.set_right_margin(margin, gtk::Unit::Mm);
    setup
}
