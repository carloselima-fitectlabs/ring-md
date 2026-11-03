# RingMD

A graphical, Linux-first Markdown editor powered by an embedded Neovim engine,
with live preview, Gruvbox themes and PDF export.

> **Status:** early development. The embedded Neovim editor, its bundled
> config and the live preview work (M0–M3); Gruvbox themes (M4) are next.
> See [docs/SPEC.md](docs/SPEC.md) for the specification and task breakdown.

## Building

Requirements: Rust 1.92+, GTK 4.14+ and WebKitGTK 6.0 development files, and
[Neovim](https://neovim.io) 0.10+ on `PATH` at run time (Ubuntu 24.04's
`neovim` package is 0.9, so use the upstream release there).

```bash
sudo apt install libgtk-4-dev libwebkitgtk-6.0-dev   # Debian / Ubuntu 24.04+
sudo dnf install gtk4-devel webkitgtk6.0-devel       # Fedora
```

```bash
cargo run -- notes.md
```

Set `RINGMD_LOG=debug` for verbose logs. RingMD starts Neovim with its own
`runtime/init.lua` (override the directory with `RINGMD_RUNTIME`) and
`NVIM_APPNAME=ringmd`: your `~/.config/nvim`, plugins and shada are not
loaded. For small tweaks, put Lua in `~/.config/ringmd/init.lua`; it runs
after the bundled config.

The preview pane renders the buffer as you type (CommonMark + GFM: tables,
task lists, footnotes, highlighted code) and follows the cursor. Scripts in
documents never run; links open in your browser; remote images load, other
remote resources do not. Inside the editor, `:Preview` or `Ctrl+P` (Normal
mode) toggles the preview pane. `:Theme <variant>`
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

## Docker

The `Dockerfile` has a `dev` stage (Rust, GTK 4 headers, Neovim, Xvfb) and a
slim `runtime` stage with just the app. `docker-compose.yml` wraps both:

```bash
docker compose run --rm check
```

runs `cargo fmt --check`, clippy and all tests, like CI. For a shell with
the toolchain:

```bash
docker compose run --rm dev
```

To run the editor itself on your X11 display (XWayland works too), allow
local containers on your X server once per session, then start it with a
file from the current directory:

```bash
xhost +si:localuser:$(id -un)
```

```bash
docker compose run --rm app notes.md
```

Set `RINGMD_DOCS=/path/to/notes` to open files from another folder, and
`RINGMD_UID`/`RINGMD_GID` if your user is not uid/gid 1000. Your
`~/.config/ringmd/init.lua` and Neovim's state persist in the
`ringmd-home` volume.

## License

[MIT](LICENSE)
