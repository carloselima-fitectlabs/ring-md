# RingMD — Specification

> Status: **v1.0** (all open questions resolved) · Process: Spec-Driven Development (SDD)
>
> This document is the source of truth. Code is written to satisfy the
> requirements and acceptance criteria below. Any change in behaviour starts
> with a change to this file.

---

## 1. Overview

RingMD is a **graphical Markdown editor for Linux** whose editing engine is a
real, embedded **Neovim** instance. It shows a **live rendered preview** next to
the editor, uses the **Gruvbox** palette throughout, and exports documents to a
**clean, printable PDF**.

### 1.1 Goals

| ID | Goal |
|----|------|
| G1 | Markdown editor powered by the real Neovim engine (not an emulation) |
| G2 | Graphical desktop app — never runs inside a terminal |
| G3 | Linux first (Wayland and X11) |
| G4 | Gruvbox theme, with variants |
| G5 | Real-time Markdown rendering |
| G6 | Markdown → PDF export |
| G7 | Simple: one window, few concepts, no configuration required |

### 1.2 Non-goals (v1)

- Windows / macOS support
- File tree, project sidebar, tabs UI (Neovim buffers are enough)
- Loading the user's personal Neovim config or plugins
- Plugin system for RingMD itself
- LaTeX math, Mermaid diagrams, WYSIWYG editing
- Exporting to formats other than PDF

### 1.3 Decisions

