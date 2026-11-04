//! Theme state (T4.5, T4.6): the chosen theme, the variant it resolves to,
//! and the CSS generated from its palette. Neovim gets the highlight groups
//! once it has started and again on every change.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::glib;
use gtk::glib::prelude::*;
use gtk::glib::subclass::prelude::*;

use crate::model::config::Setting;
use crate::model::nvim::NvimApi;
use crate::model::theme::{self, ThemeChoice, Variant};
use crate::services::SettingsStore;

mod imp {
    use super::*;

    #[derive(glib::Properties)]
    #[properties(wrapper_type = super::ThemeViewModel)]
    pub struct ThemeViewModel {
        /// Name of the variant on screen, e.g. `dark-medium`.
        #[property(get = Self::variant_name, type = String)]
        pub(super) variant: Cell<Variant>,

        /// Whether the variant on screen is a dark one.
        #[property(get = Self::is_dark, type = bool)]
        _is_dark: (),

        /// CSS for the window chrome (F5.R2).
        #[property(get)]
        pub(super) gtk_css: RefCell<String>,

        /// CSS custom properties for the preview (F5.R2).
        #[property(get)]
        pub(super) preview_css: RefCell<String>,

        /// Page background as `#rrggbb`, for widgets drawn before CSS loads.
        #[property(get)]
        pub(super) background: RefCell<String>,

        pub(super) choice: Cell<ThemeChoice>,
        pub(super) system_dark: Cell<bool>,
        pub(super) nvim_ready: Cell<bool>,
        pub(super) nvim: RefCell<Option<Rc<dyn NvimApi>>>,
        pub(super) settings: RefCell<Option<Rc<dyn SettingsStore>>>,
    }

    impl Default for ThemeViewModel {
        fn default() -> Self {
            Self {
                variant: Cell::new(Variant::default()),
                _is_dark: (),
                gtk_css: RefCell::default(),
                preview_css: RefCell::default(),
                background: RefCell::default(),
                choice: Cell::new(ThemeChoice::default()),
                system_dark: Cell::new(true),
                nvim_ready: Cell::new(false),
                nvim: RefCell::default(),
                settings: RefCell::default(),
            }
        }
    }

