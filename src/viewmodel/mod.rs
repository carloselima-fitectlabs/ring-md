//! ViewModel layer: observable state (GObject properties) and commands.
//!
//! ViewModels depend on `glib` only — never on GTK widgets — so they can be
//! tested without a display (rules A2, A6). Planned: `EditorViewModel` (M1),
//! `PreviewViewModel` (M3), `ThemeViewModel` (M4), `ExportViewModel` (M5).

pub mod app;

pub use app::AppViewModel;
