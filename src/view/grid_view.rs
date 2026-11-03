//! Draws the Neovim grid and forwards input (T1.3–T1.7, T1.11).
//!
//! Rendering goes through GTK snapshots. Each grid row is recorded once into
//! a render node and reused until Neovim changes that row, so a redraw only
//! rebuilds damaged rows (F3.R3, N4).

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, graphene, gsk, pango};

use crate::model::grid::{CursorShape, Damage, Grid, Style};
use crate::model::nvim::input::{KeyPress, Modifiers, MouseButton};
use crate::viewmodel::EditorViewModel;

/// Font measurements in logical pixels.
#[derive(Debug, Clone, Copy, Default)]
struct Metrics {
    cell_w: f64,
    cell_h: f64,
    ascent: f64,
    underline_pos: f64,
    underline_thickness: f64,
    strike_pos: f64,
    /// Extra spacing (Pango units) that stretches a glyph to the cell width.
    letter_spacing: i32,
}

mod imp {
    use std::cell::{Cell, OnceCell, RefCell};

    use super::*;

    #[derive(Debug, Default)]
    pub struct GridView {
        pub(super) vm: OnceCell<EditorViewModel>,
        pub(super) im: OnceCell<gtk::IMMulticontext>,
        pub(super) font: RefCell<Option<pango::FontDescription>>,
        pub(super) metrics: Cell<Metrics>,
        /// Cached row nodes: outer `None` = needs rebuilding, inner `None` =
        /// nothing to draw (blank row).
        pub(super) rows: RefCell<Vec<Option<Option<gsk::RenderNode>>>>,
        /// Last pointer position, for scroll events.
        pub(super) pointer: Cell<(f64, f64)>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for GridView {
        const NAME: &'static str = "RingmdGridView";
        type Type = super::GridView;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("ringmd-grid");
            klass.set_accessible_role(gtk::AccessibleRole::TextBox);
        }
    }

    impl ObjectImpl for GridView {}

    impl WidgetImpl for GridView {
        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            self.parent_size_allocate(width, height, baseline);
            if let Some(vm) = self.vm.get() {
                vm.resize(f64::from(width), f64::from(height));
            }
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let Some(vm) = self.vm.get() else {
                return;
            };
            let grid = vm.grid();
            let metrics = self.metrics.get();
            let obj = self.obj();
            let (width, height) = (obj.width() as f32, obj.height() as f32);

            snapshot.append_color(
                &rgba(grid.default_bg()),
                &graphene::Rect::new(0.0, 0.0, width, height),
            );
            if metrics.cell_h <= 0.0 {
                return;
            }

            let mut rows = self.rows.borrow_mut();
            rows.resize(grid.height(), None);
            for (row, cached) in rows.iter_mut().enumerate() {
                let node = cached.get_or_insert_with(|| self.record_row(&grid, row, &metrics));
                if let Some(node) = node {
                    snapshot.save();
                    snapshot.translate(&point(0.0, row as f64 * metrics.cell_h));
                    snapshot.append_node(node);
                    snapshot.restore();
                }
            }
            drop(rows);

