//! Integration tests: drive a real `nvim --embed` through `NvimSession`
//! (`docs/SPEC.md` §8). Needs Neovim 0.10+ on `PATH`.

#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gtk::glib;
use ringmd::model::Grid;
use ringmd::model::nvim::redraw;
use ringmd::model::nvim::{GuiCommand, NvimApi, NvimEvent, NvimSession, SpawnOptions, runtime};
use ringmd::model::theme::{ThemeChoice, Variant};
use rmpv::Value;

const TIMEOUT: Duration = Duration::from_secs(10);

/// A private home for one test: Neovim's `XDG_*` directories point inside
/// it, so tests never read or write the real user's files.
struct Home(PathBuf);

impl Home {
    fn new(test: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("ringmd-test-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.0.join(relative)
    }

    /// Write `contents` to `relative`, creating parent directories.
    fn write(&self, relative: &str, contents: &str) -> PathBuf {
        let path = self.path(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, contents).unwrap();
        path
    }

    fn xdg_env(&self) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
        ["CONFIG", "DATA", "STATE", "CACHE"]
            .into_iter()
            .map(|kind| {
                let dir = self.path(&kind.to_lowercase());
                (format!("XDG_{kind}_HOME").into(), dir.into())
            })
            .collect()
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A Neovim session plus the grid built from its redraws. Waiting helpers
/// fail with the screen contents and recent events, so a timeout in CI
/// shows what Neovim was doing.
struct Harness {
    _home: Home,
    nvim: NvimSession,
    events: async_channel::Receiver<NvimEvent>,
    grid: Grid,
    log: Vec<String>,
}

impl Harness {
    fn start(test: &str) -> Self {
        Self::start_in(Home::new(test), None)
    }

    fn start_in(home: Home, file: Option<&Path>) -> Self {
        let mut options = SpawnOptions::new(runtime::init_lua());
        options.file = file.map(Into::into);
        options.env = home.xdg_env();
        let (nvim, events) = NvimSession::spawn(&options).unwrap();
        Self {
            _home: home,
            nvim,
            events,
            grid: Grid::new(0, 0),
            log: Vec::new(),
        }
    }

    async fn attach(&mut self) {
        self.nvim.ui_attach(40, 6).await.unwrap();
    }

    /// Attach and wait for `VimEnter` (reported as buffer state), so
    /// startup, including plugin loading, has finished.
    async fn attach_ready(&mut self) {
        self.attach().await;
        self.wait_for("VimEnter", |event, _| {
            notification(event, "ringmd_buffer").map(|_| ())
        })
        .await;
    }

    async fn eval(&self, expr: &str) -> Value {
        self.nvim
            .request("nvim_eval", vec![expr.into()])
            .await
            .unwrap()
    }

    /// Wait for the next GUI command sent by the bundled config.
    async fn next_command(&mut self, what: &str) -> GuiCommand {
        self.wait_for(what, |event, _| match event {
            NvimEvent::Notification { method, params } => {
                GuiCommand::from_notification(method, params)
            }
            _ => None,
        })
        .await
    }

    /// Process events until `check` returns `Some`. Redraws are applied to
    /// the grid first; `check` sees every event.
    async fn wait_for<T>(
        &mut self,
        what: &str,
        mut check: impl FnMut(&NvimEvent, &Grid) -> Option<T>,
    ) -> T {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let event = match glib::future_with_timeout(left, self.events.recv()).await {
                Ok(Ok(event)) => event,
                Ok(Err(_)) => self.fail(what, "event channel closed"),
                Err(_) => self.fail(what, "timed out"),
            };
            match &event {
                NvimEvent::Notification { method, params } if method == "redraw" => {
                    for e in redraw::decode(params) {
                        self.grid.apply(&e);
                    }
                }
                other => self.log.push(format!("{other:?}")),
            }
            if let Some(found) = check(&event, &self.grid) {
                return found;
            }
        }
    }

    /// Wait until a flush leaves the grid matching `done`.
    async fn wait_for_screen(&mut self, what: &str, done: impl Fn(&Grid) -> bool) {
        self.wait_for(what, |event, grid| {
            let flushed =
                matches!(event, NvimEvent::Notification { method, .. } if method == "redraw");
            (flushed && done(grid)).then_some(())
        })
        .await;
    }

    async fn buffer_lines(&self) -> Value {
        self.nvim
            .request(
                "nvim_buf_get_lines",
                vec![0.into(), 0.into(), (-1).into(), false.into()],
            )
            .await
            .unwrap()
    }

    fn fail(&self, what: &str, why: &str) -> ! {
        let screen: Vec<String> = (0..self.grid.height())
            .map(|row| format!("  |{}|", self.grid.row_text(row)))
            .collect();
        let recent = self
            .log
            .iter()
            .rev()
            .take(10)
            .rev()
            .cloned()
            .collect::<Vec<_>>();
        panic!(
            "{why} waiting for {what}\nmode: {}\nscreen:\n{}\nrecent non-redraw events:\n  {}",
            self.grid.mode_name(),
            screen.join("\n"),
            recent.join("\n  "),
        );
    }
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    glib::MainContext::new().block_on(future)
}

fn notification<'a>(event: &'a NvimEvent, name: &str) -> Option<&'a [Value]> {
    match event {
        NvimEvent::Notification { method, params } if method == name => Some(params),
        _ => None,
    }
}

