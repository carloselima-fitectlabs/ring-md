//! File commands on [`AppViewModel`] (M6): open, save as, drag and drop,
//! and the unsaved-changes flow when the window closes (F1.R5, F7).
//!
//! Neovim stays the source of truth: these commands only choose paths and
//! ask the bundled config (`ringmd.open`, `ringmd.save`, `ringmd.unsaved`)
//! to act on its buffers.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk::glib;
use gtk::glib::prelude::*;
use gtk::glib::subclass::prelude::*;
use rmpv::Value;

use super::app::{AppViewModel, SIGNAL_CONFIRM_CLOSE};
use crate::model::nvim::NvimApi;
use crate::services::FileDialogService;

/// The user's answer to the unsaved-changes dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseChoice {
    Save,
    Discard,
    Cancel,
}

/// A modified buffer as reported by `ringmd.unsaved()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unsaved {
    pub buf: i64,
    /// Empty for an unnamed buffer.
    pub name: String,
}

impl Unsaved {
    fn label(&self) -> String {
        if self.name.is_empty() {
            "[No Name]".to_owned()
        } else {
            Path::new(&self.name)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| self.name.clone())
        }
    }
}

fn decode_unsaved(value: &Value) -> Vec<Unsaved> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let map = item.as_map()?;
                    let get = |key: &str| {
                        map.iter()
                            .find(|(k, _)| k.as_str() == Some(key))
                            .map(|(_, v)| v)
                    };
                    Some(Unsaved {
                        buf: get("buf")?.as_i64()?,
                        name: get("name")?.as_str()?.to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `nvim_exec_lua` arguments calling `ringmd.<function>(args...)`.
fn lua_call(function: &str, args: Vec<Value>) -> Vec<Value> {
    let params = (1..=args.len())
        .map(|i| format!("select({i}, ...)"))
        .collect::<Vec<_>>()
        .join(", ");
    vec![
        format!("return ringmd.{function}({params})").into(),
        Value::Array(args),
    ]
}

impl AppViewModel {
    /// The services the file commands need.
    pub fn set_file_services(&self, nvim: Rc<dyn NvimApi>, dialogs: Rc<dyn FileDialogService>) {
        self.imp().nvim.replace(Some(nvim));
        self.imp().dialogs.replace(Some(dialogs));
    }

    fn nvim(&self) -> Option<Rc<dyn NvimApi>> {
        self.imp().nvim.borrow().clone()
    }

    fn dialogs(&self) -> Option<Rc<dyn FileDialogService>> {
        self.imp().dialogs.borrow().clone()
    }

    /// Call a `ringmd.*` helper that returns an error message or nil.
    async fn call_helper(&self, function: &str, args: Vec<Value>) -> Result<(), String> {
        let nvim = self.nvim().ok_or("the editor is not running")?;
        let reply = nvim
            .request("nvim_exec_lua", lua_call(function, args))
            .await
            .map_err(|e| e.to_string())?;
        match reply.as_str() {
            Some(error) => Err(error.to_owned()),
            None => Ok(()),
        }
    }

    /// Command: open `path` in the editor (drag and drop, F7.R4).
    pub async fn open_file(&self, path: &Path) {
        let path = path.to_string_lossy().into_owned();
        if let Err(e) = self.call_helper("open", vec![path.into()]).await {
            self.report_error("Could not open the file", &e);
        }
    }

    /// Command: `Ctrl+O` / the Open menu item (F7.R2).
    pub async fn open_with_chooser(&self) {
        let Some(dialogs) = self.dialogs() else {
            return;
        };
        if let Some(path) = dialogs.pick_open().await {
            self.open_file(&path).await;
        }
    }

    /// Command: `Ctrl+S` on an unnamed buffer (F7.R2, F7-AC1).
    pub async fn save_as(&self) -> bool {
        self.save_buffer_as(0, "Untitled.md").await
    }

    async fn save_buffer_as(&self, buf: i64, suggested: &str) -> bool {
        let Some(dialogs) = self.dialogs() else {
            return false;
        };
        let Some(path) = dialogs.pick_save(suggested).await else {
            return false;
        };
        let path = path.to_string_lossy().into_owned();
        match self
            .call_helper("save", vec![buf.into(), path.into()])
            .await
        {
            Ok(()) => true,
            Err(e) => {
                self.report_error("Could not save the file", &e);
                false
            }
        }
    }

    /// Whether the window may close right now: only after Neovim exited.
    pub fn ready_to_close(&self) -> bool {
        self.imp().neovim_gone.get() || self.nvim().is_none()
    }

    /// The user closed the window (F1.R5). Without unsaved changes Neovim
    /// quits, which then closes the window; otherwise the View is asked to
    /// confirm via [`SIGNAL_CONFIRM_CLOSE`].
    pub async fn request_close(&self) {
        tracing::debug!("close requested");
        let Some(nvim) = self.nvim() else { return };
        let unsaved = match nvim
            .request("nvim_exec_lua", lua_call("unsaved", vec![]))
            .await
        {
            Ok(reply) => decode_unsaved(&reply),
            Err(e) => {
                tracing::warn!(error = %e, "could not list unsaved buffers; quitting");
                Vec::new()
            }
        };
        tracing::debug!(?unsaved, "unsaved buffers");
        if unsaved.is_empty() {
            nvim.notify("nvim_command", vec!["qa".into()]);
            return;
        }
        let names = unsaved
            .iter()
            .map(Unsaved::label)
            .collect::<Vec<_>>()
            .join(", ");
        self.emit_by_name::<()>(SIGNAL_CONFIRM_CLOSE, &[&names]);
    }

    /// The answer to the unsaved-changes dialog.
    pub async fn resolve_close(&self, choice: CloseChoice) {
        let Some(nvim) = self.nvim() else { return };
        match choice {
            CloseChoice::Cancel => {}
            CloseChoice::Discard => nvim.notify("nvim_command", vec!["qa!".into()]),
            CloseChoice::Save => {
                let unsaved = match nvim
                    .request("nvim_exec_lua", lua_call("unsaved", vec![]))
                    .await
                {
                    Ok(reply) => decode_unsaved(&reply),
                    Err(_) => return,
                };
                for buffer in unsaved {
                    let saved = if buffer.name.is_empty() {
                        self.save_buffer_as(buffer.buf, "Untitled.md").await
                    } else {
                        match self.call_helper("save", vec![buffer.buf.into()]).await {
                            Ok(()) => true,
                            Err(e) => {
                                self.report_error("Could not save the file", &e);
                                false
                            }
                        }
                    };
                    if !saved {
                        // Cancelled or failed: keep the window open.
                        return;
                    }
                }
                nvim.notify("nvim_command", vec!["qa".into()]);
            }
        }
    }

    /// Subscribe to close confirmations.
    pub fn connect_confirm_close<F: Fn(&Self, &str) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            SIGNAL_CONFIRM_CLOSE,
            false,
            glib::closure_local!(move |vm: &Self, names: &str| f(vm, names)),
        )
    }
}

