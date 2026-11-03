//! View layer: GTK widgets that draw ViewModel state and forward input.
//! Views contain no decisions (rule A3).

pub mod dialogs;
pub mod grid_view;
pub mod preview_view;
pub mod window;

pub use window::MainWindow;
