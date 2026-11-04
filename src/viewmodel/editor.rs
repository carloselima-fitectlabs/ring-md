//! Editor state: the Neovim grid, font and input commands (T1.3–T1.10).
//!
//! The View draws [`EditorViewModel::grid`] after every `flushed` signal and
//! forwards keyboard, mouse and size changes as commands. Every decision
//! (key encoding, shortcuts, pixel → cell mapping) is made here.

use std::cell::{Ref, RefCell};
use std::fmt;
use std::rc::Rc;

use gtk::glib;
use gtk::glib::prelude::*;
use gtk::glib::subclass::prelude::*;
use rmpv::Value;

use crate::model::clipboard::{self, Register};
use crate::model::grid::{Damage, Grid};
use crate::model::nvim::input::{
    self, KeyPress, Modifiers, MouseAction, MouseButton, MouseEvent, Shortcut,
};
use crate::model::nvim::{NvimApi, redraw};
use crate::services::ClipboardService;

/// Name of the signal emitted when Neovim finished a batch of redraws.
pub const SIGNAL_FLUSHED: &str = "flushed";

/// Default font (F3.R5).
pub const DEFAULT_FONT_FAMILY: &str = "Monospace";
pub const DEFAULT_FONT_SIZE: u32 = 12;
const MIN_FONT_SIZE: u32 = 6;
const MAX_FONT_SIZE: u32 = 72;

/// Grid size used until the View reports its real size.
const INITIAL_COLS: u32 = 80;
const INITIAL_ROWS: u32 = 24;

/// What the editor talks to.
pub struct EditorServices {
    pub nvim: Rc<dyn NvimApi>,
    pub clipboard: Rc<dyn ClipboardService>,
}

impl fmt::Debug for EditorServices {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EditorServices").finish_non_exhaustive()
    }
}

mod imp {
    use std::cell::{Cell, OnceCell};
    use std::sync::OnceLock;

    use gtk::glib::subclass::Signal;

    use super::*;

    #[derive(Debug, glib::Properties)]
    #[properties(wrapper_type = super::EditorViewModel)]
    pub struct EditorViewModel {
        /// Pango font description, e.g. `"Monospace 12"` (F3.R5).
        #[property(name = "font", get = Self::font, type = String)]
        pub(super) font_family: RefCell<String>,
        #[property(get, set = Self::set_font_size, explicit_notify, default = DEFAULT_FONT_SIZE)]
        pub(super) font_size: Cell<u32>,

        /// Grid size in cells.
        #[property(get)]
        pub(super) cols: Cell<u32>,
        #[property(get)]
        pub(super) rows: Cell<u32>,

        /// Current Neovim mode name (`normal`, `insert`, …).
        #[property(get)]
        pub(super) mode: RefCell<String>,

        pub(super) grid: RefCell<Grid>,
        pub(super) services: OnceCell<EditorServices>,
        pub(super) attached: Cell<bool>,
        /// Cell size in logical pixels, measured by the View.
        pub(super) cell_size: Cell<(f64, f64)>,
        /// Widget size in logical pixels.
        pub(super) area: Cell<(f64, f64)>,
        /// Button held during a drag, and the last cell reported.
        pub(super) drag: Cell<Option<(MouseButton, u32, u32)>>,
        /// Size `Ctrl+0` returns to (from the config).
        pub(super) default_font_size: Cell<u32>,
    }

    impl Default for EditorViewModel {
        fn default() -> Self {
            Self {
                font_family: RefCell::new(DEFAULT_FONT_FAMILY.into()),
                font_size: Cell::new(DEFAULT_FONT_SIZE),
                default_font_size: Cell::new(DEFAULT_FONT_SIZE),
                cols: Cell::new(INITIAL_COLS),
                rows: Cell::new(INITIAL_ROWS),
                mode: RefCell::new("normal".into()),
                grid: RefCell::default(),
                services: OnceCell::new(),
                attached: Cell::new(false),
                cell_size: Cell::new((0.0, 0.0)),
                area: Cell::new((0.0, 0.0)),
                drag: Cell::new(None),
            }
        }
    }

    impl EditorViewModel {
        fn font(&self) -> String {
            format!("{} {}", self.font_family.borrow(), self.font_size.get())
        }