/// Paths of dropped files that look like text documents (F7.R4).
pub fn droppable(paths: &[PathBuf]) -> Vec<PathBuf> {
    paths.iter().filter(|p| p.is_file()).cloned().collect()
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;
    use crate::model::Document;
    use crate::model::nvim::fake::{Call, FakeNvim};
    use crate::services::fake::FakeFileDialogService;

    fn setup() -> (AppViewModel, Rc<FakeNvim>, Rc<FakeFileDialogService>) {
        let vm = AppViewModel::new(Document::new());
        let nvim = Rc::new(FakeNvim::new());
        let dialogs = Rc::new(FakeFileDialogService::default());
        vm.set_file_services(nvim.clone(), dialogs.clone());
        (vm, nvim, dialogs)
    }

    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        glib::MainContext::new().block_on(future)
    }

    fn unsaved(items: &[(i64, &str)]) -> Value {
        Value::Array(
            items
                .iter()
                .map(|(buf, name)| {
                    Value::Map(vec![
                        ("buf".into(), (*buf).into()),
                        ("name".into(), (*name).into()),
                    ])
                })
                .collect(),
        )
    }

    fn commands(nvim: &FakeNvim) -> Vec<String> {
        nvim.take_calls()
            .into_iter()
            .filter_map(|call| match call {
                Call::Notify(m, args) if m == "nvim_command" => args[0].as_str().map(str::to_owned),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn lua_calls_pass_arguments() {
        assert_eq!(
            lua_call("save", vec![3.into(), "/a b.md".into()]),
            vec![
                Value::from("return ringmd.save(select(1, ...), select(2, ...))"),
                Value::Array(vec![3.into(), "/a b.md".into()]),
            ]
        );
        assert_eq!(
            lua_call("unsaved", vec![])[0],
            Value::from("return ringmd.unsaved()")
        );
    }

    #[test]
    fn closing_without_changes_quits_neovim() {
        let (vm, nvim, _) = setup();
        nvim.set_response(unsaved(&[]));
        assert!(!vm.ready_to_close());
        block_on(vm.request_close());
        assert_eq!(commands(&nvim), ["qa"]);
        vm.neovim_exited(Some(0));
        assert!(vm.ready_to_close());
    }

    /// F1-AC2: unsaved changes ask first.
    #[test]
    fn closing_with_changes_asks() {
        let (vm, nvim, _) = setup();
        nvim.set_response(unsaved(&[(1, "/notes/a.md"), (2, "")]));
        let asked = Rc::new(RefCell::new(None));
        let sink = asked.clone();
        vm.connect_confirm_close(move |_, names| *sink.borrow_mut() = Some(names.to_owned()));
        block_on(vm.request_close());
        assert_eq!(asked.borrow().as_deref(), Some("a.md, [No Name]"));
        assert!(commands(&nvim).is_empty());

        block_on(vm.resolve_close(CloseChoice::Cancel));
        assert!(commands(&nvim).is_empty());
        block_on(vm.resolve_close(CloseChoice::Discard));
        assert_eq!(commands(&nvim), ["qa!"]);
    }

    #[test]
    fn save_choice_saves_then_quits() {
        let (vm, nvim, dialogs) = setup();
        nvim.set_response(unsaved(&[(1, "/notes/a.md"), (2, "")]));
        dialogs
            .save_answers
            .borrow_mut()
            .push_back("/notes/new.md".into());
        block_on(vm.resolve_close(CloseChoice::Save));
        let calls = nvim.take_calls();
        let lua: Vec<Value> = calls
            .iter()
            .filter_map(|c| match c {
                Call::Request(m, args) if m == "nvim_exec_lua" => Some(args[1].clone()),
                _ => None,
            })
            .collect();
        // unsaved(), save(1), save(2, path)
        assert_eq!(lua.len(), 3, "{calls:?}");
        assert_eq!(lua[1], Value::Array(vec![1.into()]));
        assert_eq!(lua[2], Value::Array(vec![2.into(), "/notes/new.md".into()]));
        assert!(
            matches!(calls.last(), Some(Call::Notify(m, a)) if m == "nvim_command" && a[0] == Value::from("qa"))
        );
    }

    #[test]
    fn cancelled_save_as_keeps_the_window() {
        let (vm, nvim, _) = setup();
        nvim.set_response(unsaved(&[(2, "")]));
        block_on(vm.resolve_close(CloseChoice::Save));
        assert!(commands(&nvim).is_empty());
    }

    #[test]
    fn open_with_chooser_edits_the_file() {
        let (vm, nvim, dialogs) = setup();
        dialogs
            .open_answers
            .borrow_mut()
            .push_back("/notes/b.md".into());
        block_on(vm.open_with_chooser());
        let calls = nvim.take_calls();
        assert!(
            matches!(&calls[..], [Call::Request(m, a)] if m == "nvim_exec_lua" && a[1] == Value::Array(vec!["/notes/b.md".into()])),
            "{calls:?}"
        );
    }

    #[test]
    fn helper_errors_are_reported() {
        let (vm, nvim, _) = setup();
        nvim.set_response("E212: Can't open file for writing".into());
        let errors = Rc::new(RefCell::new(Vec::new()));
        let sink = errors.clone();
        vm.connect_error_raised(move |_, title, detail| {
            sink.borrow_mut()
                .push((title.to_owned(), detail.to_owned()));
        });
        block_on(vm.open_file(Path::new("/x.md")));
        assert_eq!(
            *errors.borrow(),
            [(
                "Could not open the file".to_owned(),
                "E212: Can't open file for writing".to_owned()
            )]
        );
    }
}
