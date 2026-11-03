//! Model layer: domain state and logic. No GTK widget types live here (rule A4).
//!
//! Planned modules (see `docs/SPEC.md` §3.1): `nvim` (M1), `grid` (M1),
//! `render` (M3), `theme` (M4), `pdf` (M5), `config` (M6).

pub mod document;

pub use document::Document;