            self.draw_cursor(snapshot, &grid, &metrics, obj.has_focus());
        }
    }

    impl GridView {
        /// Record one row into a render node at the origin.
        fn record_row(&self, grid: &Grid, row: usize, m: &Metrics) -> Option<gsk::RenderNode> {
            let snapshot = gtk::Snapshot::new();
            let default_bg = grid.default_bg();
            for run in runs(grid, row) {
                let style = grid.style(run.hl);
                let x = run.col as f64 * m.cell_w;
                let w = run.cells as f64 * m.cell_w;
                if style.bg != default_bg {
                    snapshot.append_color(&rgba(style.bg), &rect(x, 0.0, w, m.cell_h));
                }
                if !run.text.trim().is_empty() {
                    self.draw_text(&snapshot, &run.text, &style, x, m);
                }
                draw_decorations(&snapshot, &style, x, w, m);
            }
            snapshot.to_node()
        }

        fn layout(&self, text: &str, style: &Style, m: &Metrics) -> pango::Layout {
            let layout = self.obj().create_pango_layout(Some(text));
            if let Some(font) = self.font.borrow().as_ref() {
                let mut font = font.clone();
                if style.bold {
                    font.set_weight(pango::Weight::Bold);
                }
                if style.italic {
                    font.set_style(pango::Style::Italic);
                }
                layout.set_font_description(Some(&font));
            }
            if m.letter_spacing != 0 {
                let attrs = pango::AttrList::new();
                attrs.insert(pango::AttrInt::new_letter_spacing(m.letter_spacing));
                layout.set_attributes(Some(&attrs));
            }
            layout
        }

        fn draw_text(
            &self,
            snapshot: &gtk::Snapshot,
            text: &str,
            style: &Style,
            x: f64,
            m: &Metrics,
        ) {
            let layout = self.layout(text, style, m);
            // Align baselines even when a fallback font (CJK, emoji) is taller.
            let baseline = f64::from(layout.baseline()) / f64::from(pango::SCALE);
            snapshot.save();
            snapshot.translate(&point(x, m.ascent - baseline));
            snapshot.append_layout(&layout, &rgba(style.fg));
            snapshot.restore();
        }

        fn draw_cursor(&self, snapshot: &gtk::Snapshot, grid: &Grid, m: &Metrics, focused: bool) {
            let cursor = grid.cursor();
            if cursor.row >= grid.height() || cursor.col >= grid.width() {
                return;
            }
            let x = cursor.col as f64 * m.cell_w;
            let y = cursor.row as f64 * m.cell_h;
            let cell = grid.cell(cursor.row, cursor.col);
            let wide = grid
                .cell(cursor.row, cursor.col + 1)
                .is_some_and(|c| c.text.is_empty());
            let w = if wide { 2.0 * m.cell_w } else { m.cell_w };
            let color = rgba(cursor.style.bg);
            let fraction = f64::from(cursor.cell_percentage) / 100.0;

            if !focused {
                // Hollow block while the editor does not have focus.
                let t = 1.0;
                for r in [
                    rect(x, y, w, t),
                    rect(x, y + m.cell_h - t, w, t),
                    rect(x, y, t, m.cell_h),
                    rect(x + w - t, y, t, m.cell_h),
                ] {
                    snapshot.append_color(&color, &r);
                }
                return;
            }
            match cursor.shape {
                CursorShape::Block => {
                    snapshot.append_color(&color, &rect(x, y, w, m.cell_h));
                    if let Some(cell) = cell.filter(|c| !c.text.trim().is_empty()) {
                        snapshot.save();
                        snapshot.translate(&point(0.0, y));
                        self.draw_text(snapshot, &cell.text, &cursor.style, x, m);
                        snapshot.restore();
                    }
                }
                CursorShape::Vertical => {
                    let bar = (m.cell_w * fraction).max(1.0);
                    snapshot.append_color(&color, &rect(x, y, bar, m.cell_h));
                }
                CursorShape::Horizontal => {
                    let bar = (m.cell_h * fraction).max(1.0);
                    snapshot.append_color(&color, &rect(x, y + m.cell_h - bar, w, bar));
                }
            }
        }
    }
}

