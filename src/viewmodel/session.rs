//! Routes Neovim events to the ViewModels (rule A5).

use std::rc::Rc;

use gtk::glib;
use rmpv::Value;

use super::{
    AppViewModel, EditorServices, EditorViewModel, ExportViewModel, PreviewViewModel,
    ThemeViewModel,
};
use crate::model::nvim::{GuiCommand, NvimApi, NvimEvent, NvimSession, SpawnOptions};
use crate::services::{ClipboardService, FileDialogService};

/// The ViewModels Neovim's events are routed to.
#[derive(Debug, Clone)]
pub struct ViewModels {
    pub app: AppViewModel,
    pub editor: EditorViewModel,
    pub preview: PreviewViewModel,
    pub theme: ThemeViewModel,
    pub export: ExportViewModel,
}

/// The ViewModels that exist before Neovim starts.
#[derive(Debug, Clone)]
pub struct Targets {
    pub app: AppViewModel,
    pub preview: PreviewViewModel,
    pub theme: ThemeViewModel,
    pub export: ExportViewModel,
}

/// Start Neovim and wire it to `targets` and the returned editor. On
/// failure the error is reported through the app ViewModel once the main
/// loop runs (so the window is listening) and the editor stays detached
/// (F2.R3).
pub fn start(
    targets: &Targets,
    options: &SpawnOptions,
    clipboard: Rc<dyn ClipboardService>,
    dialogs: Rc<dyn FileDialogService>,
) -> EditorViewModel {
    let app = &targets.app;
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
    targets.theme.attach_neovim(nvim.clone());
    targets.export.attach_neovim(nvim.clone());
    targets.app.set_file_services(nvim.clone(), dialogs);
    let editor = EditorViewModel::new(EditorServices {
        nvim: nvim.clone(),
        clipboard,
    });

    let vms = ViewModels {
        app: app.clone(),
        editor: editor.clone(),
        preview: targets.preview.clone(),
        theme: targets.theme.clone(),
        export: targets.export.clone(),
    };
    glib::spawn_future_local(async move {
        while let Ok(event) = events.recv().await {
            dispatch(&vms, nvim.as_ref(), event);
        }
    });
    editor
}

