//! Enforces the MVVM layering rules from `docs/SPEC.md` §3.2.
//!
//! - A4: Model code does not use GTK (only `glib` re-exported through `gtk`).
//! - A2/A6: ViewModels do not use GTK widgets (only `glib`), so they stay
//!   testable without a display.
//! - A1: Model and ViewModel never reach up into `view`.

#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};

fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(rust_files(&path));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
    files
}

/// Returns `file:line: text` for every line in `layer` matching a forbidden pattern.
fn violations(layer: &str, forbidden: &[&str]) -> Vec<String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(layer);
    let mut found = Vec::new();
    for file in rust_files(&dir) {
        let source = fs::read_to_string(&file).unwrap();
        for (number, line) in source.lines().enumerate() {
            let code = line.split("//").next().unwrap_or_default();
            if forbidden.iter().any(|pattern| code.contains(pattern)) {
                found.push(format!("{}:{}: {}", file.display(), number + 1, line.trim()));
            }
        }
    }
    found
}

/// GTK paths other than `gtk::glib` are UI toolkit code.
const GTK_UI: &[&str] = &[
    "gtk::prelude",
    "gtk::Widget",
    "gtk::Window",
    "gtk::Application",
    "gtk::gdk",
    "gtk::gio",
    "gtk::pango",
    "gtk::cairo",
    "gtk::{",
    "use gtk;",
];

#[test]
fn model_has_no_gtk_or_upward_dependencies() {
    let mut forbidden = GTK_UI.to_vec();
    forbidden.extend(["crate::view", "crate::viewmodel"]);
    let found = violations("model", &forbidden);
    assert!(found.is_empty(), "Model layer violations:\n{}", found.join("\n"));
}

#[test]
fn viewmodels_use_glib_only() {
    let mut forbidden = GTK_UI.to_vec();
    forbidden.push("crate::view::");
    let found = violations("viewmodel", &forbidden);
    assert!(found.is_empty(), "ViewModel layer violations:\n{}", found.join("\n"));
}

#[test]
fn services_do_not_depend_on_view_layers() {
    let found = violations("services", &["crate::view", "crate::viewmodel"]);
    assert!(found.is_empty(), "Service layer violations:\n{}", found.join("\n"));
}
