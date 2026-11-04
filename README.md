# RingMD

A graphical, Linux-first Markdown editor powered by an embedded Neovim engine,
with live preview, Gruvbox themes and PDF export.

> **Status:** feature-complete for v1 (M0–M7). Editor, bundled config, live
> preview, Gruvbox themes, PDF export, files, configuration and local
> installation work; manual QA on real desktops is in progress.
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
mode) toggles the preview pane.

`:Theme <variant>` switches between the Gruvbox variants (`dark-hard`,
`dark-medium`, `dark-soft`, `light-hard`, `light-medium`, `light-soft`)
live, or `system` / `system-hard` / `system-soft` to follow the desktop's
dark/light preference. The choice is saved to
`~/.config/ringmd/config.toml`, which also accepts (all optional):

```toml
theme = "dark-medium"
font = "Monospace"
font_size = 12
[preview]
visible = true
debounce_ms = 50
sync_scroll = true
[pdf]
page_size = "A4"     # or "Letter"
margin_mm = 20
```

Invalid values are logged and replaced by their defaults. Switching the
theme, toggling the preview and zooming the font are saved back to the
file, keeping your comments.

| Key | Action |
|-----|--------|
| `Ctrl+P` | Toggle the preview (Normal mode) |
| `Ctrl+E` | Export PDF (Normal mode) |
| `Ctrl+O` | Open a file (Normal mode) |
| `Ctrl+S` | Save, or Save As for an unnamed buffer (all modes) |
| `Ctrl+=` / `Ctrl+-` / `Ctrl+0` | Font size up / down / reset |
| `F11` | Fullscreen |

In other modes `Ctrl+O/E/P` keep their Neovim meaning. Closing the window
with unsaved changes asks to Save, Discard or Cancel; files dropped on the
window open in the editor, and files changed on disk are reloaded when
RingMD regains focus (`autoread`).

`:ExportPdf [path]` or `Ctrl+E` (Normal mode) writes a plain, printable PDF
(white page, serif text) next to the file, or where you say; an unnamed
buffer asks for a location. No external tools are needed. Known gap: links
in the PDF are not clickable (WebKitGTK's PDF output has no link
annotations), so each external link's address is printed after it.

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

## Installing

From source (Neovim 0.10+ must be on `PATH`):

```bash
cargo build --release
```

```bash
./data/install.sh ~/.local
```

This installs `ringmd`, its Neovim runtime, the desktop entry (so “Open
with RingMD” appears for `.md` files) and the icon under the prefix, and
refreshes the desktop and icon caches. It warns if `~/.local/bin` is not on
`PATH` or if Neovim is missing or older than 0.10. Use
`sudo ./data/install.sh /usr/local` for a system-wide install instead.

To update, pull, rebuild and run the same command again. To remove it:

```bash
./data/install.sh --uninstall ~/.local
```

Your `~/.config/ringmd` (config and `init.lua`) is left in place.

As a Flatpak (experimental; not published on Flathub), which bundles Neovim and the Markdown parsers:

```bash
flatpak-builder --user --install --install-deps-from=flathub --force-clean build flatpak/io.github.ringmd.RingMD.yml
```

CI also builds `ringmd.flatpak` as an artifact on every push; install it
with `flatpak install --user ringmd.flatpak`.

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

The container is mainly for building and testing. WebKit's sandbox needs
unprivileged user namespaces, which many Docker setups block; if the
preview stays blank or PDF export fails in the container, install RingMD
natively as described above.

## License

[MIT](LICENSE)
