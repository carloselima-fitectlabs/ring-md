//! Modal dialogs shown by Views.

use gtk::prelude::*;

/// Show a modal error dialog with a short `title` and a longer `detail`.
pub fn show_error(parent: Option<&impl IsA<gtk::Window>>, title: &str, detail: &str) {
    gtk::AlertDialog::builder()
        .modal(true)
        .message(title)
        .detail(detail)
        .buttons(["Close"])
        .build()
        .show(parent);
}
