//! GTK application lifecycle (F1.R1).

use gtk::prelude::*;
use gtk::{gio, glib};

use crate::cli::Cli;
use crate::model::Document;
use crate::view::MainWindow;
use crate::viewmodel::AppViewModel;

/// Application ID. Provisional until the Flatpak ID is chosen (T7.2).
pub const APP_ID: &str = "io.github.ringmd.RingMD";

/// Build and run the application. Returns the process exit code.
pub fn run(cli: Cli) -> glib::ExitCode {
    // Each window drives its own Neovim process, so every launch is its own
    // instance instead of forwarding to an already-running one.
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();

    let document = cli.file.map(Document::with_path).unwrap_or_default();
    app.connect_activate(move |app| {
        tracing::debug!(?document, "activating");
        let vm = AppViewModel::new(document.clone());
        MainWindow::new(app, vm).present();
    });

    // Arguments were already parsed by clap; don't let GTK parse them again.
    app.run_with_args(&[env!("CARGO_PKG_NAME")])
}
