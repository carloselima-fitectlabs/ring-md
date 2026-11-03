//! Routes Neovim events to the ViewModels (rule A5).

use std::rc::Rc;

use gtk::glib;
use rmpv::Value;

use super::{AppViewModel, EditorServices, EditorViewModel};
use crate::model::nvim::{GuiCommand, NvimApi, NvimEvent, NvimSession, SpawnOptions};
use crate::services::ClipboardService;

/// Start Neovim and wire it to `app` and the returned editor. On failure
/// the error is reported through `app` once the main loop runs (so the
/// window is listening) and the editor stays detached (F2.R3).
pub fn start(
    app: &AppViewModel,
    options: &SpawnOptions,
    clipboard: Rc<dyn ClipboardService>,
) -> EditorViewModel {
    let (session, events) = match NvimSession::spawn(options) {
        Ok(started) => started,
        Err(e) => {
            let app = app.clone();
            glib::idle_add_local_once(move || {
                app.report_error("Could not start Neovim", &e.to_string());
            });
            return EditorViewModel::new(EditorServices {
                nvim: Rc::new(Detached),
                clipboard,
            });
        }
    };
    let nvim: Rc<dyn NvimApi> = Rc::new(session);
    let editor = EditorViewModel::new(EditorServices {
        nvim: nvim.clone(),
        clipboard,
    });

    let (app, editor_ref) = (app.clone(), editor.clone());
    glib::spawn_future_local(async move {
        while let Ok(event) = events.recv().await {
            dispatch(&app, &editor_ref, nvim.as_ref(), event);
        }
    });
    editor
}

/// Handle one event from Neovim.
pub fn dispatch(
    app: &AppViewModel,
    editor: &EditorViewModel,
    nvim: &dyn NvimApi,
    event: NvimEvent,
) {
    match event {
        NvimEvent::Notification { method, params } => {
            if let Some(command) = GuiCommand::from_notification(&method, &params) {
                run_command(app, nvim, command);
                return;
            }
            match method.as_str() {
                "redraw" => editor.handle_redraw(&params),
                "ringmd_buffer" => {
                    app.apply_buffer_state(params.first().unwrap_or(&Value::Nil));
                }
                "ringmd_clipboard_set" => editor.clipboard_set(&params),
                "nvim_error_event" => tracing::warn!(?params, "neovim reported an error"),
                other => tracing::debug!(method = other, "unhandled notification"),
            }
        }
        NvimEvent::Request { id, method, .. } => match method.as_str() {
            "ringmd_clipboard_get" => {
                let editor = editor.clone();
                glib::spawn_future_local(async move { editor.clipboard_get(id).await });
            }
            other => {
                tracing::warn!(method = other, "unknown request from neovim");
                nvim.respond(id, Err(format!("unknown method: {other}").into()));
            }
        },
        NvimEvent::Exited { code } => app.neovim_exited(code),
    }
}

/// Route a `:Preview` / `:ExportPdf` / `:Theme` command to the same
/// ViewModel command a GUI shortcut uses (rule A5).
fn run_command(app: &AppViewModel, nvim: &dyn NvimApi, command: GuiCommand) {
    tracing::debug!(?command, "command from neovim");
    match command {
        GuiCommand::TogglePreview => app.toggle_preview(),
        // Themes (M4) and PDF export (M5) are not built yet; say so where
        // the user typed the command.
        GuiCommand::SetTheme(_) => echo_warning(nvim, "RingMD: themes are not available yet"),
        GuiCommand::ExportPdf { .. } => {
            echo_warning(nvim, "RingMD: PDF export is not available yet");
        }
    }
}

fn echo_warning(nvim: &dyn NvimApi, text: &str) {
    let chunk = Value::Array(vec![text.into(), "WarningMsg".into()]);
    nvim.notify(
        "nvim_echo",
        vec![Value::Array(vec![chunk]), true.into(), Value::Map(vec![])],
    );
}

/// Stand-in used when Neovim failed to start: drops every call.
#[derive(Debug)]
struct Detached;

impl NvimApi for Detached {
    fn notify(&self, _method: &str, _args: Vec<Value>) {}

    fn request(
        &self,
        _method: &str,
        _args: Vec<Value>,
    ) -> crate::services::LocalBoxFuture<'static, Result<Value, crate::model::nvim::NvimError>>
    {
        Box::pin(async { Err(crate::model::nvim::NvimError::Closed) })
    }

    fn respond(&self, _id: u64, _result: Result<Value, Value>) {}
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::model::Document;
    use crate::model::nvim::fake::{Call, FakeNvim};
    use crate::services::fake::FakeClipboardService;

    fn setup() -> (AppViewModel, EditorViewModel, Rc<FakeNvim>) {
        let nvim = Rc::new(FakeNvim::new());
        let editor = EditorViewModel::new(EditorServices {
            nvim: nvim.clone(),
            clipboard: Rc::new(FakeClipboardService::default()),
        });
        (AppViewModel::new(Document::new()), editor, nvim)
    }

    fn notification(method: &str, params: Vec<Value>) -> NvimEvent {
        NvimEvent::Notification {
            method: method.into(),
            params,
        }
    }

    #[test]
    fn buffer_state_updates_title() {
        let (app, editor, nvim) = setup();
        let state = Value::Map(vec![
            ("name".into(), "/home/me/notes.md".into()),
            ("modified".into(), true.into()),
        ]);
        dispatch(
            &app,
            &editor,
            nvim.as_ref(),
            notification("ringmd_buffer", vec![state]),
        );
        assert_eq!(app.title(), "notes.md+ — RingMD");
    }

    #[test]
    fn preview_command_toggles_preview() {
        let (app, editor, nvim) = setup();
        assert!(app.preview_visible());
        dispatch(
            &app,
            &editor,
            nvim.as_ref(),
            notification("ringmd_preview", vec![]),
        );
        assert!(!app.preview_visible());
        assert!(nvim.take_calls().is_empty());
    }

    #[test]
    fn unbuilt_commands_warn_in_neovim() {
        let (app, editor, nvim) = setup();
        dispatch(
            &app,
            &editor,
            nvim.as_ref(),
            notification("ringmd_theme", vec!["light-soft".into()]),
        );
        let calls = nvim.take_calls();
        assert!(
            matches!(&calls[..], [Call::Notify(method, _)] if method == "nvim_echo"),
            "{calls:?}"
        );
    }

    #[test]
    fn unknown_requests_get_an_error() {
        let (app, editor, nvim) = setup();
        dispatch(
            &app,
            &editor,
            nvim.as_ref(),
            NvimEvent::Request {
                id: 3,
                method: "bogus".into(),
                params: vec![],
            },
        );
        assert_eq!(
            nvim.take_calls(),
            [Call::Respond(3, Err("unknown method: bogus".into()))]
        );
    }

    #[test]
    fn exit_closes_and_crash_reports() {
        let (app, editor, nvim) = setup();
        let closes = Rc::new(Cell::new(0));
        let errors = Rc::new(Cell::new(0));
        let (c, e) = (closes.clone(), errors.clone());
        app.connect_close_requested(move |_| c.set(c.get() + 1));
        app.connect_error_raised(move |_, _, _| e.set(e.get() + 1));

        dispatch(
            &app,
            &editor,
            nvim.as_ref(),
            NvimEvent::Exited { code: Some(0) },
        );
        assert_eq!((closes.get(), errors.get()), (1, 0));

        dispatch(
            &app,
            &editor,
            nvim.as_ref(),
            NvimEvent::Exited { code: None },
        );
        assert_eq!((closes.get(), errors.get()), (1, 1));
    }
}