#[test]
fn typing_shows_up_in_the_grid() {
    let mut h = Harness::start("typing_shows_up_in_the_grid");
    block_on(async {
        h.attach().await;
        h.nvim.input("ihello <lt>world><Esc>");
        h.wait_for_screen("typed text", |g| {
            g.row_text(0).starts_with("hello <world>") && g.mode_name() == "normal"
        })
        .await;
        assert_eq!(h.grid.width(), 40);
        assert_eq!(h.grid.height(), 6);
        assert_eq!(h.grid.cursor().col, 12);
    });
}

#[test]
fn bundled_config_is_loaded() {
    let mut h = Harness::start("bundled_config_is_loaded");
    block_on(async {
        h.attach().await;
        let myvimrc = h
            .nvim
            .request("nvim_eval", vec!["$MYVIMRC".into()])
            .await
            .unwrap();
        let expected = runtime::init_lua();
        assert_eq!(myvimrc.as_str(), expected.to_str());
    });
}

#[test]
fn resize_changes_the_grid() {
    let mut h = Harness::start("resize_changes_the_grid");
    block_on(async {
        h.attach().await;
        h.nvim.ui_try_resize(30, 10);
        h.wait_for_screen("30x10 grid", |g| g.width() == 30 && g.height() == 10)
            .await;
    });
}

#[test]
fn clipboard_goes_through_the_gui() {
    let mut h = Harness::start("clipboard_goes_through_the_gui");
    block_on(async {
        h.attach().await;
        h.nvim.input("ione<Esc>\"+yy");
        let copied = h
            .wait_for("ringmd_clipboard_set", |event, _| {
                notification(event, "ringmd_clipboard_set").map(<[Value]>::to_vec)
            })
            .await;
        assert_eq!(
            copied,
            vec![Value::Array(vec!["one".into(), "".into()]), "V".into()]
        );

        // Paste: Neovim asks the GUI and blocks until it answers.
        h.nvim.input("\"+p");
        let id = h
            .wait_for("ringmd_clipboard_get", |event, _| match event {
                NvimEvent::Request { id, method, .. } if method == "ringmd_clipboard_get" => {
                    Some(*id)
                }
                _ => None,
            })
            .await;
        let answer = Value::Array(vec![Value::Array(vec!["pasted".into()]), "V".into()]);
        h.nvim.respond(id, Ok(answer));
        assert_eq!(
            h.buffer_lines().await,
            Value::Array(vec!["one".into(), "pasted".into()])
        );
    });
}

#[test]
fn buffer_state_is_reported() {
    let home = Home::new("buffer_state_is_reported");
    let file = home.write("notes.md", "# Notes\n");
    let mut h = Harness::start_in(home, Some(&file));
    block_on(async {
        h.attach().await;
        h.nvim.input("x");
        h.wait_for("modified buffer state", |event, _| {
            let state = notification(event, "ringmd_buffer")?.first()?.as_map()?;
            let get = |key: &str| {
                state
                    .iter()
                    .find(|(k, _)| k.as_str() == Some(key))
                    .map(|(_, v)| v.clone())
            };
            let name = get("name")?;
            assert!(name.as_str()?.ends_with("notes.md"), "{name}");
            (get("modified")?.as_bool() == Some(true)).then_some(())
        })
        .await;
    });
}

#[test]
fn quitting_reports_exit() {
    let mut h = Harness::start("quitting_reports_exit");
    block_on(async {
        h.attach().await;
        h.nvim.input(":qa!<CR>");
        let code = h
            .wait_for("exit", |event, _| match event {
                NvimEvent::Exited { code } => Some(*code),
                _ => None,
            })
            .await;
        assert_eq!(code, Some(0));
    });
}

