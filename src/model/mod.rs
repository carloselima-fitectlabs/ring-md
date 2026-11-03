//! Model layer: domain state and logic. No GTK widget types live here (rule A4).
//!
//! Planned modules (see `docs/SPEC.md` §3.1): `render` (M3), `theme` (M4),
//! `pdf` (M5), `config` (M6).

pub mod clipboard;
pub mod document;
pub mod grid;
pub mod nvim;

pub use document::Document;
pub use grid::Grid;