glib::wrapper! {
    /// The editor widget (View for [`EditorViewModel`]).
    pub struct GridView(ObjectSubclass<imp::GridView>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl GridView {
    pub fn new(vm: &EditorViewModel) -> Self {
        let view: Self = glib::Object::builder()
            .property("focusable", true)
            .property("focus-on-click", true)
            .property("hexpand", true)
            .property("vexpand", true)
            .property("overflow", gtk::Overflow::Hidden)
            .build();
        let imp = view.imp();
        let _ = imp.vm.set(vm.clone());
        view.set_cursor_from_name(Some("text"));

        view.update_font();
        vm.connect_font_notify(glib::clone!(
            #[weak]
            view,
            move |_| view.update_font()
        ));
        vm.connect_flushed(glib::clone!(
            #[weak]
            view,
            move |vm| view.on_flushed(vm)
        ));

        view.setup_keyboard(vm);
        view.setup_mouse(vm);
        view
    }

    /// Re-measure the font, tell the ViewModel the new cell size and redraw.
    fn update_font(&self) {
        let imp = self.imp();
        let Some(vm) = imp.vm.get() else {
            return;
        };
        let font = pango::FontDescription::from_string(&vm.font());
        let context = self.pango_context();
        let layout = pango::Layout::new(&context);
        layout.set_font_description(Some(&font));
        layout.set_text("M");
        let (_, logical) = layout.extents();
        let scale = f64::from(pango::SCALE);
        let advance = f64::from(logical.width()) / scale;
        let fm = context.metrics(Some(&font), None);

        let cell_w = advance.ceil().max(1.0);
        let cell_h = (f64::from(logical.height()) / scale).ceil().max(1.0);
        let ascent = f64::from(layout.baseline()) / scale;
        let metrics = Metrics {
            cell_w,
            cell_h,
            ascent,
            underline_pos: ascent - f64::from(fm.underline_position()) / scale,
            underline_thickness: (f64::from(fm.underline_thickness()) / scale).max(1.0),
            strike_pos: ascent - f64::from(fm.strikethrough_position()) / scale,
            letter_spacing: ((cell_w - advance) * scale).round() as i32,
        };
        tracing::debug!(font = %vm.font(), ?metrics, "font metrics");

        imp.font.replace(Some(font));
        imp.metrics.set(metrics);
        imp.rows.borrow_mut().clear();
        vm.set_cell_size(cell_w, cell_h);
        self.queue_draw();
    }

    fn on_flushed(&self, vm: &EditorViewModel) {
        let imp = self.imp();
        match vm.take_damage() {
            Damage::Full => imp.rows.borrow_mut().clear(),
            Damage::Rows(rows) => {
                let mut cache = imp.rows.borrow_mut();
                for row in rows {
                    if let Some(slot) = cache.get_mut(row) {
                        *slot = None;
                    }
                }
            }
        }
        // Keep IME popups next to the cursor.
        if let Some(im) = imp.im.get() {
            let m = imp.metrics.get();
            let cursor = vm.grid().cursor();
            im.set_cursor_location(&gdk::Rectangle::new(
                (cursor.col as f64 * m.cell_w) as i32,
                (cursor.row as f64 * m.cell_h) as i32,
                m.cell_w as i32,
                m.cell_h as i32,
            ));
        }
        self.queue_draw();
    }

    fn setup_keyboard(&self, vm: &EditorViewModel) {
        let im = gtk::IMMulticontext::new();
        im.set_client_widget(Some(self));
        im.connect_commit(glib::clone!(
            #[weak]
            vm,
            move |_, text| vm.commit_text(text)
        ));

        let keys = gtk::EventControllerKey::new();
        keys.set_im_context(Some(&im));
        keys.connect_key_pressed(glib::clone!(
            #[weak]
            vm,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, keyval, _, state| {
                let key = KeyPress {
                    name: keyval.name().map(|n| n.to_string()).unwrap_or_default(),
                    ch: keyval.to_unicode(),
                    modifiers: modifiers(state),
                };
                if vm.key_pressed(&key) {
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            }
        ));
        self.add_controller(keys);

        let focus = gtk::EventControllerFocus::new();
        focus.connect_enter(glib::clone!(
            #[weak(rename_to = view)]
            self,
            #[weak]
            im,
            move |_| {
                im.focus_in();
                view.queue_draw();
            }
        ));
        focus.connect_leave(glib::clone!(
            #[weak(rename_to = view)]
            self,
            #[weak]
            im,
            move |_| {
                im.focus_out();
                view.queue_draw();
            }
        ));
        self.add_controller(focus);
        let _ = self.imp().im.set(im);
    }

    fn setup_mouse(&self, vm: &EditorViewModel) {
        // Press, drag and release in one gesture; Neovim detects
        // double/triple clicks from press timing itself.
        let drag = gtk::GestureDrag::new();
        drag.set_button(0);
        drag.connect_drag_begin(glib::clone!(
            #[weak(rename_to = view)]
            self,
            #[weak]
            vm,
            move |gesture, x, y| {
                view.grab_focus();
                let Some(button) = mouse_button(gesture.current_button()) else {
                    return;
                };
                vm.mouse_pressed(button, modifiers(gesture.current_event_state()), x, y);
            }
        ));
        drag.connect_drag_update(glib::clone!(
            #[weak]
            vm,
            move |gesture, dx, dy| {
                if let Some((x, y)) = gesture.start_point() {
                    vm.mouse_dragged(modifiers(gesture.current_event_state()), x + dx, y + dy);
                }
            }
        ));
        drag.connect_drag_end(glib::clone!(
            #[weak]
            vm,
            move |gesture, dx, dy| {
                if let Some((x, y)) = gesture.start_point() {
                    vm.mouse_released(modifiers(gesture.current_event_state()), x + dx, y + dy);
                }
            }
        ));
        self.add_controller(drag);

        let motion = gtk::EventControllerMotion::new();
        motion.connect_motion(glib::clone!(
            #[weak(rename_to = view)]
            self,
            move |_, x, y| view.imp().pointer.set((x, y))
        ));
        self.add_controller(motion);

        let scroll = gtk::EventControllerScroll::new(
            gtk::EventControllerScrollFlags::BOTH_AXES | gtk::EventControllerScrollFlags::DISCRETE,
        );
        scroll.connect_scroll(glib::clone!(
            #[weak(rename_to = view)]
            self,
            #[weak]
            vm,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |controller, dx, dy| {
                let (x, y) = view.imp().pointer.get();
                vm.scrolled(modifiers(controller.current_event_state()), x, y, dx, dy);
                glib::Propagation::Stop
            }
        ));
        self.add_controller(scroll);
    }
}

/// A run of cells drawn with one style.
struct Run {
    col: usize,
    cells: usize,
    hl: u64,
    text: String,
}

/// Split a row into runs. ASCII cells with the same highlight are grouped;
/// other characters get their own run so wide glyphs from fallback fonts
/// cannot push later cells out of their columns (F3.R4).
fn runs(grid: &Grid, row: usize) -> Vec<Run> {
    let cells = grid.row(row);
    let mut runs = Vec::new();
    let mut col = 0;
    while col < cells.len() {
        let start = col;
        let hl = cells[col].hl;
        let mut text = String::new();
        let is_simple = |i: usize| {
            let text = &cells[i].text;
            text.len() == 1
                && text.is_ascii()
                && cells.get(i + 1).is_none_or(|next| !next.text.is_empty())
        };
        if is_simple(col) {
            while col < cells.len() && cells[col].hl == hl && is_simple(col) {
                text.push_str(&cells[col].text);
                col += 1;
            }
        } else {
            text.push_str(&cells[col].text);
            col += 1;
            // Right halves of a double-width character.
            while col < cells.len() && cells[col].text.is_empty() {
                col += 1;
            }
        }
        runs.push(Run {
            col: start,
            cells: col - start,
            hl,
            text,
        });
    }
    runs
}

fn draw_decorations(snapshot: &gtk::Snapshot, style: &Style, x: f64, w: f64, m: &Metrics) {
    let t = m.underline_thickness;
    if style.underline {
        snapshot.append_color(&rgba(style.sp), &rect(x, m.underline_pos, w, t));
    }
    if style.strikethrough {
        snapshot.append_color(&rgba(style.fg), &rect(x, m.strike_pos, w, t));
    }
    if style.undercurl {
        let top = m.underline_pos - t;
        let height = (m.cell_h - top).max(2.0 * t);
        let cr = snapshot.append_cairo(&rect(x, top, w, height));
        let (r, g, b) = channels(style.sp);
        cr.set_source_rgb(r, g, b);
        cr.set_line_width(t);
        let amplitude = (height / 2.0 - t).max(0.5);
        let step = 2.0;
        cr.move_to(x, top + height / 2.0);
        let mut px = x;
        let mut up = true;
        while px < x + w {
            px += step;
            let py = top + height / 2.0 + if up { -amplitude } else { amplitude };
            cr.line_to(px.min(x + w), py);
            up = !up;
        }
        let _ = cr.stroke();
    }
}

fn modifiers(state: gdk::ModifierType) -> Modifiers {
    Modifiers {
        shift: state.contains(gdk::ModifierType::SHIFT_MASK),
        ctrl: state.contains(gdk::ModifierType::CONTROL_MASK),
        alt: state.contains(gdk::ModifierType::ALT_MASK),
        logo: state.contains(gdk::ModifierType::SUPER_MASK),
    }
}

fn mouse_button(button: u32) -> Option<MouseButton> {
    match button {
        gdk::BUTTON_PRIMARY => Some(MouseButton::Left),
        gdk::BUTTON_MIDDLE => Some(MouseButton::Middle),
        gdk::BUTTON_SECONDARY => Some(MouseButton::Right),
        _ => None,
    }
}

fn channels(color: u32) -> (f64, f64, f64) {
    let c = |shift: u32| f64::from((color >> shift) & 0xff) / 255.0;
    (c(16), c(8), c(0))
}

fn rgba(color: u32) -> gdk::RGBA {
    let (r, g, b) = channels(color);
    gdk::RGBA::new(r as f32, g as f32, b as f32, 1.0)
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> graphene::Rect {
    graphene::Rect::new(x as f32, y as f32, w as f32, h as f32)
}

fn point(x: f64, y: f64) -> graphene::Point {
    graphene::Point::new(x as f32, y as f32)
}
