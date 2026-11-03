//! Integration tests: drive a real `nvim --embed` through `NvimSession`
//! (`docs/SPEC.md` §8). Needs Neovim 0.10+ on `PATH`.

#![allow(clippy::unwrap_used)]

use std::future::Future;
use std::time::Duration;

use gtk::glib;
use ringmd::model::Grid;
use ringmd::model::nvim::redraw;
use ringmd::model::nvim::{NvimApi, NvimEvent, NvimSession, SpawnOptions, runtime};
use rmpv::Value;

const TIMEOUT: Duration = Duration::from_secs(10);

fn run<F: Future>(future: F) -> F::Output {
    glib::MainContext::new().block_on(async move {
        glib::future_with_timeout(TIMEOUT, future)
            .await
            .expect("timed out waiting for neovim")
    })
}

fn spawn(file: Option<&std::path::Path>) -> (NvimSession, async_channel::Receiver<NvimEvent>) {
    let mut options = SpawnOptions::new(runtime::init_lua());
    options.file = file.map(Into::into);
    NvimSession::spawn(&options).unwrap()
}

/// Apply redraw events until a flush leaves the grid matching `done`.
async fn redraw_until(
    events: &async_channel::Receiver<NvimEvent>,
    grid: &mut Grid,
    mut on_other: impl FnMut(NvimEvent),
    done: impl Fn(&Grid) -> bool,
) {
    loop {
        match events.recv().await.unwrap() {
            NvimEvent::Notification { method, params } if method == "redraw" => {
                let mut flushed = false;
                for event in redraw::decode(&params) {
                    flushed |= grid.apply(&event);
                }
                if flushed && done(grid) {
                    return;
                }
            }
            other => on_other(other),
        }
    }
}

#[test]
fn typing_shows_up_in_the_grid() {
    let (nvim, events) = spawn(None);
    run(async {
        nvim.ui_attach(40, 6).await.unwrap();
        nvim.input("ihello <lt>world><Esc>");
        let mut grid = Grid::new(0, 0);
        redraw_until(
            &events,
            &mut grid,
            |_| {},
            |g| g.row_text(0).starts_with("hello <world>") && g.mode_name() == "normal",
        )
        .await;
        assert_eq!(grid.width(), 40);
        assert_eq!(grid.height(), 6);
        assert_eq!(grid.cursor().col, 12);
    });
}

#[test]
fn bundled_config_is_loaded() {
    let (nvim, _events) = spawn(None);
    run(async {
        nvim.ui_attach(40, 6).await.unwrap();
        let myvimrc = nvim
            .request("nvim_eval", vec!["$MYVIMRC".into()])
            .await
            .unwrap();
        let expected = runtime::init_lua();
        assert_eq!(myvimrc.as_str(), expected.to_str());
    });
}

#[test]
fn resize_changes_the_grid() {
    let (nvim, events) = spawn(None);
    run(async {
        nvim.ui_attach(40, 6).await.unwrap();
        nvim.ui_try_resize(30, 10);
        let mut grid = Grid::new(0, 0);
        redraw_until(
            &events,
            &mut grid,
            |_| {},
            |g| g.width() == 30 && g.height() == 10,
        )
        .await;
    });
}

#[test]
fn clipboard_goes_through_the_gui() {
    let (nvim, events) = spawn(None);
    run(async {
        nvim.ui_attach(40, 6).await.unwrap();
        nvim.input("ione<Esc>\"+yy");
        let copied = loop {
            if let NvimEvent::Notification { method, params } = events.recv().await.unwrap()
                && method == "ringmd_clipboard_set"
            {
                break params;
            }
        };
        assert_eq!(
            copied,
            vec![Value::Array(vec!["one".into(), "".into()]), "V".into()]
        );

        // Paste: Neovim asks the GUI and blocks until it answers.
        nvim.input("\"+p");
        loop {
            if let NvimEvent::Request { id, method, .. } = events.recv().await.unwrap()
                && method == "ringmd_clipboard_get"
            {
                let answer = Value::Array(vec![Value::Array(vec!["pasted".into()]), "V".into()]);
                nvim.respond(id, Ok(answer));
                break;
            }
        }
        let lines = nvim
            .request(
                "nvim_buf_get_lines",
                vec![0.into(), 0.into(), (-1).into(), false.into()],
            )
            .await
            .unwrap();
        assert_eq!(lines, Value::Array(vec!["one".into(), "pasted".into()]));
    });
}

#[test]
fn buffer_state_is_reported() {
    let dir = std::env::temp_dir().join(format!("ringmd-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("notes.md");
    std::fs::write(&file, "# Notes\n").unwrap();

    let (nvim, events) = spawn(Some(&file));
    run(async {
        nvim.ui_attach(40, 6).await.unwrap();
        nvim.input("x");
        loop {
            if let NvimEvent::Notification { method, params } = events.recv().await.unwrap()
                && method == "ringmd_buffer"
            {
                let state = params[0].as_map().unwrap();
                let get = |key: &str| {
                    state
                        .iter()
                        .find(|(k, _)| k.as_str() == Some(key))
                        .map(|(_, v)| v.clone())
                        .unwrap()
                };
                assert!(get("name").as_str().unwrap().ends_with("notes.md"));
                if get("modified").as_bool() == Some(true) {
                    break;
                }
            }
        }
    });
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn quitting_reports_exit() {
    let (nvim, events) = spawn(None);
    run(async {
        nvim.ui_attach(40, 6).await.unwrap();
        nvim.input(":qa!<CR>");
        loop {
            if let NvimEvent::Exited { code } = events.recv().await.unwrap() {
                assert_eq!(code, Some(0));
                break;
            }
        }
    });
}