#[test]
fn user_neovim_config_and_plugins_are_ignored() {
    let home = Home::new("user_neovim_config_and_plugins_are_ignored");
    home.write("config/nvim/init.lua", "vim.g.leak_init = 1");
    home.write("config/nvim/plugin/leak.lua", "vim.g.leak_plugin = 1");
    home.write(
        "data/nvim/site/pack/p/start/leak/plugin/leak.lua",
        "vim.g.leak_pack = 1",
    );
    home.write(
        "config/ringmd/plugin/leak.lua",
        "vim.g.leak_ringmd_plugin = 1",
    );
    home.write(
        "data/ringmd/site/pack/p/start/leak/plugin/leak.lua",
        "vim.g.leak_ringmd_pack = 1",
    );
    let state = home.path("state/ringmd");
    let mut h = Harness::start_in(home, None);
    block_on(async {
        h.attach_ready().await;
        for name in [
            "leak_init",
            "leak_plugin",
            "leak_pack",
            "leak_ringmd_plugin",
            "leak_ringmd_pack",
        ] {
            assert_eq!(
                h.eval(&format!("exists('g:{name}')")).await,
                0.into(),
                "{name}"
            );
        }
        // Shada, swap and undo live under RingMD's own state directory.
        assert_eq!(h.eval("stdpath('state')").await.as_str(), state.to_str(),);
        // Neovim's own runtime plugins still load.
        assert_eq!(h.eval("exists('g:loaded_matchparen')").await, 1.into());
    });
}

#[test]
fn ringmd_user_init_is_sourced_last() {
    let home = Home::new("ringmd_user_init_is_sourced_last");
    // Sees the bundled shiftwidth=2, then overrides it.
    home.write(
        "config/ringmd/init.lua",
        "vim.g.seen_shiftwidth = vim.o.shiftwidth\nvim.o.shiftwidth = 8",
    );
    let mut h = Harness::start_in(home, None);
    block_on(async {
        h.attach_ready().await;
        assert_eq!(h.eval("g:seen_shiftwidth").await, 2.into());
        assert_eq!(h.eval("&shiftwidth").await, 8.into());
        assert_eq!(h.eval("exists(':Preview')").await, 2.into());
    });
}

#[test]
fn markdown_defaults_apply() {
    let home = Home::new("markdown_defaults_apply");
    let file = home.write("notes.md", "# Notes\n\n- item\n");
    let mut h = Harness::start_in(home, Some(&file));
    block_on(async {
        h.attach_ready().await;
        let options = "[&wrap, &linebreak, &spell, &conceallevel, &expandtab, \
                       &shiftwidth, &number, &termguicolors, &filetype]";
        let expected = Value::Array(vec![
            1.into(),
            1.into(),
            0.into(),
            0.into(),
            1.into(),
            2.into(),
            0.into(),
            1.into(),
            "markdown".into(),
        ]);
        assert_eq!(h.eval(options).await, expected);
        // Neovim 0.10+ ships the markdown parser, so Treesitter is active.
        let treesitter =
            "luaeval('vim.treesitter.highlighter.active[vim.api.nvim_get_current_buf()] ~= nil')";
        assert_eq!(h.eval(treesitter).await, true.into());
    });
}

#[test]
fn unnamed_buffers_are_markdown() {
    let mut h = Harness::start("unnamed_buffers_are_markdown");
    block_on(async {
        h.attach_ready().await;
        assert_eq!(h.eval("&filetype").await, "markdown".into());
        // Change the first buffer so `:enew` really creates a second one.
        let set_lines = vec![
            0.into(),
            0.into(),
            (-1).into(),
            false.into(),
            Value::Array(vec!["# one".into()]),
        ];
        h.nvim
            .request("nvim_buf_set_lines", set_lines)
            .await
            .unwrap();
        h.nvim
            .request("nvim_command", vec!["enew".into()])
            .await
            .unwrap();
        assert_eq!(
            h.eval("[bufnr(), &filetype]").await,
            Value::Array(vec![2.into(), "markdown".into()])
        );
    });
}

#[test]
fn commands_complete() {
    let mut h = Harness::start("commands_complete");
    block_on(async {
        h.attach_ready().await;
        for command in ["Preview", "ExportPdf", "Theme"] {
            assert_eq!(
                h.eval(&format!("exists(':{command}')")).await,
                2.into(),
                "{command}"
            );
        }
        let all: Vec<Value> = ThemeChoice::names().into_iter().map(Into::into).collect();
        assert_eq!(
            h.eval("getcompletion('Theme ', 'cmdline')").await,
            Value::Array(all)
        );
        assert_eq!(
            h.eval("getcompletion('Theme light-s', 'cmdline')").await,
            Value::Array(vec!["light-soft".into()])
        );
    });
}