    impl std::fmt::Debug for ThemeViewModel {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("ThemeViewModel")
                .field("variant", &self.variant.get())
                .field("choice", &self.choice.get())
                .finish_non_exhaustive()
        }
    }

    impl ThemeViewModel {
        fn variant_name(&self) -> String {
            self.variant.get().name().to_owned()
        }

        fn is_dark(&self) -> bool {
            self.variant.get().is_dark()
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ThemeViewModel {
        const NAME: &'static str = "RingmdThemeViewModel";
        type Type = super::ThemeViewModel;
    }

    #[glib::derived_properties]
    impl ObjectImpl for ThemeViewModel {}
}

glib::wrapper! {
    /// ViewModel for the Gruvbox theme. See `docs/SPEC.md` §3.1.
    pub struct ThemeViewModel(ObjectSubclass<imp::ThemeViewModel>);
}

impl ThemeViewModel {
    /// Start with `choice` (from the config). `settings` persists changes
    /// made with [`Self::set_choice`].
    pub fn new(choice: ThemeChoice, settings: Rc<dyn SettingsStore>) -> Self {
        let vm: Self = glib::Object::new();
        let imp = vm.imp();
        imp.settings.replace(Some(settings));
        imp.choice.set(choice);
        let variant = choice.resolve(imp.system_dark.get());
        imp.variant.set(variant);
        vm.update_css(variant);
        vm
    }

    /// The theme the user chose (may be `system`).
    pub fn choice(&self) -> ThemeChoice {
        self.imp().choice.get()
    }

    /// The variant currently shown.
    pub fn current_variant(&self) -> Variant {
        self.imp().variant.get()
    }

    /// Command: `:Theme <name>` or the menu. Applies live and persists
    /// (F5.R4).
    pub fn set_choice(&self, choice: ThemeChoice) {
        self.imp().choice.set(choice);
        if let Some(settings) = self.imp().settings.borrow().as_ref() {
            settings.save(&Setting::Theme(choice));
        }
        self.refresh();
    }

    /// The desktop's dark/light preference changed (F5.R5).
    pub fn set_system_prefers_dark(&self, dark: bool) {
        if self.imp().system_dark.replace(dark) != dark {
            self.refresh();
        }
    }

    /// The Neovim session to send highlight groups to.
    pub fn attach_neovim(&self, nvim: Rc<dyn NvimApi>) {
        self.imp().nvim.replace(Some(nvim));
    }

    /// Neovim has started (`VimEnter`): give it the highlight groups.
    pub fn neovim_ready(&self) {
        self.imp().nvim_ready.set(true);
        self.apply_to_neovim();
    }

    fn refresh(&self) {
        let imp = self.imp();
        let variant = imp.choice.get().resolve(imp.system_dark.get());
        if imp.variant.replace(variant) == variant {
            return;
        }
        tracing::info!(%variant, "theme changed");
        self.update_css(variant);
        self.notify_variant();
        self.notify_is_dark();
        self.apply_to_neovim();
    }

    fn update_css(&self, variant: Variant) {
        let palette = variant.palette();
        let imp = self.imp();
        imp.gtk_css.replace(palette.gtk_css());
        imp.preview_css
            .replace(palette.preview_css(variant.is_dark()));
        imp.background.replace(palette.bg0.to_string());
        self.notify_gtk_css();
        self.notify_preview_css();
        self.notify_background();
    }

    fn apply_to_neovim(&self) {
        let imp = self.imp();
        if !imp.nvim_ready.get() {
            return;
        }
        if let Some(nvim) = imp.nvim.borrow().as_ref() {
            nvim.notify(
                "nvim_exec_lua",
                theme::apply_highlights_args(imp.variant.get()),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::nvim::fake::{Call, FakeNvim};
    use crate::model::theme::Contrast;
    use crate::services::fake::FakeSettingsStore;

    fn setup(choice: ThemeChoice) -> (ThemeViewModel, Rc<FakeNvim>, Rc<FakeSettingsStore>) {
        let settings = Rc::new(FakeSettingsStore::default());
        let vm = ThemeViewModel::new(choice, settings.clone());
        (vm, Rc::new(FakeNvim::new()), settings)
    }

    fn highlight_calls(nvim: &FakeNvim) -> Vec<String> {
        nvim.take_calls()
            .into_iter()
            .filter_map(|call| match call {
                Call::Notify(method, args) if method == "nvim_exec_lua" => {
                    let background = args[1].as_array()?[1].as_str()?.to_owned();
                    Some(background)
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn starts_with_the_configured_theme() {
        let (vm, _, _) = setup(ThemeChoice::Fixed(Variant::LightHard));
        assert_eq!(vm.variant(), "light-hard");
        assert!(!vm.is_dark());
        assert!(vm.preview_css().contains("--bg: #f9f5d7"));
        assert!(vm.gtk_css().contains("#f9f5d7"));
        assert_eq!(vm.background(), "#f9f5d7");
    }

    /// F5-AC1: a switch recolors everything; F5-AC2: it persists.
    #[test]
    fn set_choice_applies_live_and_persists() {
        let (vm, nvim, settings) = setup(ThemeChoice::default());
        vm.attach_neovim(nvim.clone());
        vm.neovim_ready();
        assert_eq!(highlight_calls(&nvim), ["dark"]);

        let notified = Rc::new(RefCell::new(Vec::new()));
        let sink = notified.clone();
        vm.connect_notify_local(None, move |_, pspec| {
            sink.borrow_mut().push(pspec.name().to_owned());
        });
        let light_soft = ThemeChoice::Fixed(Variant::LightSoft);
        vm.set_choice(light_soft);
        assert_eq!(vm.variant(), "light-soft");
        assert_eq!(highlight_calls(&nvim), ["light"]);
        for property in ["gtk-css", "preview-css", "background", "variant", "is-dark"] {
            assert!(
                notified.borrow().iter().any(|p| p == property),
                "{property}"
            );
        }
        assert_eq!(*settings.saved.borrow(), [Setting::Theme(light_soft)]);
    }

    #[test]
    fn highlights_wait_for_neovim() {
        let (vm, nvim, _) = setup(ThemeChoice::default());
        vm.set_choice(ThemeChoice::Fixed(Variant::DarkHard));
        assert!(nvim.take_calls().is_empty());
        vm.attach_neovim(nvim.clone());
        vm.neovim_ready();
        assert_eq!(highlight_calls(&nvim), ["dark"]);
    }

    /// F5.R5: `system` follows the desktop and keeps its contrast.
    #[test]
    fn system_theme_follows_the_desktop() {
        let (vm, nvim, _) = setup(ThemeChoice::System(Contrast::Soft));
        vm.attach_neovim(nvim.clone());
        vm.neovim_ready();
        nvim.take_calls();
        assert_eq!(vm.variant(), "dark-soft");
        vm.set_system_prefers_dark(false);
        assert_eq!(vm.variant(), "light-soft");
        assert_eq!(highlight_calls(&nvim), ["light"]);
        // A fixed variant ignores the desktop.
        vm.set_choice(ThemeChoice::Fixed(Variant::DarkHard));
        vm.set_system_prefers_dark(true);
        vm.set_system_prefers_dark(false);
        assert_eq!(vm.variant(), "dark-hard");
    }

    #[test]
    fn same_variant_does_not_reapply() {
        let (vm, nvim, _) = setup(ThemeChoice::default());
        vm.attach_neovim(nvim.clone());
        vm.neovim_ready();
        nvim.take_calls();
        vm.set_choice(ThemeChoice::Fixed(Variant::DarkMedium));
        assert!(nvim.take_calls().is_empty());
    }
}
