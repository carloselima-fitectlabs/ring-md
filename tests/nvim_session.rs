//! Integration tests: drive a real `nvim --embed` through `NvimSession`
//! (`docs/SPEC.md` §8). Needs Neovim 0.10+ on `PATH`.

#![allow(clippy::unwrap_used)]

use std::time::{Duration, Instant};

use gtk::glib;
use ringmd::model::Grid;
use ringmd::model::nvim::redraw;
use ringmd::model::nvim::{NvimApi, NvimEvent, NvimSession, SpawnOptions, runtime};
use rmpv::Value;

const TIMEOUT: Duration = Duration::from_secs(10);

/// A Neovim session plus the grid built from its redraws. Waiting helpers
/// fail with the screen contents and recent events, so a timeout in CI
/// shows what Neovim was doing.
struct Harness {
    nvim: NvimSession,
    events: async_channel::Receiver<NvimEvent>,
    grid: Grid,
    log: Vec<String>,
}

impl Harness {
    fn start(file: Option<&std::path::Path>) -> Self {
        let mut options = SpawnOptions::new(runtime::init_lua());
        options.file = file.map(Into::into);
        let (nvim, events) = NvimSession::spawn(&options).unwrap();
        Self {
            nvim,
            events,
            grid: Grid::new(0, 0),
            log: Vec::new(),
        }
    }

    async fn attach(&mut self) {
        self.nvim.ui_attach(40, 6).await.unwrap();
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
    let mut h = Harness::start(None);
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
    let mut h = Harness::start(None);
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
    let mut h = Harness::start(None);
    block_on(async {
        h.attach().await;
        h.nvim.ui_try_resize(30, 10);
        h.wait_for_screen("30x10 grid", |g| g.width() == 30 && g.height() == 10)
            .await;
    });
}

#[test]
fn clipboard_goes_through_the_gui() {
    let mut h = Harness::start(None);
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
    let dir = std::env::temp_dir().join(format!("ringmd-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("notes.md");
    std::fs::write(&file, "# Notes\n").unwrap();

    let mut h = Harness::start(Some(&file));
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
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn quitting_reports_exit() {
    let mut h = Harness::start(None);
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