#[test]
fn commands_reach_the_gui() {
    let home = Home::new("commands_reach_the_gui");
    let dir = home.path("");
    let mut h = Harness::start_in(home, None);
    block_on(async {
        h.attach_ready().await;
        h.nvim.input(":Preview<CR>");
        assert_eq!(h.next_command(":Preview").await, GuiCommand::TogglePreview);

        // An unknown variant is rejected in Neovim and never reaches the GUI.
        h.nvim.input(":Theme bogus<CR><CR>:Theme light-soft<CR>");
        assert_eq!(
            h.next_command(":Theme").await,
            GuiCommand::SetTheme(ThemeChoice::Fixed(Variant::LightSoft))
        );

        h.nvim.input(":ExportPdf<CR>");
        assert_eq!(
            h.next_command(":ExportPdf").await,
            GuiCommand::ExportPdf { path: None }
        );

        // Relative paths are resolved against Neovim's working directory.
        let cd = format!(":cd {}<CR>:ExportPdf out.pdf<CR>", dir.display());
        h.nvim.input(&cd);
        let expected = std::fs::canonicalize(&dir).unwrap().join("out.pdf");
        assert_eq!(
            h.next_command(":ExportPdf out.pdf").await,
            GuiCommand::ExportPdf {
                path: Some(expected)
            }
        );
    });
}

/// The `text` of a `ringmd_text` notification.
fn preview_text(event: &NvimEvent) -> Option<String> {
    let state = notification(event, "ringmd_text")?.first()?.as_map()?;
    let (_, text) = state.iter().find(|(k, _)| k.as_str() == Some("text"))?;
    Some(text.as_str()?.to_owned())
}

#[test]
fn preview_feed_sends_text_and_cursor() {
    let home = Home::new("preview_feed_sends_text_and_cursor");
    let file = home.write("notes.md", "# One\n\ntwo\n");
    let mut h = Harness::start_in(home, Some(&file));
    block_on(async {
        h.attach().await;
        // Entering the buffer sends its text right away.
        let text = h
            .wait_for("initial text", |event, _| preview_text(event))
            .await;
        assert_eq!(text, "# One\n\ntwo\n");

        // Edits arrive after the debounce, as one message with the result.
        h.nvim.input("Goadded<Esc>");
        let text = h
            .wait_for("edited text", |event, _| {
                preview_text(event).filter(|t| t.contains("added"))
            })
            .await;
        assert_eq!(text, "# One\n\ntwo\nadded\n");

        h.nvim.input("gg");
        let line = h
            .wait_for("cursor line 1", |event, _| {
                notification(event, "ringmd_cursor")?
                    .first()?
                    .as_u64()
                    .filter(|&l| l == 1)
            })
            .await;
        assert_eq!(line, 1);
    });
}

#[test]
fn ctrl_p_toggles_preview_in_normal_mode_only() {
    let mut h = Harness::start("ctrl_p_toggles_preview_in_normal_mode_only");
    block_on(async {
        h.attach_ready().await;
        // In Insert mode Ctrl+P keeps Neovim's meaning (completion), so
        // the first command seen must come from the Normal-mode press.
        h.nvim.input("i<C-p><Esc>:Theme dark-soft<CR><C-p>");
        assert_eq!(
            h.next_command("first command").await,
            GuiCommand::SetTheme(ThemeChoice::Fixed(Variant::DarkSoft))
        );
        assert_eq!(h.next_command("Ctrl+P").await, GuiCommand::TogglePreview);
    });
}

#[test]
fn theme_highlights_apply_in_neovim() {
    let mut h = Harness::start("theme_highlights_apply_in_neovim");
    block_on(async {
        h.attach().await;
        h.wait_for("ringmd_ready", |event, _| {
            notification(event, "ringmd_ready").map(|_| ())
        })
        .await;
        for (variant, background, normal_bg) in [
            (Variant::LightSoft, "light", 0xf2e5bc),
            (Variant::DarkHard, "dark", 0x1d2021),
        ] {
            h.nvim
                .request(
                    "nvim_exec_lua",
                    ringmd::model::theme::apply_highlights_args(variant),
                )
                .await
                .unwrap();
            assert_eq!(h.eval("&background").await, background.into());
            assert_eq!(h.eval("g:colors_name").await, "ringmd-gruvbox".into());
            let normal = h
                .nvim
                .request(
                    "nvim_get_hl",
                    vec![0.into(), Value::Map(vec![("name".into(), "Normal".into())])],
                )
                .await
                .unwrap();
            let bg = normal
                .as_map()
                .unwrap()
                .iter()
                .find(|(k, _)| k.as_str() == Some("bg"))
                .and_then(|(_, v)| v.as_u64());
            assert_eq!(bg, Some(normal_bg), "{variant}");
        }
        // Treesitter captures resolve to the Markdown groups.
        let heading = h
            .eval("luaeval('vim.api.nvim_get_hl(0, {name = \"@markup.heading.1\"}).link')")
            .await;
        assert_eq!(heading, "markdownH1".into());
    });
}
