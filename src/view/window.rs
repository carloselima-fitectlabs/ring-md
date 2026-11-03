//! Main application window.

use gtk::prelude::*;
use gtk::{gdk, gio, glib};

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

        let headerbar = build_headerbar(&vm);
        window.set_titlebar(Some(&headerbar));
        // The headerbar is hidden in fullscreen (F1.R7).
        vm.bind_property("fullscreen", &headerbar, "visible")
            .sync_create()
            .invert_boolean()
            .build();
        install_actions(&window, targets);
        accept_dropped_files(&window, &vm);
        handle_close(&window, &vm);
        window.connect_is_active_notify(glib::clone!(
            #[weak]
            editor_vm,
            move |window| editor_vm.focus_changed(window.is_active())
        ));

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

/// Title, preview toggle and menu (F1.R7). No keyboard accelerators: the
/// GUI shortcuts are Neovim mappings, so they respect the mode (D8).
fn build_headerbar(vm: &AppViewModel) -> gtk::HeaderBar {
    let headerbar = gtk::HeaderBar::new();

    let preview = gtk::ToggleButton::builder()
        .icon_name("view-dual-symbolic")
        .tooltip_text("Preview (Ctrl+P)")
        .focus_on_click(false)
        .build();
    vm.bind_property("preview-visible", &preview, "active")
        .sync_create()
        .bidirectional()
        .build();
    headerbar.pack_start(&preview);

    let themes = gio::Menu::new();
    for name in crate::model::theme::ThemeChoice::names() {
        let label = theme_label(&name);
        themes.append(Some(&label), Some(&format!("win.theme::{name}")));
    }
    let menu = gio::Menu::new();
    menu.append(Some("Open…"), Some("win.open"));
    menu.append(Some("Export PDF"), Some("win.export-pdf"));
    menu.append_submenu(Some("Theme"), &themes);
    menu.append(Some("About RingMD"), Some("win.about"));
    let menu_button = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .menu_model(&menu)
        .tooltip_text("Menu")
        .focus_on_click(false)
        .build();
    headerbar.pack_end(&menu_button);
    headerbar
}

/// `dark-medium` → `Dark medium`, `system-soft` → `System (soft)`.
fn theme_label(name: &str) -> String {
    match name {
        "system" => "Follow system".to_owned(),
        _ => match name.strip_prefix("system-") {
            Some(contrast) => format!("Follow system ({contrast})"),
            None => {
                let mut label = name.replacen('-', " ", 1);
                if let Some(first) = label.get_mut(0..1) {
                    first.make_ascii_uppercase();
                }
                label
            }
        },
    }
}

/// Window actions behind the menu; each calls a ViewModel command.
fn install_actions(window: &gtk::ApplicationWindow, targets: &Targets) {
    let open = gio::SimpleAction::new("open", None);
    open.connect_activate(glib::clone!(
        #[weak(rename_to = app)]
        targets.app,
        move |_, _| {
            glib::spawn_future_local(async move { app.open_with_chooser().await });
        }
    ));

    let export_pdf = gio::SimpleAction::new("export-pdf", None);
    export_pdf.connect_activate(glib::clone!(
        #[weak(rename_to = app)]
        targets.app,
        #[weak(rename_to = export)]
        targets.export,
        move |_, _| {
            let document = app.document().path().map(Into::into);
            glib::spawn_future_local(async move { export.export(None, document).await });
        }
    ));

    let theme_vm = &targets.theme;
    let theme = gio::SimpleAction::new_stateful(
        "theme",
        Some(glib::VariantTy::STRING),
        &theme_vm.choice().name().to_variant(),
    );
    theme.connect_activate(glib::clone!(
        #[weak]
        theme_vm,
        move |_, target| {
            if let Some(choice) = target
                .and_then(|v| v.get::<String>())
                .and_then(|name| name.parse().ok())
            {
                theme_vm.set_choice(choice);
            }
        }
    ));
    theme_vm.connect_variant_notify(glib::clone!(
        #[weak]
        theme,
        move |vm| theme.set_state(&vm.choice().name().to_variant())
    ));

    let about = gio::SimpleAction::new("about", None);
    about.connect_activate(glib::clone!(
        #[weak]
        window,
        move |_, _| dialogs::show_about(Some(&window))
    ));

    for action in [open, export_pdf, theme, about] {
        window.add_action(&action);
    }
}

/// Dropping files on the window opens them (F7.R4).
fn accept_dropped_files(window: &gtk::ApplicationWindow, vm: &AppViewModel) {
    let target = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
    target.connect_drop(glib::clone!(
        #[weak]
        vm,
        #[upgrade_or]
        false,
        move |_, value, _, _| {
            let Ok(list) = value.get::<gdk::FileList>() else {
                return false;
            };
            let paths: Vec<_> = list.files().iter().filter_map(|f| f.path()).collect();
            let paths = crate::viewmodel::files::droppable(&paths);
            if paths.is_empty() {
                return false;
            }
            glib::spawn_future_local(async move {
                for path in paths {
                    vm.open_file(&path).await;
                }
            });
            true
        }
    ));
    window.add_controller(target);
}

/// Closing goes through Neovim (F1.R5): the window closes only once
/// Neovim has quit, after asking about unsaved changes.
fn handle_close(window: &gtk::ApplicationWindow, vm: &AppViewModel) {
    window.connect_close_request(glib::clone!(
        #[weak]
        vm,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_| {
            if vm.ready_to_close() {
                return glib::Propagation::Proceed;
            }
            glib::spawn_future_local(async move { vm.request_close().await });
            glib::Propagation::Stop
        }
    ));
    vm.connect_confirm_close(glib::clone!(
        #[weak]
        window,
        move |vm, unsaved| {
            let (vm, unsaved) = (vm.clone(), unsaved.to_owned());
            glib::spawn_future_local(async move {
                let choice = dialogs::confirm_close(Some(&window), &unsaved).await;
                vm.resolve_close(choice).await;
            });
        }
    ));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_labels() {
        assert_eq!(theme_label("dark-medium"), "Dark medium");
        assert_eq!(theme_label("light-soft"), "Light soft");
        assert_eq!(theme_label("system"), "Follow system");
        assert_eq!(theme_label("system-hard"), "Follow system (hard)");
    }
}
