//! Main application window.

use gtk::glib;
use gtk::prelude::*;

use super::dialogs;
use crate::viewmodel::AppViewModel;

const DEFAULT_WIDTH: i32 = 1100;
const DEFAULT_HEIGHT: i32 = 720;

/// The RingMD window: editor pane and preview pane in a resizable split (F1.R3).
///
/// The panes are placeholders until the Neovim grid (M1) and the WebKit
/// preview (M3) land.
#[derive(Debug)]
pub struct MainWindow {
    window: gtk::ApplicationWindow,
    vm: AppViewModel,
}

impl MainWindow {
    pub fn new(app: &gtk::Application, vm: AppViewModel) -> Self {
        let editor = placeholder("Editor", "Neovim engine arrives in M1");
        let preview = placeholder("Preview", "Live Markdown preview arrives in M3");

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
        vm.bind_property("fullscreen", &window, "fullscreened")
            .sync_create()
            .bidirectional()
            .build();

        vm.connect_error_raised(glib::clone!(
            #[weak]
            window,
            move |_, title, detail| dialogs::show_error(Some(&window), title, detail)
        ));

        Self { window, vm }
    }

    pub fn window(&self) -> &gtk::ApplicationWindow {
        &self.window
    }

    pub fn view_model(&self) -> &AppViewModel {
        &self.vm
    }

    pub fn present(&self) {
        self.window.present();
    }
}

fn placeholder(title: &str, subtitle: &str) -> gtk::Widget {
    let heading = gtk::Label::builder().label(title).build();
    heading.add_css_class("title-1");
    let caption = gtk::Label::builder().label(subtitle).build();
    caption.add_css_class("dim-label");

    let column = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .valign(gtk::Align::Center)
        .hexpand(true)
        .vexpand(true)
        .build();
    column.append(&heading);
    column.append(&caption);
    column.upcast()
}