        fn set_font_size(&self, size: u32) {
            let size = size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
            if self.font_size.replace(size) != size {
                self.obj().notify_font_size();
                self.obj().notify_font();
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for EditorViewModel {
        const NAME: &'static str = "RingmdEditorViewModel";
        type Type = super::EditorViewModel;
    }

    #[glib::derived_properties]
    impl ObjectImpl for EditorViewModel {
        fn signals() -> &'static [Signal] {
            static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| vec![Signal::builder(SIGNAL_FLUSHED).build()])
        }
    }
}

glib::wrapper! {
    /// ViewModel for the editor pane. See `docs/SPEC.md` §3.1.
    pub struct EditorViewModel(ObjectSubclass<imp::EditorViewModel>);
}

impl EditorViewModel {
    pub fn new(services: EditorServices) -> Self {
        let vm: Self = glib::Object::new();
        let _ = vm.imp().services.set(services);
        vm
    }

    fn nvim(&self) -> Option<&Rc<dyn NvimApi>> {
        self.imp().services.get().map(|s| &s.nvim)
    }

    /// Attach to Neovim as a UI with the current grid size (F2.R1). Called
    /// automatically on the first resize, so Neovim starts at the real size.
    pub fn attach(&self) {
        let Some(nvim) = self.nvim() else {
            return;
        };
        if self.imp().attached.replace(true) {
            return;
        }
        // Written immediately, so later notifications (resize, input) reach
        // Neovim after it. Errors arrive as `nvim_error_event`.
        drop(nvim.ui_attach(self.cols(), self.rows()));
    }

    /// The grid to draw. Do not hold the borrow across commands.
    pub fn grid(&self) -> Ref<'_, Grid> {
        self.imp().grid.borrow()
    }

    /// Rows changed since the last call.
    pub fn take_damage(&self) -> Damage {
        self.imp().grid.borrow_mut().take_damage()
    }

    /// Handle a `redraw` notification.
    pub fn handle_redraw(&self, params: &[Value]) {
        let mut flushed = false;
        let mode = {
            let mut grid = self.imp().grid.borrow_mut();
            for event in redraw::decode(params) {
                flushed |= grid.apply(&event);
            }
            grid.mode_name().to_owned()
        };
        if *self.imp().mode.borrow() != mode {
            self.imp().mode.replace(mode);
            self.notify_mode();
        }
        if flushed {
            self.emit_by_name::<()>(SIGNAL_FLUSHED, &[]);
        }
    }

