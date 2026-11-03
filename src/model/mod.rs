//! Model layer: domain state and logic. No GTK widget types live here (rule A4).
//!
//! Planned modules (see `docs/SPEC.md` §3.1): `pdf` (M5).

pub mod clipboard;
pub mod config;
pub mod document;
pub mod grid;
pub mod nvim;
pub mod render;
pub mod theme;

pub use document::Document;
pub use grid::Grid;