| ID | Decision |
|----|----------|
| D1 | Engine: real Neovim, spawned with `nvim --embed`, driven over msgpack-RPC |
| D2 | Neovim runs with a **clean built-in config** (`-u` pointing to RingMD's bundled `init.lua`, `--clean` semantics; user config is never loaded) |
| D3 | Gruvbox ships in **variants**: dark/light × hard/medium/soft contrast |
| D4 | PDF output is **plain white and printable**, *not* themed |
| D5 | Name: **RingMD**. Primary distribution: **Flatpak** |
| D6 | Stack: Rust · GTK4 (gtk-rs) · WebKitGTK 6 · comrak · nvim-rs (or equivalent) |
| D7 | Architecture pattern: **MVVM** (Model–View–ViewModel), see §3.2 |
| D8 | `Ctrl+O` / `Ctrl+E` / `Ctrl+P` are GUI shortcuts in **Normal mode only**; in other modes they go to Neovim |
| D9 | PDF body font: **serif** (Noto Serif, falling back to the system serif font) |
| D10 | License: **MIT** |
| D11 | Window chrome: **minimal headerbar** (file title + preview toggle button + menu with Export PDF / Theme) |
| D12 | Preview may load **remote images** (`https://`); no other remote resources (scripts, styles, iframes, fonts) |

---

## 2. Glossary

| Term | Meaning |
|------|---------|
| Grid | The character cell matrix Neovim sends via `ext_linegrid` UI events |
| Editor pane | The GTK widget that draws the Neovim grid and forwards input |
| Preview pane | The WebKitGTK view showing rendered HTML |
| Variant | One Gruvbox palette combination (e.g. *dark-hard*) |
| Ex command | A `:command` typed in Neovim |

---

## 3. Architecture

Runtime data flow (the layering of these parts is defined in §3.1):

```
┌──────────────────────── RingMD (GTK4 Application) ────────────────────────┐
│                                                                           │
│  ┌──────────── Editor pane ────────────┐  ┌────────── Preview pane ─────┐ │
│  │  GridWidget (DrawingArea + Pango)   │  │  WebKitGTK WebView          │ │
│  │   ▲ redraw events      │ key/mouse  │  │   ▲ JS: patch <article>     │ │
│  └───┼────────────────────┼────────────┘  └───┼─────────────────────────┘ │
│      │                    ▼                   │                           │
│  ┌───┴──────── NvimBridge (async RPC) ─────┐  │                           │
│  │ nvim --embed -u <bundled init.lua>      │  │                           │
│  │ nvim_ui_attach(ext_linegrid)            │  │                           │
│  │ nvim_buf_attach → on_lines ─────────────┼──┼──► Renderer (comrak) ──┐  │
│  │ rpcnotify ← :Preview :ExportPdf :Theme  │  │    MD → HTML + sourcepos│  │
│  └─────────────────────────────────────────┘  └─────────────────────────┘  │
│                                                                           │
│  Theme (Gruvbox palette → nvim highlights, GTK CSS, preview CSS)    │
│  PdfExporter (offscreen WebView + print stylesheet → PDF file)            │
│  Config (~/.config/ringmd/config.toml)                                    │
└───────────────────────────────────────────────────────────────────────────┘
```

### 3.1 MVVM layers

The code is split into three layers. Dependencies only point downward:
**View → ViewModel → Model**.

```
┌──────────────────────────── View (GTK4 widgets) ────────────────────────────┐
│ MainWindow · GridView · PreviewView · Dialogs (unsaved, file chooser, error) │
│ - draws state, forwards user input as ViewModel method calls                 │
│ - binds to ViewModel properties (GObject property bindings / notify::)      │
└──────────────────────────────────────┬──────────────────────────────────────┘
                                       │ properties ▲  commands ▼
┌──────────────────────────── ViewModel (GObject) ────────────────────────────┐
│ AppViewModel · EditorViewModel · PreviewViewModel · ThemeViewModel ·         │
│ ExportViewModel                                                              │
│ - exposes observable state (properties) and commands (methods)              │
│ - no GTK widget types, no drawing, no direct I/O                            │
└──────────────────────────────────────┬──────────────────────────────────────┘
                                       │ calls ▼  events ▲
┌──────────────────────────── Model (plain Rust) ─────────────────────────────┐
│ NvimSession · Grid · Document · MarkdownRenderer · Palette/Theme ·           │
│ PdfExporter (service trait) · Config                                         │
│ - domain logic and services, no GTK dependency (except behind traits)       │
└─────────────────────────────────────────────────────────────────────────────┘
```

| Layer | Component | Responsibility |
|-------|-----------|----------------|
| Model | `NvimSession` | Spawning Neovim, RPC client, decoding redraw events, encoding input |
| Model | `Grid` | Cells, highlight table, cursor, mode info — updated from redraw events |
| Model | `Document` | Current buffer text, file path, modified flag (mirrored from Neovim) |
| Model | `MarkdownRenderer` | Markdown → HTML with source-line mapping, sanitization |
| Model | `Palette` / `Theme` | Gruvbox variants; generates nvim highlights, GTK CSS, preview CSS |
| Model | `PdfExporter` | Trait + WebKit implementation: HTML + print CSS → PDF file |
| Model | `Config` | Load/save/validate `config.toml` |
| ViewModel | `AppViewModel` | Window title, preview visibility, fullscreen, quit/unsaved flow, error messages |
| ViewModel | `EditorViewModel` | Exposes `Grid` snapshots + damage, cursor shape, font; commands: `key_pressed`, `mouse`, `resize`, `zoom` |
| ViewModel | `PreviewViewModel` | Debounced HTML, `scroll_target_line`, base URI; commands: `link_activated` |
| ViewModel | `ThemeViewModel` | `variant`, generated CSS strings; command: `set_variant` |
| ViewModel | `ExportViewModel` | `is_exporting`, last result; command: `export(path)` |
| View | `MainWindow` | Layout (`GtkPaned`), headerbar, shortcuts, toasts |
| View | `GridView` | `GtkDrawingArea` drawing the grid via Pango/Cairo |
| View | `PreviewView` | `WebKitWebView`, applies HTML patches and scroll targets |
| View | `Dialogs` | Unsaved changes, file chooser, error dialog |
| — | `runtime/` | Bundled `init.lua` and Lua glue (shipped with the app) |

### 3.2 MVVM rules

| ID | Rule |
|----|------|
| A1 | Views hold a reference to their ViewModel; ViewModels never reference Views. |
| A2 | ViewModels are GObject subclasses; state is exposed as GObject properties so Views can use `bind_property` / `notify::`. |
| A3 | Views contain no logic beyond drawing, layout and forwarding input. Any decision (e.g. "should Ctrl+P toggle the preview in this mode?") lives in a ViewModel. |
| A4 | Models have no dependency on GTK widgets. Platform services (PDF printing, clipboard, file chooser) are reached through traits so ViewModels can be tested with fakes. |
| A5 | Neovim callbacks (`rpcnotify` for `:Preview`, `:Theme`, `:ExportPdf`) arrive in the Model and are routed to ViewModel commands — the same path as GUI shortcuts. |
| A6 | Every ViewModel has unit tests that run without a display (no `gtk::init`). |

### 3.3 Source layout

```
src/
  main.rs     entry point: CLI → logging → app
  lib.rs      crate root (layers below)
  app.rs      GTK application lifecycle
  cli.rs      `ringmd [FILE]`
  logging.rs  `RINGMD_LOG` tracing setup
  model/      document.rs grid.rs clipboard.rs theme.rs nvim/ · render.rs pdf.rs config.rs
    nvim/     session.rs (process + RPC) rpc.rs redraw.rs input.rs command.rs version.rs runtime.rs fake.rs
  viewmodel/  app.rs editor.rs session.rs (event routing) · preview.rs theme.rs export.rs
  view/       window.rs dialogs.rs grid_view.rs · preview_view.rs
  services/   traits (PDF, clipboard, file dialogs) + gtk.rs (real) + fake.rs for tests
runtime/      init.lua
tests/        architecture.rs (layering rules) nvim_session.rs (real nvim) · fixtures/
```

Modules after `·` are planned for later milestones. `tests/architecture.rs`
enforces rules A1, A2 and A4 by failing the build when a lower layer imports
GTK UI types or a higher layer.

---

## 4. Features & Requirements

Requirement IDs use the form `F<feature>.R<n>`. Each requirement has
acceptance criteria (`AC`) that must be verifiable by a test or a manual check.

### F1 — Application shell

| ID | Requirement |
|----|-------------|
| F1.R1 | RingMD launches as a native GTK4 window (no terminal involved). |
| F1.R2 | `ringmd [FILE]` opens `FILE`; with no argument, an empty unnamed buffer opens. |
| F1.R3 | Window has a single editor pane and an optional preview pane in a resizable split. |
| F1.R7 | A minimal headerbar shows the window title, a preview toggle button and a menu (Open, Export PDF, Theme ▸ variants, About). Hidden in fullscreen. |
| F1.R4 | Window title shows `<filename>[+] — RingMD` (`+` when modified). |
| F1.R5 | Closing the window with unsaved changes asks to Save / Discard / Cancel. |
| F1.R6 | Works on Wayland and X11. |

**AC**
- F1-AC1: `ringmd notes.md` opens a window showing the file contents within 500 ms on a typical laptop.
- F1-AC2: Editing then closing the window shows the unsaved-changes dialog.
- F1-AC3: App starts under both `GDK_BACKEND=wayland` and `GDK_BACKEND=x11`.

### F2 — Embedded Neovim engine

| ID | Requirement |
|----|-------------|
| F2.R1 | Spawns `nvim --embed` and attaches as a UI with `ext_linegrid` (and `rgb`). |
| F2.R2 | Neovim always starts with RingMD's bundled `init.lua`; user config, user plugins and `shada` from the user's Neovim are **not** loaded. |
| F2.R3 | Minimum supported Neovim version: **0.10**. On older/missing nvim, show a clear error dialog. |
| F2.R4 | All keyboard input (including modifiers, special keys, IME/dead keys) is forwarded via `nvim_input`. |
| F2.R5 | Mouse click, drag and wheel are forwarded via `nvim_input_mouse`. |
| F2.R6 | Window resize triggers `nvim_ui_try_resize` with the new grid size. |
| F2.R7 | If Neovim exits (`:q`, `:wq`), RingMD closes. If it crashes, show an error dialog. |
| F2.R8 | Clipboard: `"+` and `"*` registers use the system clipboard via GTK (no external `xclip`/`wl-copy` needed). |

**AC**
- F2-AC1: `:echo $MYVIMRC` shows the bundled RingMD path, not `~/.config/nvim/init.lua`.
- F2-AC2: Macros (`qa…q`, `@a`), registers, text objects, `:s///g` and `.` repeat all work.
- F2-AC3: `"+y` in RingMD then Ctrl+V in another app pastes the text.
- F2-AC4: Removing `nvim` from `PATH` (non-Flatpak build) produces a readable error dialog.

### F3 — Editor rendering (grid widget)

| ID | Requirement |
|----|-------------|
| F3.R1 | Draws the grid with a monospace font using Pango, honouring fg/bg/sp colors, bold, italic, underline, undercurl, strikethrough, reverse. |
| F3.R2 | Cursor shape follows `mode_info_set` (block / vertical bar / horizontal bar). |
| F3.R3 | Handles `grid_scroll` efficiently (no full redraw per scroll). |
| F3.R4 | Supports wide characters (CJK) and emoji occupying two cells. |
| F3.R5 | Font family and size configurable; default `monospace 12`. Ctrl+= / Ctrl+- / Ctrl+0 change/reset size. |
| F3.R6 | Rendering is HiDPI-aware (fractional scaling). |

**AC**
- F3-AC1: Scrolling a 10 000-line file with `Ctrl+D` holds ≥ 60 fps on a typical laptop.
- F3-AC2: Cursor is a block in Normal, a bar in Insert, an underline in Replace.
- F3-AC3: A line containing `日本語 😀` aligns correctly with surrounding text.

### F4 — Live Markdown preview

| ID | Requirement |
|----|-------------|
| F4.R1 | Preview pane renders the current buffer as HTML using CommonMark + GFM extensions: tables, task lists, strikethrough, autolinks, footnotes. |
| F4.R2 | Preview updates on every buffer change, debounced (default 50 ms), without reloading the page. |
| F4.R3 | Preview scroll position is preserved across updates. |
| F4.R4 | Preview scroll follows the editor cursor line (scroll sync), using source-position data. |
| F4.R5 | Fenced code blocks are syntax highlighted. |
| F4.R6 | Relative image paths resolve relative to the file's directory. |
| F4.R7 | Links open in the system browser, never inside the preview. |
| F4.R8 | Raw HTML in Markdown is sanitized (no scripts executed). |
| F4.R10 | Remote images (`https://`) are loaded; all other remote resources (scripts, stylesheets, fonts, iframes) are blocked. |
| F4.R9 | `:Preview` toggles the preview pane; `Ctrl+P` does the same. |

**AC**
- F4-AC1: Typing a character updates the preview in ≤ 100 ms for a 1 000-line document.
- F4-AC2: Moving the cursor to a heading 300 lines down scrolls the preview to that heading.
- F4-AC3: `<script>alert(1)</script>` in the document does not execute.
- F4-AC4: `![](img/a.png)` displays the image next to the opened file.
- F4-AC5: `![](https://…/x.png)` displays; `<link rel=stylesheet href="https://…">` is not loaded.

### F5 — Gruvbox theme & variants

| ID | Requirement |
|----|-------------|
| F5.R1 | Ships 6 variants: `dark-hard`, `dark-medium`, `dark-soft`, `light-hard`, `light-medium`, `light-soft`. Default: `dark-medium`. |
| F5.R2 | One palette definition drives three outputs: Neovim highlight groups, GTK window CSS, preview CSS. |
| F5.R3 | Neovim colorscheme covers base UI groups and Markdown/Treesitter Markdown groups (headings, emphasis, code, links, lists, quotes). |
| F5.R4 | `:Theme <variant>` switches variant live, with completion; the choice persists in config. |
| F5.R5 | Option `theme = "system"` follows the desktop dark/light preference (via libadwaita / portal), keeping the chosen contrast. |

**AC**
- F5-AC1: `:Theme light-soft` recolors editor, preview and window chrome without restart.
- F5-AC2: Restarting RingMD keeps the last chosen variant.
- F5-AC3: Hex values in each variant match the reference Gruvbox palette (see Appendix A).

### F6 — PDF export

| ID | Requirement |
|----|-------------|
| F6.R1 | `:ExportPdf [path]` exports the current buffer to PDF. Default path: same directory and name as the file, with `.pdf`. Unnamed buffer → file chooser. |
| F6.R2 | PDF style is **plain and printable**: white background, black text, no Gruvbox colors, serif body font (Noto Serif → system serif), monospace code with light-grey background. |
| F6.R3 | Default page: A4, 20 mm margins. Page size (A4/Letter) and margins configurable. |
| F6.R4 | Headings avoid being orphaned at page bottom; code blocks, tables and images avoid splitting where possible. |
| F6.R5 | Links remain clickable in the PDF. |
| F6.R6 | Export runs without blocking editing; a notification reports success (`Exported to …`) or the error. |
| F6.R7 | Export needs no external tools (no LaTeX, pandoc, wkhtmltopdf). |

**AC**
- F6-AC1: Exporting a sample doc with headings, table, code, image, task list produces a valid PDF that opens in Evince/Okular.
- F6-AC2: The PDF has a white background regardless of the active Gruvbox variant.
- F6-AC3: Clicking a link in the PDF opens the URL.

### F7 — File operations

| ID | Requirement |
|----|-------------|
| F7.R1 | `:w`, `:e`, `:wq`, `:q!` behave as in Neovim. |
| F7.R2 | `Ctrl+O` opens a GTK file chooser → `:edit <file>`. `Ctrl+S` → `:write` (or Save As chooser if unnamed). |
| F7.R3 | Files modified on disk trigger Neovim's normal `autoread`/`FileChangedShell` behaviour. |
| F7.R4 | Dropping a `.md` file onto the window opens it. |

**AC**
- F7-AC1: `Ctrl+S` on an unnamed buffer opens Save As; the file is written to the chosen path.
- F7-AC2: Dragging `README.md` from the file manager opens it.

### F8 — Bundled runtime (clean config)

| ID | Requirement |
|----|-------------|
| F8.R1 | Bundled `init.lua` sets sensible Markdown defaults: `wrap`, `linebreak`, `spell` off by default, `conceallevel=0`, `expandtab`, `shiftwidth=2`, `number` off, `termguicolors`. |
| F8.R2 | Registers user commands `:Preview`, `:ExportPdf`, `:Theme` that call back to the GUI via `rpcnotify`. |
| F8.R3 | Treesitter Markdown highlighting is enabled when the parser is available (bundled in Flatpak). |
| F8.R4 | Optional escape hatch: `~/.config/ringmd/init.lua`, if present, is sourced **after** the bundled one (for small tweaks only; still no user `nvim` config). |

**AC**
- F8-AC1: A fresh user account with no config gets the same experience as a configured one.
- F8-AC2: `:Preview`, `:ExportPdf`, `:Theme` exist and tab-complete.

### F9 — Configuration

| ID | Requirement |
|----|-------------|
| F9.R1 | Config file: `$XDG_CONFIG_HOME/ringmd/config.toml`. All keys optional. |
| F9.R2 | Keys: `theme`, `font`, `font_size`, `preview.visible`, `preview.debounce_ms`, `preview.sync_scroll`, `pdf.page_size`, `pdf.margin_mm`. |
| F9.R3 | Invalid config values log a warning and fall back to defaults; the app still starts. |
| F9.R4 | Settings changed via commands (`:Theme`, font zoom, preview toggle) are persisted. |

**AC**
- F9-AC1: Deleting the config file restores all defaults.
- F9-AC2: `theme = "purple"` starts the app with `dark-medium` and logs a warning.

### F10 — Packaging & distribution

| ID | Requirement |
|----|-------------|
| F10.R1 | Flatpak manifest (GNOME runtime) bundling Neovim ≥ 0.10 and the Treesitter Markdown parsers. |
| F10.R2 | `.desktop` file, AppStream metainfo, and icon; registers as handler for `text/markdown`. |
| F10.R3 | `cargo build --release` produces a standalone binary that uses system `nvim`. |
| F10.R4 | CI builds and tests on every push (Linux). |

**AC**
- F10-AC1: `flatpak install` + launching from the app menu works on a clean Fedora and Ubuntu.
- F10-AC2: “Open with RingMD” appears for `.md` files in the file manager.

---

## 5. Keybindings (GUI-level)

Everything else is plain Neovim.

| Key | Action |
|-----|--------|
| `Ctrl+P` | Toggle preview (`:Preview`) |
| `Ctrl+E` | Export PDF (`:ExportPdf`) |
| `Ctrl+O` | Open file chooser |
| `Ctrl+S` | Save / Save As |
| `Ctrl+=` / `Ctrl+-` / `Ctrl+0` | Font size up / down / reset |
| `F11` | Fullscreen |

> `Ctrl+O` / `Ctrl+E` / `Ctrl+P` are intercepted only in **Normal mode** (D8);
> in Insert, Visual, Command-line and other modes they are sent to Neovim.
> In Normal mode they therefore replace Neovim's jumplist-back (`Ctrl+O`) and
> scroll-down (`Ctrl+E`); those remain available as `:normal! <C-o>` or via
> `<C-o>`-free alternatives (`''`, `g;`). `Ctrl+S`, `Ctrl+=/-/0` and `F11`
> are intercepted in all modes.

---

## 6. Non-functional requirements

| ID | Requirement |
|----|-------------|
| N1 | Cold start to interactive editor ≤ 500 ms (excluding Flatpak first-run). |
| N2 | Idle memory ≤ 150 MB including WebKit. |
| N3 | No network access at runtime except remote images referenced by the document (F4.R10). |
| N4 | Keystroke-to-screen latency in the editor ≤ 16 ms (one frame). |
| N5 | Code is `cargo clippy -D warnings` and `cargo fmt` clean. |
| N6 | License: MIT (`LICENSE` file at repo root). |

---

## 7. Task breakdown

Tasks are grouped by milestone. Each task lists the requirements it satisfies
and what it depends on. A task is **done** when its linked acceptance criteria
pass and it has tests where testable.

Legend: **S** ≈ ½ day · **M** ≈ 1–2 days · **L** ≈ 3–5 days

Progress: **M0, M1, M2 done** · M3–M7 not started.

### M0 — Project foundation ✅

| Task | Description | Reqs | Deps | Size |
|------|-------------|------|------|------|
| T0.1 | Cargo project, MVVM source layout (§3.3), `rustfmt`/`clippy` config, MIT `LICENSE` | N5, N6, A1–A6 | — | S |
| T0.2 | GitHub Actions CI: build, test, clippy, fmt on Ubuntu with GTK4 + WebKitGTK 6 | F10.R4 | T0.1 | S |
| T0.3 | GTK4 `Application` + empty window, CLI arg parsing (`ringmd [FILE]`) | F1.R1, F1.R2 | T0.1 | S |
| T0.4 | Logging (`tracing`) and error-dialog helper | F2.R3, F2.R7 | T0.3 | S |
| T0.5 | MVVM scaffolding: GObject ViewModel base pattern, `AppViewModel` skeleton, service traits (`PdfService`, `ClipboardService`, `FileDialogService`) with fakes for tests | A2, A4, A6 | T0.1 | M |

### M1 — Neovim engine in a window *(critical path)* ✅

| Task | Description | Reqs | Deps | Size |
|------|-------------|------|------|------|
| T1.1 | **Model** `NvimSession`: spawn `nvim --embed -u <runtime>/init.lua`, async RPC on GLib main loop, version check | F2.R1–R3 | T0.3 | M |
| T1.2 | **Model** `Grid`: decode `redraw` events into the grid (`grid_resize`, `grid_line`, `grid_clear`, `grid_scroll`, `grid_cursor_goto`, `hl_attr_define`, `default_colors_set`, `flush`) | F2.R1, F3.R3 | T1.1 | M |
| T1.3 | **ViewModel** `EditorViewModel` (grid snapshot + damage, cursor, font properties) and **View** `GridView`: draw cells with Pango layouts into GTK snapshots, text attributes, wide chars, HiDPI | F3.R1, F3.R4, F3.R6 | T1.2 | L |
| T1.4 | Cursor rendering from `mode_info_set` / `mode_change` | F3.R2 | T1.3 | S |
| T1.5 | **View→VM** keyboard input → `EditorViewModel::key_pressed` → `nvim_input` (keysym mapping, modifiers, IME via `GtkIMContext`) | F2.R4 | T1.1 | M |
| T1.6 | **View→VM** mouse input → `EditorViewModel::mouse` → `nvim_input_mouse` | F2.R5 | T1.3 | S |
| T1.7 | **View→VM** resize → `EditorViewModel::resize` → `nvim_ui_try_resize` | F2.R6 | T1.3 | S |
| T1.8 | **Model** `Document` (path, modified flag); **AppViewModel** `title` property; Neovim exit/crash handling | F2.R7, F1.R4 | T1.1 | S |
| T1.9 | Clipboard provider (`g:clipboard`: copy → rpcnotify, paste → rpcrequest → `ClipboardService` → GTK clipboard) | F2.R8 | T1.1 | M |
| T1.10 | Font config + zoom shortcuts | F3.R5 | T1.3 | S |
| T1.11 | Performance pass: damage-region redraws (per-row render-node cache); verify F3-AC1 and N4 on real hardware | F3.R3, N4 | T1.3 | M |

### M2 — Bundled runtime (clean config) ✅

| Task | Description | Reqs | Deps | Size |
|------|-------------|------|------|------|
| T2.1 | `runtime/init.lua` with Markdown defaults; ensure user config/plugins/shada are isolated (`NVIM_APPNAME=ringmd` for the child process; user/system dirs dropped from `runtimepath`/`packpath`) | F2.R2, F8.R1 | T1.1 | S |
| T2.2 | User commands `:Preview`, `:ExportPdf`, `:Theme` → `rpcnotify` to GUI, with completion | F8.R2 | T2.1 | S |
| T2.3 | Treesitter Markdown highlighting when parser available | F8.R3 | T2.1 | S |
| T2.4 | Optional `~/.config/ringmd/init.lua` sourced after bundled config | F8.R4 | T2.1 | S |

### M3 — Live preview

| Task | Description | Reqs | Deps | Size |
|------|-------------|------|------|------|
| T3.1 | **Model** `MarkdownRenderer`: comrak with GFM extensions + `sourcepos`; HTML sanitization; unit tests with fixture docs | F4.R1, F4.R8 | T0.1 | M |
| T3.2 | Code block syntax highlighting (syntect, CSS classes) | F4.R5 | T3.1 | S |
| T3.3 | **View** `PreviewView`: WebKitGTK WebView in a `GtkPaned`, base HTML shell, scripts disabled for document content | F1.R3, F4.R8 | T0.3 | M |
| T3.4 | **ViewModel** `PreviewViewModel`: `nvim_buf_attach` → debounced re-render → `html` property; View patches `<article>` via JS, preserving scroll | F4.R2, F4.R3 | T1.1, T3.1, T3.3, T0.5 | M |
| T3.5 | Base URI = file directory for relative images | F4.R6 | T3.3 | S |
| T3.6 | Open links in system browser (navigation policy) | F4.R7 | T3.3 | S |
| T3.9 | Resource policy: allow remote images, block other remote resources; test with fixture | F4.R10 | T3.3 | S |
| T3.7 | Scroll sync: `CursorMoved` → `PreviewViewModel.scroll_target_line` → View scrolls to nearest `data-sourcepos` element | F4.R4 | T3.4 | M |
| T3.8 | `AppViewModel.preview_visible` + `toggle_preview` command, bound to `:Preview` and `Ctrl+P` | F4.R9 | T2.2, T3.3 | S |

### M4 — Gruvbox theme & variants

| Task | Description | Reqs | Deps | Size |
|------|-------------|------|------|------|
| T4.1 | **Model** `Palette`: data for 6 variants (Appendix A) + unit tests against reference hex | F5.R1, F5-AC3 | T0.1 | S |
| T4.2 | Generate Neovim highlights (UI + Markdown + Treesitter groups) from palette; apply via `nvim_set_hl` | F5.R2, F5.R3 | T4.1, T1.1 | M |
| T4.3 | Generate GTK CSS for window chrome (headerbar, paned handle, dialogs) | F5.R2 | T4.1 | S |
| T4.4 | Generate preview CSS (typography + palette) | F5.R2 | T4.1, T3.3 | S |
| T4.5 | **ViewModel** `ThemeViewModel`: `variant` property, `set_variant` command; `:Theme <variant>` live switch + persistence | F5.R4 | T2.2, T4.2–T4.4, T6.1 | S |
| T4.6 | `theme = "system"` following desktop color scheme | F5.R5 | T4.5 | S |

### M5 — PDF export

| Task | Description | Reqs | Deps | Size |
|------|-------------|------|------|------|
| T5.1 | Print stylesheet: white background, black text, serif body, monospace code, page-break rules | F6.R2, F6.R4 | T3.1 | M |
| T5.2 | `PdfService` WebKit implementation: offscreen WebView + `WebKitPrintOperation` → PDF file, page size & margins | F6.R1, F6.R3, F6.R5, F6.R7 | T3.3, T5.1 | M |
| T5.3 | `:ExportPdf [path]` / `Ctrl+E`, default path, file chooser for unnamed buffer | F6.R1 | T2.2, T5.2 | S |
| T5.4 | **ViewModel** `ExportViewModel`: async export, `is_exporting` + result properties; View shows toast | F6.R6 | T5.3 | S |
| T5.5 | Golden test: export fixture doc, assert valid PDF, page count, white background, link annotations | F6-AC1–AC3 | T5.2 | M |

### M6 — Files & configuration

| Task | Description | Reqs | Deps | Size |
|------|-------------|------|------|------|
| T6.1 | **Model** `Config`: TOML load/save, defaults, validation with warnings | F9.R1–R4 | T0.1 | S |
| T6.2 | Unsaved-changes flow in `AppViewModel`; View shows the dialog | F1.R5 | T1.8 | S |
| T6.3 | `Ctrl+O` open / `Ctrl+S` save & Save As choosers | F7.R2 | T1.1 | S |
| T6.4 | Drag-and-drop file open | F7.R4 | T1.1 | S |
| T6.5 | `autoread` + external change handling in bundled config | F7.R3 | T2.1 | S |
| T6.6 | Mode-aware shortcut decisions in `EditorViewModel` (§5, rule A3) | §5 | T1.5 | S |
| T6.7 | Fullscreen (`F11`) | §5 | T0.3 | S |
| T6.8 | **View** minimal headerbar bound to `AppViewModel` (title, preview toggle, menu) | F1.R7 | T0.5, T3.8 | S |

### M7 — Packaging & release

| Task | Description | Reqs | Deps | Size |
|------|-------------|------|------|------|
| T7.1 | App icon, `.desktop` file, AppStream metainfo, `text/markdown` MIME association | F10.R2 | T0.3 | S |
| T7.2 | Flatpak manifest: GNOME runtime, Rust SDK extension, bundled Neovim + Treesitter parsers | F10.R1 | M1–M5 | M |
| T7.3 | CI job building the Flatpak bundle as an artifact | F10.R4 | T7.2 | S |
| T7.4 | README: install, usage, keybindings, config reference | — | all | S |
| T7.5 | Manual QA pass on Fedora + Ubuntu, Wayland + X11 (F1-AC3, F10-AC1/2) | F1.R6 | T7.2 | S |

### 7.1 Critical path

```
T0.1 → T0.3 → T1.1 → T1.2 → T1.3 → (T1.5, T1.7) → T3.4 → T3.7 → T5.2 → T7.2
```

M3 (`render`) and M4 (`theme` palette) can be developed in parallel with M1
since their core logic has no GUI dependency.

---

## 8. Testing strategy

| Level | What |
|-------|------|
| Unit (Model) | `MarkdownRenderer` (fixtures → HTML snapshots), `Palette` (hex values), `Config` (parsing/fallbacks), redraw decoder (recorded event streams → expected `Grid`) |
| Unit (ViewModel) | Every ViewModel tested headless with fake services and a fake Neovim session: property changes, commands, debounce, unsaved-changes flow (rule A6) |
| Integration | Headless `nvim --embed` driven by the bridge: send keys, assert buffer contents and grid state |
| Golden | PDF export of fixture documents; preview HTML snapshots |
| Manual | Visual checks per variant, Wayland/X11, HiDPI, IME input |

Fixture documents live in `tests/fixtures/` and cover every GFM feature in F4.R1.

---

## 9. Resolved questions

| ID | Question | Answer |
|----|----------|--------|
| Q1 | Scope of `Ctrl+O`/`Ctrl+E`/`Ctrl+P` | Normal mode only → D8 |
| Q2 | PDF body font | Serif → D9 |
| Q3 | License | MIT → D10 |
| Q4 | Window chrome | Minimal headerbar → D11 |
| Q5 | Remote images in preview | Yes, images only → D12 |

---

## Appendix A — Gruvbox reference palette

Background by variant:

| Variant | bg0 |
|---------|-----|
| dark-hard | `#1d2021` |
| dark-medium | `#282828` |
| dark-soft | `#32302f` |
| light-hard | `#f9f5d7` |
| light-medium | `#fbf1c7` |
| light-soft | `#f2e5bc` |

Dark palette:

| Name | Hex | Name | Hex |
|------|-----|------|-----|
| bg1 | `#3c3836` | fg0 | `#fbf1c7` |
| bg2 | `#504945` | fg1 | `#ebdbb2` |
| bg3 | `#665c54` | fg2 | `#d5c4a1` |
| bg4 | `#7c6f64` | gray | `#928374` |
| red | `#fb4934` | green | `#b8bb26` |
| yellow | `#fabd2f` | blue | `#83a598` |
| purple | `#d3869b` | aqua | `#8ec07c` |
| orange | `#fe8019` | | |

Light palette:

| Name | Hex | Name | Hex |
|------|-----|------|-----|
| bg1 | `#ebdbb2` | fg0 | `#282828` |
| bg2 | `#d5c4a1` | fg1 | `#3c3836` |
| bg3 | `#bdae93` | fg2 | `#504945` |
| bg4 | `#a89984` | gray | `#928374` |
| red | `#9d0006` | green | `#79740e` |
| yellow | `#b57614` | blue | `#076678` |
| purple | `#8f3f71` | aqua | `#427b58` |
| orange | `#af3a03` | | |