    /// Subscribe to completed redraw batches.
    pub fn connect_flushed<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_closure(
            SIGNAL_FLUSHED,
            false,
            glib::closure_local!(move |vm: &Self| f(vm)),
        )
    }

    /// Command: a key was pressed (F2.R4). Returns `true` if it was used.
    pub fn key_pressed(&self, key: &KeyPress) -> bool {
        if let Some(shortcut) = input::shortcut(key) {
            match shortcut {
                Shortcut::ZoomIn => self.zoom_in(),
                Shortcut::ZoomOut => self.zoom_out(),
                Shortcut::ZoomReset => self.zoom_reset(),
            }
            return true;
        }
        let Some(keys) = input::encode_key(key) else {
            return false;
        };
        if let Some(nvim) = self.nvim() {
            nvim.input(&keys);
        }
        true
    }

    /// Command: an input method committed text (IME, dead keys).
    pub fn commit_text(&self, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Some(nvim) = self.nvim() {
            nvim.input(&input::escape_text(text));
        }
    }

    /// The cell under a point, clamped to the grid.
    fn cell_at(&self, x: f64, y: f64) -> (u32, u32) {
        let (cell_w, cell_h) = self.imp().cell_size.get();
        if cell_w <= 0.0 || cell_h <= 0.0 {
            return (0, 0);
        }
        let to_index = |pos: f64, size: f64, count: u32| {
            // Saturating float → int conversion; negative positions map to 0.
            ((pos / size).floor().max(0.0) as u32).min(count.saturating_sub(1))
        };
        (
            to_index(y, cell_h, self.rows()),
            to_index(x, cell_w, self.cols()),
        )
    }

    /// Command: mouse button pressed at `(x, y)` (F2.R5).
    pub fn mouse_pressed(&self, button: MouseButton, modifiers: Modifiers, x: f64, y: f64) {
        let (row, col) = self.cell_at(x, y);
        self.imp().drag.set(Some((button, row, col)));
        self.send_mouse(button, MouseAction::Press, modifiers, row, col);
    }

    /// Command: pointer moved with a button held. Only cell changes are sent.
    pub fn mouse_dragged(&self, modifiers: Modifiers, x: f64, y: f64) {
        let Some((button, last_row, last_col)) = self.imp().drag.get() else {
            return;
        };
        let (row, col) = self.cell_at(x, y);
        if (row, col) == (last_row, last_col) {
            return;
        }
        self.imp().drag.set(Some((button, row, col)));
        self.send_mouse(button, MouseAction::Drag, modifiers, row, col);
    }

    /// Command: mouse button released.
    pub fn mouse_released(&self, modifiers: Modifiers, x: f64, y: f64) {
        let Some((button, _, _)) = self.imp().drag.take() else {
            return;
        };
        let (row, col) = self.cell_at(x, y);
        self.send_mouse(button, MouseAction::Release, modifiers, row, col);
    }

    /// Command: scroll wheel. `dy > 0` scrolls down.
    pub fn scrolled(&self, modifiers: Modifiers, x: f64, y: f64, dx: f64, dy: f64) {
        let (row, col) = self.cell_at(x, y);
        let action = if dy > 0.0 {
            MouseAction::WheelDown
        } else if dy < 0.0 {
            MouseAction::WheelUp
        } else if dx > 0.0 {
            MouseAction::WheelRight
        } else if dx < 0.0 {
            MouseAction::WheelLeft
        } else {
            return;
        };
        self.send_mouse(MouseButton::Wheel, action, modifiers, row, col);
    }

    fn send_mouse(
        &self,
        button: MouseButton,
        action: MouseAction,
        modifiers: Modifiers,
        row: u32,
        col: u32,
    ) {
        if let Some(nvim) = self.nvim() {
            nvim.input_mouse(&MouseEvent {
                button,
                action,
                modifiers,
                row,
                col,
            });
        }
    }

    /// Command: the View measured the font's cell size (logical pixels).
    pub fn set_cell_size(&self, width: f64, height: f64) {
        self.imp().cell_size.set((width, height));
        self.update_grid_size();
    }

    /// Command: the editor area changed size (F2.R6).
    pub fn resize(&self, width: f64, height: f64) {
        self.imp().area.set((width, height));
        self.update_grid_size();
    }

    fn update_grid_size(&self) {
        let (cell_w, cell_h) = self.imp().cell_size.get();
        let (width, height) = self.imp().area.get();
        if cell_w <= 0.0 || cell_h <= 0.0 || width <= 0.0 || height <= 0.0 {
            return;
        }
        let cols = ((width / cell_w).floor() as u32).max(1);
        let rows = ((height / cell_h).floor() as u32).max(1);
        let changed = (cols, rows) != (self.cols(), self.rows());
        if changed {
            self.imp().cols.set(cols);
            self.imp().rows.set(rows);
            self.notify_cols();
            self.notify_rows();
        }
        if !self.imp().attached.get() {
            self.attach();
        } else if changed && let Some(nvim) = self.nvim() {
            nvim.ui_try_resize(cols, rows);
        }
    }

    /// Command: `Ctrl+=` (F3.R5).
    pub fn zoom_in(&self) {
        self.set_font_size(self.font_size() + 1);
    }

    /// Command: `Ctrl+-`.
    pub fn zoom_out(&self) {
        self.set_font_size(self.font_size().saturating_sub(1));
    }

    /// Command: `Ctrl+0`: back to the configured size.
    pub fn zoom_reset(&self) {
        self.set_font_size(self.imp().default_font_size.get());
    }

    /// Apply the configured font (F9.R2): family and default size.
    pub fn configure_font(&self, family: &str, size: u32) {
        let imp = self.imp();
        let size = size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
        imp.default_font_size.set(size);
        let family_changed = *imp.font_family.borrow() != family;
        if family_changed {
            imp.font_family.replace(family.to_owned());
        }
        if imp.font_size.replace(size) != size {
            self.notify_font_size();
            self.notify_font();
        } else if family_changed {
            self.notify_font();
        }
    }

    /// Command: the window gained or lost focus. Neovim then fires
    /// `FocusGained`/`FocusLost`, which re-checks files changed on disk
    /// (F7.R3).
    pub fn focus_changed(&self, focused: bool) {
        if self.imp().attached.get()
            && let Some(nvim) = self.nvim()
        {
            nvim.notify("nvim_ui_set_focus", vec![focused.into()]);
        }
    }

    /// Neovim copied to `"+` or `"*` (T1.9).
    pub fn clipboard_set(&self, params: &[Value]) {
        let lines = params
            .first()
            .and_then(Value::as_array)
            .map(|lines| {
                lines
                    .iter()
                    .map(|l| l.as_str().unwrap_or_default().to_owned())
                    .collect()
            })
            .unwrap_or_default();
        let regtype = params
            .get(1)
            .and_then(Value::as_str)
            .unwrap_or("v")
            .to_owned();
        let text = clipboard::to_clipboard_text(&Register { lines, regtype });
        if let Some(services) = self.imp().services.get() {
            services.clipboard.set_text(&text);
        }
    }

    /// Neovim pastes from `"+` or `"*` and waits for request `id`.
    pub async fn clipboard_get(&self, id: u64) {
        let Some(services) = self.imp().services.get() else {
            return;
        };
        let text = match services.clipboard.read_text().await {
            Ok(text) => text.unwrap_or_default(),
            Err(e) => {
                tracing::warn!(error = %e, "could not read the clipboard");
                String::new()
            }
        };
        let register = clipboard::from_clipboard_text(&text);
        let lines = register.lines.into_iter().map(Value::from).collect();
        services.nvim.respond(
            id,
            Ok(Value::Array(vec![
                Value::Array(lines),
                register.regtype.into(),
            ])),
        );
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::model::nvim::fake::{Call, FakeNvim};
    use crate::services::fake::FakeClipboardService;

    fn editor() -> (EditorViewModel, Rc<FakeNvim>, Rc<FakeClipboardService>) {
        let nvim = Rc::new(FakeNvim::new());
        let clipboard = Rc::new(FakeClipboardService::default());
        let vm = EditorViewModel::new(EditorServices {
            nvim: nvim.clone(),
            clipboard: clipboard.clone(),
        });
        (vm, nvim, clipboard)
    }

    fn batch(name: &str, args: Vec<Value>) -> Value {
        let mut items = vec![Value::from(name)];
        items.extend(args);
        Value::Array(items)
    }

    #[test]
    fn defaults() {
        let (vm, _, _) = editor();
        assert_eq!(vm.font(), "Monospace 12");
        assert_eq!((vm.cols(), vm.rows()), (80, 24));
        assert_eq!(vm.mode(), "normal");
    }

    #[test]
    fn keys_are_encoded_and_sent() {
        let (vm, nvim, _) = editor();
        assert!(vm.key_pressed(&KeyPress::new("Escape", None, Modifiers::NONE)));
        assert!(vm.key_pressed(&KeyPress::new("w", Some('w'), Modifiers::CTRL)));
        assert!(!vm.key_pressed(&KeyPress::new("Shift_L", None, Modifiers::NONE)));
        vm.commit_text("ç<");
        vm.commit_text("");
        assert_eq!(nvim.take_input(), ["<Esc>", "<C-w>", "ç<lt>"]);
    }

    #[test]
    fn zoom_shortcuts_change_font_without_reaching_nvim() {
        let (vm, nvim, _) = editor();
        let notified = Rc::new(Cell::new(0));
        let counter = notified.clone();
        vm.connect_font_notify(move |_| counter.set(counter.get() + 1));

        assert!(vm.key_pressed(&KeyPress::new("equal", Some('='), Modifiers::CTRL)));
        assert_eq!(vm.font(), "Monospace 13");
        vm.key_pressed(&KeyPress::new("minus", Some('-'), Modifiers::CTRL));
        vm.key_pressed(&KeyPress::new("minus", Some('-'), Modifiers::CTRL));
        assert_eq!(vm.font_size(), 11);
        vm.key_pressed(&KeyPress::new("0", Some('0'), Modifiers::CTRL));
        assert_eq!(vm.font_size(), DEFAULT_FONT_SIZE);
        assert_eq!(notified.get(), 4);
        assert!(nvim.take_calls().is_empty());
    }

    #[test]
    fn configured_font_is_the_reset_target() {
        let (vm, _, _) = editor();
        vm.configure_font("Iosevka", 15);
        assert_eq!(vm.font(), "Iosevka 15");
        vm.zoom_in();
        vm.zoom_reset();
        assert_eq!(vm.font_size(), 15);
    }

    #[test]
    fn font_size_is_clamped() {
        let (vm, _, _) = editor();
        vm.set_font_size(1);
        assert_eq!(vm.font_size(), MIN_FONT_SIZE);
        for _ in 0..100 {
            vm.zoom_in();
        }
        assert_eq!(vm.font_size(), MAX_FONT_SIZE);
    }

    #[test]
    fn first_resize_attaches_then_resizes() {
        let (vm, nvim, _) = editor();
        vm.set_cell_size(10.0, 20.0);
        assert!(nvim.take_calls().is_empty(), "no size yet");
        vm.resize(805.0, 405.0);
        assert_eq!((vm.cols(), vm.rows()), (80, 20));
        assert_eq!(
            nvim.take_calls(),
            [Call::Request(
                "nvim_ui_attach".into(),
                vec![
                    80.into(),
                    20.into(),
                    Value::Map(vec![
                        ("rgb".into(), true.into()),
                        ("ext_linegrid".into(), true.into()),
                    ]),
                ]
            )]
        );

        vm.resize(1000.0, 400.0);
        assert_eq!(
            nvim.take_calls(),
            [Call::Notify(
                "nvim_ui_try_resize".into(),
                vec![100.into(), 20.into()]
            )]
        );
        // Same cell count: no extra resize.
        vm.resize(1005.0, 410.0);
        assert!(nvim.take_calls().is_empty());
    }

    #[test]
    fn mouse_maps_pixels_to_cells() {
        let (vm, nvim, _) = editor();
        vm.set_cell_size(10.0, 20.0);
        vm.resize(800.0, 400.0);
        nvim.take_calls(); // ui_attach
        vm.mouse_pressed(MouseButton::Left, Modifiers::NONE, 35.0, 45.0);
        vm.mouse_dragged(Modifiers::NONE, 38.0, 50.0); // same cell: dropped
        vm.mouse_dragged(Modifiers::NONE, 52.0, 50.0);
        vm.mouse_released(Modifiers::NONE, 52.0, 50.0);
        vm.mouse_dragged(Modifiers::NONE, 90.0, 90.0); // no button: dropped
        vm.scrolled(Modifiers::CTRL, 9999.0, -5.0, 0.0, 1.0);

        let mouse = |button: &str, action: &str, mods: &str, row: u32, col: u32| {
            Call::Notify(
                "nvim_input_mouse".into(),
                vec![
                    button.into(),
                    action.into(),
                    mods.into(),
                    0.into(),
                    row.into(),
                    col.into(),
                ],
            )
        };
        assert_eq!(
            nvim.take_calls(),
            [
                mouse("left", "press", "", 2, 3),
                mouse("left", "drag", "", 2, 5),
                mouse("left", "release", "", 2, 5),
                mouse("wheel", "down", "C", 0, 79),
            ]
        );
    }

    #[test]
    fn redraw_updates_grid_mode_and_signals_flush() {
        let (vm, _, _) = editor();
        let flushes = Rc::new(Cell::new(0));
        let counter = flushes.clone();
        vm.connect_flushed(move |_| counter.set(counter.get() + 1));

        vm.handle_redraw(&[
            batch(
                "grid_resize",
                vec![Value::Array(vec![1.into(), 5.into(), 2.into()])],
            ),
            batch(
                "grid_line",
                vec![Value::Array(vec![
                    1.into(),
                    0.into(),
                    0.into(),
                    Value::Array(vec![
                        Value::Array(vec!["h".into(), 0.into()]),
                        Value::Array(vec!["i".into()]),
                    ]),
                ])],
            ),
        ]);
        assert_eq!(flushes.get(), 0, "no flush yet");

        vm.handle_redraw(&[
            batch(
                "mode_change",
                vec![Value::Array(vec!["insert".into(), 1.into()])],
            ),
            batch("flush", vec![Value::Array(vec![])]),
        ]);
        assert_eq!(flushes.get(), 1);
        assert_eq!(vm.grid().row_text(0), "hi   ");
        assert_eq!(vm.mode(), "insert");
        assert_eq!(vm.take_damage(), Damage::Full);
    }

    #[test]
    fn clipboard_copy_and_paste() {
        let (vm, nvim, clipboard) = editor();
        vm.clipboard_set(&[Value::Array(vec!["one".into(), "".into()]), "V".into()]);
        assert_eq!(clipboard.text.borrow().as_deref(), Some("one\n"));

        clipboard.text.replace(Some("a\nb".into()));
        glib::MainContext::new().block_on(vm.clipboard_get(9));
        assert_eq!(
            nvim.take_calls(),
            [Call::Respond(
                9,
                Ok(Value::Array(vec![
                    Value::Array(vec!["a".into(), "b".into()]),
                    "v".into()
                ]))
            )]
        );
    }
}
