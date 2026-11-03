//! View layer: GTK widgets that draw ViewModel state and forward input.
//! Views contain no decisions (rule A3). Planned: `grid_view` (M1),
//! `preview_view` (M3).

pub mod dialogs;
pub mod window;

pub use window::MainWindow;
