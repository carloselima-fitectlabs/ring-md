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

/// Ask what to do with unsaved changes before closing (F1.R5). Escape and
/// closing the dialog mean Cancel.
pub async fn confirm_close(
    parent: Option<&impl IsA<gtk::Window>>,
    unsaved: &str,
) -> crate::viewmodel::files::CloseChoice {
    use crate::viewmodel::files::CloseChoice;
    let dialog = gtk::AlertDialog::builder()
        .modal(true)
        .message("Save changes before closing?")
        .detail(format!("Unsaved: {unsaved}"))
        .buttons(["Cancel", "Discard", "Save"])
        .cancel_button(0)
        .default_button(2)
        .build();
    match dialog.choose_future(parent).await {
        Ok(2) => CloseChoice::Save,
        Ok(1) => CloseChoice::Discard,
        _ => CloseChoice::Cancel,
    }
}

/// The About dialog (F1.R7).
pub fn show_about(parent: Option<&impl IsA<gtk::Window>>) {
    let about = gtk::AboutDialog::builder()
        .program_name("RingMD")
        .version(env!("CARGO_PKG_VERSION"))
        .comments(env!("CARGO_PKG_DESCRIPTION"))
        .website(env!("CARGO_PKG_REPOSITORY"))
        .license_type(gtk::License::MitX11)
        .modal(true)
        .build();
    if let Some(parent) = parent {
        about.set_transient_for(Some(parent));
    }
    about.present();
}
