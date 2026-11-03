//! GTK application lifecycle (F1.R1).

use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, gio, glib};

use crate::cli::Cli;
use crate::model::Document;
use crate::model::config::{self, Config};
use crate::model::nvim::{SpawnOptions, runtime};
use crate::services::gtk::{
    GtkClipboardService, GtkFileDialogService, GtkLinkService, SystemColorScheme,
};
use crate::services::webkit::WebKitPdfService;
use crate::services::{FileSettingsStore, PdfOptions};
use crate::view::MainWindow;
use crate::viewmodel::session::{self, Targets};
use crate::viewmodel::{
    AppViewModel, ExportServices, ExportViewModel, PreviewViewModel, ThemeViewModel,
};

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

    let config_path = config::default_path();
    let (config, warnings) = Config::load(&config_path);
    for warning in &warnings {
        tracing::warn!(path = %config_path.display(), "{warning}");
    }

    let mut options = SpawnOptions::new(runtime::init_lua());
    options.file = cli.file.clone();
    let document = cli.file.map(Document::with_path).unwrap_or_default();
    app.connect_activate(move |app| {
        tracing::debug!(?document, "activating");
        let Some(display) = gdk::Display::default() else {
            tracing::error!("no display available");
            return;
        };
        let vm = AppViewModel::new(document.clone());
        let clipboard = Rc::new(GtkClipboardService::new(&display));
        let preview = PreviewViewModel::new(Rc::new(GtkLinkService::default()));
        let theme = ThemeViewModel::new(
            config.theme,
            Rc::new(FileSettingsStore::new(config_path.clone())),
        );
        theme
            .bind_property("preview-css", &preview, "theme-css")
            .sync_create()
            .build();
        theme
            .bind_property("background", &preview, "background")
            .sync_create()
            .build();
        SystemColorScheme::watch(glib::clone!(
            #[weak]
            theme,
            move |dark| theme.set_system_prefers_dark(dark)
        ));
        let dialogs = Rc::new(GtkFileDialogService::default());
        let export = ExportViewModel::new(ExportServices {
            pdf: Rc::new(WebKitPdfService),
            dialogs: dialogs.clone(),
            options: PdfOptions {
                page_size: config.pdf_page_size,
                margin_mm: config.pdf_margin_mm,
            },
        });
        let targets = Targets {
            app: vm,
            preview,
            theme,
            export,
        };
        let editor = session::start(&targets, &options, clipboard);
        let window = MainWindow::new(app, &targets, &editor);
        dialogs.set_parent(window.window());
        window.present();
    });

    // Arguments were already parsed by clap; don't let GTK parse them again.
    app.run_with_args(&[env!("CARGO_PKG_NAME")])
}
