//! Main application window.

use gtk::glib;
use gtk::prelude::*;

use super::dialogs;
use super::grid_view::GridView;
use super::preview_view::PreviewView;
use crate::viewmodel::{AppViewModel, EditorViewModel, PreviewViewModel};

const DEFAULT_WIDTH: i32 = 1100;
const DEFAULT_HEIGHT: i32 = 720;

/// The RingMD window: editor pane and preview pane in a resizable split (F1.R3).
#[derive(Debug)]
pub struct MainWindow {
    window: gtk::ApplicationWindow,
    editor: GridView,
    vm: AppViewModel,
}

impl MainWindow {
    pub fn new(
        app: &gtk::Application,
        vm: AppViewModel,
        editor_vm: &EditorViewModel,
        preview_vm: &PreviewViewModel,
    ) -> Self {
        let editor = GridView::new(editor_vm);
        let preview = PreviewView::new(preview_vm).widget().clone();

        let split = gtk::Paned::builder()
            .orientation(gtk::Orientation::Horizontal)
            .start_child(&editor)
            .end_child(&preview)
            .resize_start_child(true)
            .resize_end_child(true)
            .shrink_start_child(false)
            .shrink_end_child(false)
            .position(DEFAULT_WIDTH / 2)
            .build();

        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .default_width(DEFAULT_WIDTH)
            .default_height(DEFAULT_HEIGHT)
            .child(&split)
            .build();

        vm.bind_property("title", &window, "title")
            .sync_create()
            .build();
        vm.bind_property("preview-visible", &preview, "visible")
            .sync_create()
            .build();
        vm.bind_property("preview-visible", preview_vm, "active")
            .sync_create()
            .build();
        vm.bind_property("fullscreen", &window, "fullscreened")
            .sync_create()
            .bidirectional()
            .build();

        vm.connect_error_raised(glib::clone!(
            #[weak]
            window,
            move |_, title, detail| dialogs::show_error(Some(&window), title, detail)
        ));
        vm.connect_close_requested(glib::clone!(
            #[weak]
            window,
            move |_| window.close()
        ));

        Self { window, editor, vm }
    }

    pub fn window(&self) -> &gtk::ApplicationWindow {
        &self.window
    }

    pub fn view_model(&self) -> &AppViewModel {
        &self.vm
    }

    pub fn present(&self) {
        self.window.present();
        self.editor.grab_focus();
    }
}