/// Handle one event from Neovim.
pub fn dispatch(vms: &ViewModels, nvim: &dyn NvimApi, event: NvimEvent) {
    let ViewModels {
        app,
        editor,
        preview,
        theme,
        ..
    } = vms;
    match event {
        NvimEvent::Notification { method, params } => {
            if let Some(command) = GuiCommand::from_notification(&method, &params) {
                run_command(vms, command);
                return;
            }
            match method.as_str() {
                "redraw" => editor.handle_redraw(&params),
                "ringmd_buffer" => {
                    app.apply_buffer_state(params.first().unwrap_or(&Value::Nil));
                    preview.set_document_path(app.document().path());
                }
                "ringmd_text" => preview.apply_text_event(&params),
                "ringmd_ready" => theme.neovim_ready(),
                "ringmd_cursor" => {
                    let line = params.first().and_then(Value::as_u64).unwrap_or(1);
                    preview.set_cursor_line(u32::try_from(line).unwrap_or(u32::MAX));
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
fn run_command(vms: &ViewModels, command: GuiCommand) {
    tracing::debug!(?command, "command from neovim");
    match command {
        GuiCommand::TogglePreview => vms.app.toggle_preview(),
        GuiCommand::SetTheme(choice) => vms.theme.set_choice(choice),
        GuiCommand::ToggleFullscreen => vms.app.toggle_fullscreen(),
        GuiCommand::Open => {
            let app = vms.app.clone();
            glib::spawn_future_local(async move { app.open_with_chooser().await });
        }
        GuiCommand::SaveAs => {
            let app = vms.app.clone();
            glib::spawn_future_local(async move {
                app.save_as().await;
            });
        }
        GuiCommand::ExportPdf { path } => {
            let export = vms.export.clone();
            let document = vms.app.document().path().map(Into::into);
            glib::spawn_future_local(async move { export.export(path, document).await });
        }
    }
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
    use crate::model::theme::{ThemeChoice, Variant};
    use crate::services::fake::{
        FakeClipboardService, FakeFileDialogService, FakeLinkService, FakePdfService,
        FakeSettingsStore,
    };
    use crate::viewmodel::ExportServices;

    fn setup() -> (ViewModels, Rc<FakeNvim>) {
        let nvim = Rc::new(FakeNvim::new());
        let editor = EditorViewModel::new(EditorServices {
            nvim: nvim.clone(),
            clipboard: Rc::new(FakeClipboardService::default()),
        });
        let vms = ViewModels {
            app: AppViewModel::new(Document::new()),
            editor,
            preview: PreviewViewModel::new(Rc::new(FakeLinkService::default())),
            theme: ThemeViewModel::new(
                ThemeChoice::default(),
                Rc::new(FakeSettingsStore::default()),
            ),
            export: ExportViewModel::new(ExportServices {
                pdf: Rc::new(FakePdfService::default()),
                dialogs: Rc::new(FakeFileDialogService::default()),
                options: Default::default(),
            }),
        };
        (vms, nvim)
    }

    fn notification(method: &str, params: Vec<Value>) -> NvimEvent {
        NvimEvent::Notification {
            method: method.into(),
            params,
        }
    }

    #[test]
    fn buffer_state_updates_title() {
        let (vms, nvim) = setup();
        let app = &vms.app;
        let state = Value::Map(vec![
            ("name".into(), "/home/me/notes.md".into()),
            ("modified".into(), true.into()),
        ]);
        dispatch(
            &vms,
            nvim.as_ref(),
            notification("ringmd_buffer", vec![state]),
        );
        assert_eq!(app.title(), "notes.md+ — RingMD");
    }

    #[test]
    fn preview_command_toggles_preview() {
        let (vms, nvim) = setup();
        let app = &vms.app;
        assert!(app.preview_visible());
        dispatch(&vms, nvim.as_ref(), notification("ringmd_preview", vec![]));
        assert!(!app.preview_visible());
        assert!(nvim.take_calls().is_empty());
    }

    #[test]
    fn theme_command_switches_variant() {
        let (vms, nvim) = setup();
        dispatch(
            &vms,
            nvim.as_ref(),
            notification("ringmd_theme", vec!["light-soft".into()]),
        );
        assert_eq!(vms.theme.current_variant(), Variant::LightSoft);
    }

    #[test]
    fn export_command_starts_an_export() {
        let (vms, nvim) = setup();
        let finished = Rc::new(Cell::new(false));
        let f = finished.clone();
        vms.export.connect_finished(move |_, _, _| f.set(true));
        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                dispatch(
                    &vms,
                    nvim.as_ref(),
                    notification("ringmd_export_pdf", vec!["/tmp/x.pdf".into()]),
                );
                // The export runs on the main loop; without Neovim attached it
                // reports an error rather than hanging.
                while !finished.get() {
                    context.iteration(true);
                }
            })
            .unwrap();
    }

    #[test]
    fn preview_follows_buffer_and_cursor() {
        let (vms, nvim) = setup();
        let state = Value::Map(vec![
            ("name".into(), "/home/me/notes/a.md".into()),
            ("modified".into(), false.into()),
        ]);
        dispatch(
            &vms,
            nvim.as_ref(),
            notification("ringmd_buffer", vec![state]),
        );
        assert_eq!(vms.preview.base_uri(), "file:///home/me/notes/");
        dispatch(
            &vms,
            nvim.as_ref(),
            notification("ringmd_cursor", vec![7.into()]),
        );
        assert_eq!(vms.preview.scroll_target_line(), 7);
    }

    #[test]
    fn unknown_requests_get_an_error() {
        let (vms, nvim) = setup();
        dispatch(
            &vms,
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
        let (vms, nvim) = setup();
        let app = &vms.app;
        let closes = Rc::new(Cell::new(0));
        let errors = Rc::new(Cell::new(0));
        let (c, e) = (closes.clone(), errors.clone());
        app.connect_close_requested(move |_| c.set(c.get() + 1));
        app.connect_error_raised(move |_, _, _| e.set(e.get() + 1));

        dispatch(&vms, nvim.as_ref(), NvimEvent::Exited { code: Some(0) });
        assert_eq!((closes.get(), errors.get()), (1, 0));

        dispatch(&vms, nvim.as_ref(), NvimEvent::Exited { code: None });
        assert_eq!((closes.get(), errors.get()), (1, 1));
    }
}
