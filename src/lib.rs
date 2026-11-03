//! RingMD — a graphical Markdown editor powered by an embedded Neovim engine.
//!
//! The code follows MVVM (see `docs/SPEC.md` §3): [`view`] → [`viewmodel`] →
//! [`model`], with platform access behind the traits in [`services`].

pub mod app;
pub mod cli;
pub mod logging;
pub mod model;
pub mod services;
pub mod view;
pub mod viewmodel;
