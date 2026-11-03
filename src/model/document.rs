//! The document currently being edited.
//!
//! In later milestones this mirrors the Neovim buffer (path and modified flag);
//! Neovim stays the source of truth for the text itself.

use std::path::{Path, PathBuf};

/// Name shown for a buffer that has no file yet (matches Neovim's wording).
pub const UNNAMED: &str = "[No Name]";

/// File-level state of the current document.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Document {
    path: Option<PathBuf>,
    modified: bool,
}

impl Document {
    /// An unnamed, unmodified document.
    pub fn new() -> Self {
        Self::default()
    }

    /// A document backed by `path`. The file does not need to exist yet,
    /// following Vim semantics (`ringmd new.md` creates it on `:w`).
    pub fn with_path(path: impl Into<PathBuf>) -> Self {
        Self {
            path: Some(path.into()),
            modified: false,
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn set_path(&mut self, path: Option<PathBuf>) {
        self.path = path;
    }

    pub fn is_modified(&self) -> bool {
        self.modified
    }

    pub fn set_modified(&mut self, modified: bool) {
        self.modified = modified;
    }

    /// Short name for display: the file name, or [`UNNAMED`].
    pub fn display_name(&self) -> String {
        self.path
            .as_deref()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| UNNAMED.to_owned())
    }

    /// Window title as required by F1.R4: `<filename>[+] — RingMD`.
    pub fn window_title(&self) -> String {
        let marker = if self.modified { "+" } else { "" };
        format!("{}{marker} — RingMD", self.display_name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unnamed_document_title() {
        assert_eq!(Document::new().window_title(), "[No Name] — RingMD");
    }

    #[test]
    fn named_document_uses_file_name_only() {
        let doc = Document::with_path("/home/me/notes/todo.md");
        assert_eq!(doc.display_name(), "todo.md");
        assert_eq!(doc.window_title(), "todo.md — RingMD");
    }

    #[test]
    fn modified_marker() {
        let mut doc = Document::with_path("a.md");
        doc.set_modified(true);
        assert_eq!(doc.window_title(), "a.md+ — RingMD");
    }

    #[test]
    fn path_ending_in_dotdot_falls_back_to_unnamed() {
        assert_eq!(Document::with_path("..").display_name(), UNNAMED);
    }
}
