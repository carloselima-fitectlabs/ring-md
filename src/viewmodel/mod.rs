//! ViewModel layer: observable state (GObject properties) and commands.
//!
//! ViewModels depend on `glib` only — never on GTK widgets — so they can be
//! tested without a display (rules A2, A6). Planned: `ThemeViewModel` (M4), `ExportViewModel` (M5).

pub mod app;
pub mod editor;
pub mod preview;
pub mod session;

pub use app::AppViewModel;
pub use editor::{EditorServices, EditorViewModel};
pub use preview::PreviewViewModel;
