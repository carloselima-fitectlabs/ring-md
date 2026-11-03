# RingMD

A graphical, Linux-first Markdown editor powered by an embedded Neovim engine,
with live preview, Gruvbox themes and PDF export.

> **Status:** early development. The embedded Neovim editor and its bundled
> config work (M0–M2); live preview (M3) is next.
> See [docs/SPEC.md](docs/SPEC.md) for the specification and task breakdown.

## Building

Requirements: Rust 1.92+, GTK 4.14+ development files, and
[Neovim](https://neovim.io) 0.10+ on `PATH` at run time (Ubuntu 24.04's
`neovim` package is 0.9, so use the upstream release there).

```bash
sudo apt install libgtk-4-dev        # Debian / Ubuntu 24.04+
sudo dnf install gtk4-devel          # Fedora
```

```bash
cargo run -- notes.md
```

Set `RINGMD_LOG=debug` for verbose logs. RingMD starts Neovim with its own
`runtime/init.lua` (override the directory with `RINGMD_RUNTIME`) and
`NVIM_APPNAME=ringmd`: your `~/.config/nvim`, plugins and shada are not
loaded. For small tweaks, put Lua in `~/.config/ringmd/init.lua`; it runs
after the bundled config.

Inside the editor, `:Preview` toggles the preview pane. `:Theme <variant>`
and `:ExportPdf [path]` exist and tab-complete, but only report "not
available yet" until themes (M4) and PDF export (M5) land.

## Development

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test          # includes tests/nvim_session.rs, which needs nvim 0.10+
```

The code follows **MVVM** (`docs/SPEC.md` §3): `view` → `viewmodel` → `model`,
with platform access behind traits in `services`. ViewModels depend on `glib`
only and are tested without a display; `tests/architecture.rs` enforces the
layering.

## License

[MIT](LICENSE)
