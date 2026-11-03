//! Platform services reached through traits (rule A4).
//!
//! ViewModels depend on these traits, never on concrete GTK/WebKit types, so
//! they can be unit-tested headless with the fakes in [`fake`]. Real
//! implementations live in [`gtk`] as the milestones need them: clipboard
//! (T1.9), links (T3.6); file dialogs (T6.3) and PDF (T5.2) follow.

pub mod fake;
pub mod gtk;

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

/// Futures returned by services. They run on the GLib main context, so they
/// do not need to be `Send`.
pub type LocalBoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// Errors any platform service can report.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ServiceError {
    #[error("operation cancelled")]
    Cancelled,
    #[error("I/O error: {0}")]
    Io(String),
    #[error("{0}")]
    Other(String),
}

pub use crate::model::config::PageSize;

/// Options for a PDF export (F6.R3).
#[derive(Debug, Clone, PartialEq)]
pub struct PdfOptions {
    pub page_size: PageSize,
    pub margin_mm: f64,
}

impl Default for PdfOptions {
    fn default() -> Self {
        Self {
            page_size: PageSize::A4,
            margin_mm: 20.0,
        }
    }
}

/// Turns print-ready HTML into a PDF file (F6).
pub trait PdfService {
    fn export<'a>(
        &'a self,
        html: &'a str,
        options: &'a PdfOptions,
        output: &'a Path,
    ) -> LocalBoxFuture<'a, Result<(), ServiceError>>;
}

/// System clipboard access backing Neovim's `"+` / `"*` registers (F2.R8).
pub trait ClipboardService {
    fn read_text(&self) -> LocalBoxFuture<'_, Result<Option<String>, ServiceError>>;
    fn set_text(&self, text: &str);
}

/// Native open/save dialogs (F7.R2). `None` means the user cancelled.
pub trait FileDialogService {
    fn pick_open(&self) -> LocalBoxFuture<'_, Option<PathBuf>>;
    fn pick_save<'a>(&'a self, suggested_name: &'a str) -> LocalBoxFuture<'a, Option<PathBuf>>;
}

/// Opens URIs in the user's default application, e.g. links clicked in the
/// preview go to the system browser (F4.R7).
pub trait LinkService {
    fn open(&self, uri: &str);
}

/// Persists settings changed from inside the app (F9.R4).
pub trait SettingsStore {
    fn save(&self, setting: &crate::model::config::Setting);
}

/// Saves settings into `config.toml`, keeping the rest of the file.
#[derive(Debug, Clone)]
pub struct FileSettingsStore {
    path: PathBuf,
}

impl FileSettingsStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl SettingsStore for FileSettingsStore {
    fn save(&self, setting: &crate::model::config::Setting) {
        match crate::model::config::save_setting(&self.path, setting) {
            Ok(()) => tracing::debug!(?setting, path = %self.path.display(), "saved setting"),
            Err(e) => tracing::warn!(error = %e, "could not save setting"),
        }
    }
}
