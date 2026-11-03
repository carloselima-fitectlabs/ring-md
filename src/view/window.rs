//! Main application window.

use gtk::glib;
use gtk::prelude::*;

use super::dialogs;
use super::grid_view::GridView;
use super::preview_view::PreviewView;
use crate::viewmodel::session::Targets;
use crate::viewmodel::{AppViewModel, EditorViewModel, ExportViewModel, ThemeViewModel};

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
    pub fn new(app: &gtk::Application, targets: &Targets, editor_vm: &EditorViewModel) -> Self {
        let vm = targets.app.clone();
        let preview_vm = &targets.preview;
        let theme_vm = &targets.theme;
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

        let overlay = gtk::Overlay::builder().child(&split).build();
        let toast = Toast::new();
        overlay.add_overlay(toast.widget());
        show_export_results(&toast, &targets.export);

        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .default_width(DEFAULT_WIDTH)
            .default_height(DEFAULT_HEIGHT)
            .child(&overlay)
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

        apply_theme(&window, theme_vm);

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

/// Window chrome colours from the theme (F5.R2), live on every change.
fn apply_theme(window: &gtk::ApplicationWindow, theme: &ThemeViewModel) {
    let provider = gtk::CssProvider::new();
    gtk::style_context_add_provider_for_display(
        &WidgetExt::display(window),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    let update = move |theme: &ThemeViewModel| {
        provider.load_from_string(&theme.gtk_css());
        if let Some(settings) = gtk::Settings::default() {
            // Dialogs and other stock widgets follow the variant's brightness.
            settings.set_gtk_application_prefer_dark_theme(theme.is_dark());
        }
    };
    update(theme);
    theme.connect_gtk_css_notify(update);
}

/// A short message at the bottom of the window that hides itself (F6.R6).
#[derive(Debug, Clone)]
struct Toast {
    revealer: gtk::Revealer,
    label: gtk::Label,
    hide_after: std::rc::Rc<std::cell::RefCell<Option<glib::SourceId>>>,
}

impl Toast {
    const VISIBLE_FOR: std::time::Duration = std::time::Duration::from_secs(4);

    fn new() -> Self {
        let label = gtk::Label::builder().wrap(true).max_width_chars(80).build();
        label.add_css_class("ringmd-toast");
        let revealer = gtk::Revealer::builder()
            .child(&label)
            .transition_type(gtk::RevealerTransitionType::SlideUp)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::End)
            .margin_bottom(24)
            .can_target(false)
            .build();
        Self {
            revealer,
            label,
            hide_after: Default::default(),
        }
    }

    fn widget(&self) -> &gtk::Revealer {
        &self.revealer
    }

    fn show(&self, message: &str, error: bool) {
        self.label.set_text(message);
        if error {
            self.label.add_css_class("error");
        } else {
            self.label.remove_css_class("error");
        }
        self.revealer.set_reveal_child(true);
        if let Some(previous) = self.hide_after.borrow_mut().take() {
            previous.remove();
        }
        let revealer = self.revealer.downgrade();
        let slot = self.hide_after.clone();
        let id = glib::timeout_add_local_once(Self::VISIBLE_FOR, move || {
            slot.borrow_mut().take();
            if let Some(revealer) = revealer.upgrade() {
                revealer.set_reveal_child(false);
            }
        });
        self.hide_after.borrow_mut().replace(id);
    }
}

fn show_export_results(toast: &Toast, export: &ExportViewModel) {
    let toast_clone = toast.clone();
    export.connect_is_exporting_notify(move |export| {
        if export.is_exporting() {
            toast_clone.show("Exporting PDF…", false);
        }
    });
    let toast = toast.clone();
    export.connect_finished(move |_, ok, message| toast.show(message, !ok));
}
