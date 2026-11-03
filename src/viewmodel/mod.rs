//! ViewModel layer: observable state (GObject properties) and commands.
//!
//! ViewModels depend on `glib` only — never on GTK widgets — so they can be
//! tested without a display (rules A2, A6). Planned: `ExportViewModel` (M5).

pub mod app;
pub mod editor;
pub mod preview;
pub mod session;
pub mod theme;

pub use app::AppViewModel;
pub use editor::{EditorServices, EditorViewModel};
pub use preview::PreviewViewModel;
pub use theme::ThemeViewModel;
