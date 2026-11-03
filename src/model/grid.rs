//! The editor grid: cells, highlights, cursor and modes, updated from
//! `redraw` events (T1.2, T1.4).
//!
//! RingMD does not use `ext_multigrid`, so only grid 1 exists.

use std::collections::HashMap;

use super::nvim::redraw::RedrawEvent;

/// Highlight attributes from `hl_attr_define`. Colors are `0xRRGGBB`;
/// `None` means "use the default color".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HlAttr {
    pub fg: Option<u32>,
    pub bg: Option<u32>,
    pub sp: Option<u32>,
    pub reverse: bool,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub undercurl: bool,
    pub strikethrough: bool,
}

/// A fully resolved cell style: defaults filled in and `reverse` applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Style {
    pub fg: u32,
    pub bg: u32,
    pub sp: u32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub undercurl: bool,
    pub strikethrough: bool,
}

/// One grid cell. `text` is empty for the right half of a double-width
/// character (F3.R4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub text: String,
    pub hl: u64,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            text: " ".into(),
            hl: 0,
        }
    }
}

/// Cursor shape for a mode (F3.R2).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CursorShape {
    #[default]
    Block,
    Horizontal,
    Vertical,
}

/// One entry of `mode_info_set`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeInfo {
    pub name: String,
    pub cursor_shape: CursorShape,
    /// Size of a bar cursor as a percentage of the cell.
    pub cell_percentage: u32,
    /// Highlight for the cursor; `0` means "reverse the cell colors".
    pub attr_id: u64,
}

impl Default for ModeInfo {
    fn default() -> Self {
        Self {
            name: "normal".into(),
            cursor_shape: CursorShape::Block,
            cell_percentage: 100,
            attr_id: 0,
        }
    }
}

/// The cursor as it should be drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
    pub shape: CursorShape,
    pub cell_percentage: u32,
    /// Style of the cursor itself (its `bg` is the cursor color).
    pub style: Style,
}

/// What changed since the last [`Grid::take_damage`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Damage {
    /// Everything (resize, colors or highlights changed).
    Full,
    /// Only these rows, in ascending order.
    Rows(Vec<usize>),
}

/// Default colors when Neovim has not sent any (Gruvbox dark).
const DEFAULT_FG: u32 = 0xebdbb2;
const DEFAULT_BG: u32 = 0x282828;
const DEFAULT_SP: u32 = 0xfb4934;

#[derive(Debug, Clone)]
pub struct Grid {
    width: usize,
    height: usize,
    cells: Vec<Cell>,
    highlights: HashMap<u64, HlAttr>,
    default_fg: u32,
    default_bg: u32,
    default_sp: u32,
    cursor_row: usize,
    cursor_col: usize,
    modes: Vec<ModeInfo>,
    mode: usize,
    mode_name: String,
    dirty_rows: Vec<bool>,
    full_damage: bool,
}

impl Default for Grid {
    fn default() -> Self {
        Self::new(0, 0)
    }
}

impl Grid {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            cells: vec![Cell::default(); width * height],
            highlights: HashMap::new(),
            default_fg: DEFAULT_FG,
            default_bg: DEFAULT_BG,
            default_sp: DEFAULT_SP,
            cursor_row: 0,
            cursor_col: 0,
            modes: Vec::new(),
            mode: 0,
            mode_name: "normal".into(),
            dirty_rows: vec![false; height],
            full_damage: true,
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn cell(&self, row: usize, col: usize) -> Option<&Cell> {
        if row < self.height && col < self.width {
            self.cells.get(row * self.width + col)
        } else {
            None
        }
    }

    /// The cells of one row.
    pub fn row(&self, row: usize) -> &[Cell] {
        if row < self.height {
            &self.cells[row * self.width..(row + 1) * self.width]
        } else {
            &[]
        }
    }

    /// The text of one row (handy in tests and logs).
    pub fn row_text(&self, row: usize) -> String {
        self.row(row).iter().map(|c| c.text.as_str()).collect()
    }

    /// Name of the current mode, e.g. `"normal"` or `"insert"`.
    pub fn mode_name(&self) -> &str {
        &self.mode_name
    }

    /// The default background (used to clear the widget).
    pub fn default_bg(&self) -> u32 {
        self.default_bg
    }

    /// Resolve a highlight id into concrete colors and attributes.
    pub fn style(&self, hl: u64) -> Style {
        let attr = self.highlights.get(&hl).copied().unwrap_or_default();
        let mut fg = attr.fg.unwrap_or(self.default_fg);
        let mut bg = attr.bg.unwrap_or(self.default_bg);
        if attr.reverse {
            std::mem::swap(&mut fg, &mut bg);
        }
        Style {
            fg,
            bg,
            sp: attr.sp.unwrap_or(self.default_sp),
            bold: attr.bold,
            italic: attr.italic,
            underline: attr.underline,
            undercurl: attr.undercurl,
            strikethrough: attr.strikethrough,
        }
    }

    /// The cursor for the current mode (F3.R2).
    pub fn cursor(&self) -> Cursor {
        let info = self.modes.get(self.mode).cloned().unwrap_or_default();
        let cell_hl = self
            .cell(self.cursor_row, self.cursor_col)
            .map_or(0, |c| c.hl);
        let style = if info.attr_id == 0 {
            // No `Cursor` highlight: invert the cell under the cursor.
            let cell = self.style(cell_hl);
            Style {
                fg: cell.bg,
                bg: cell.fg,
                ..cell
            }
        } else {
            let attr = self
                .highlights
                .get(&info.attr_id)
                .copied()
                .unwrap_or_default();
            let cell = self.style(cell_hl);
            let mut fg = attr.fg.unwrap_or(cell.bg);
            let mut bg = attr.bg.unwrap_or(cell.fg);
            if attr.reverse {
                std::mem::swap(&mut fg, &mut bg);
            }
            Style { fg, bg, ..cell }
        };
        Cursor {
            row: self.cursor_row,
            col: self.cursor_col,
            shape: info.cursor_shape,
            cell_percentage: info.cell_percentage.clamp(1, 100),
            style,
        }
    }

    /// Return and reset what changed since the last call.
    pub fn take_damage(&mut self) -> Damage {
        if std::mem::take(&mut self.full_damage) {
            self.dirty_rows.fill(false);
            return Damage::Full;
        }
        let rows = self
            .dirty_rows
            .iter_mut()
            .enumerate()
            .filter_map(|(row, dirty)| std::mem::take(dirty).then_some(row))
            .collect();
        Damage::Rows(rows)
    }

    fn mark_row(&mut self, row: usize) {
        if let Some(dirty) = self.dirty_rows.get_mut(row) {
            *dirty = true;
        }
    }

    /// Apply one decoded `redraw` event. Returns `true` on `flush`, when the
    /// grid is ready to be drawn.
    pub fn apply(&mut self, event: &RedrawEvent) -> bool {
        match event {
            RedrawEvent::GridResize {
                grid: 1,
                width,
                height,
            } => self.resize(*width, *height),
            RedrawEvent::DefaultColorsSet { fg, bg, sp } => {
                self.default_fg = fg.unwrap_or(DEFAULT_FG);
                self.default_bg = bg.unwrap_or(DEFAULT_BG);
                self.default_sp = sp.unwrap_or(DEFAULT_SP);
                self.full_damage = true;
            }
            RedrawEvent::HlAttrDefine { id, attr } => {
                self.highlights.insert(*id, *attr);
                // Redefined ids can change cells already on screen.
                self.full_damage = true;
            }
            RedrawEvent::GridLine {
                grid: 1,
                row,
                col_start,
                cells,
            } => self.put_line(*row, *col_start, cells),
            RedrawEvent::GridClear { grid: 1 } => {
                self.cells.fill(Cell::default());
                self.full_damage = true;
            }
            RedrawEvent::GridCursorGoto { grid: 1, row, col } => {
                self.mark_row(self.cursor_row);
                self.cursor_row = *row;
                self.cursor_col = *col;
                self.mark_row(*row);
            }
            RedrawEvent::GridScroll {
                grid: 1,
                top,
                bot,
                left,
                right,
                rows,
            } => self.scroll(*top, *bot, *left, *right, *rows),
            RedrawEvent::ModeInfoSet { modes } => {
                self.modes.clone_from(modes);
                self.mark_row(self.cursor_row);
            }
            RedrawEvent::ModeChange { name, index } => {
                self.mode = *index;
                self.mode_name.clone_from(name);
                self.mark_row(self.cursor_row);
            }
            RedrawEvent::Flush => return true,
            _ => {}
        }
        false
    }

    fn resize(&mut self, width: usize, height: usize) {
        let mut cells = vec![Cell::default(); width * height];
        for row in 0..height.min(self.height) {
            for col in 0..width.min(self.width) {
                cells[row * width + col] = self.cells[row * self.width + col].clone();
            }
        }
        self.cells = cells;
        self.width = width;
        self.height = height;
        self.dirty_rows = vec![false; height];
        self.cursor_row = self.cursor_row.min(height.saturating_sub(1));
        self.cursor_col = self.cursor_col.min(width.saturating_sub(1));
        self.full_damage = true;
    }

    fn put_line(&mut self, row: usize, col_start: usize, cells: &[super::nvim::redraw::LineCell]) {
        if row >= self.height {
            return;
        }
        let base = row * self.width;
        let mut col = col_start;
        let mut hl = 0;
        for cell in cells {
            if let Some(id) = cell.hl {
                hl = id;
            }
            for _ in 0..cell.repeat {
                if col >= self.width {
                    break;
                }
                let target = &mut self.cells[base + col];
                target.text.clone_from(&cell.text);
                target.hl = hl;
                col += 1;
            }
        }
        self.mark_row(row);
    }

    /// `grid_scroll`: move the region `[top, bot) × [left, right)` up by
    /// `rows` (down when negative). Vacated rows are redrawn by Neovim.
    fn scroll(&mut self, top: usize, bot: usize, left: usize, right: usize, rows: i64) {
        let bot = bot.min(self.height);
        let right = right.min(self.width);
        let Ok(shift) = usize::try_from(rows.unsigned_abs()) else {
            return;
        };
        if top >= bot || left >= right || shift == 0 {
            return;
        }
        let copy_row = |cells: &mut Vec<Cell>, width: usize, from: usize, to: usize| {
            for col in left..right {
                cells[to * width + col] = cells[from * width + col].clone();
            }
        };
        if rows > 0 {
            for row in top..bot.saturating_sub(shift) {
                copy_row(&mut self.cells, self.width, row + shift, row);
            }
        } else {
            for row in (top + shift..bot).rev() {
                copy_row(&mut self.cells, self.width, row - shift, row);
            }
        }
        for row in top..bot {
            self.mark_row(row);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::nvim::redraw::LineCell;

    fn line(row: usize, col: usize, text: &str, hl: u64) -> RedrawEvent {
        RedrawEvent::GridLine {
            grid: 1,
            row,
            col_start: col,
            cells: text
                .chars()
                .enumerate()
                .map(|(i, c)| LineCell {
                    text: c.to_string(),
                    hl: (i == 0).then_some(hl),
                    repeat: 1,
                })
                .collect(),
        }
    }

    fn grid_with(rows: &[&str]) -> Grid {
        let width = rows.iter().map(|r| r.len()).max().unwrap_or(0);
        let mut grid = Grid::new(0, 0);
        grid.apply(&RedrawEvent::GridResize {
            grid: 1,
            width,
            height: rows.len(),
        });
        for (row, text) in rows.iter().enumerate() {
            grid.apply(&line(row, 0, text, 0));
        }
        grid.take_damage();
        grid
    }

    fn texts(grid: &Grid) -> Vec<String> {
        (0..grid.height()).map(|r| grid.row_text(r)).collect()
    }

    #[test]
    fn grid_line_with_repeat_and_inherited_hl() {
        let mut grid = Grid::new(6, 1);
        grid.apply(&RedrawEvent::GridLine {
            grid: 1,
            row: 0,
            col_start: 1,
            cells: vec![
                LineCell {
                    text: "a".into(),
                    hl: Some(4),
                    repeat: 1,
                },
                LineCell {
                    text: "-".into(),
                    hl: None,
                    repeat: 3,
                },
            ],
        });
        assert_eq!(grid.row_text(0), " a--- ");
        assert_eq!(grid.cell(0, 3).unwrap().hl, 4);
        assert_eq!(grid.cell(0, 5).unwrap().hl, 0);
    }

    #[test]
    fn grid_line_clips_at_width() {
        let mut grid = Grid::new(3, 1);
        grid.apply(&line(0, 1, "abcdef", 0));
        assert_eq!(grid.row_text(0), " ab");
    }

    #[test]
    fn wide_characters_take_two_cells() {
        let mut grid = Grid::new(4, 1);
        grid.apply(&RedrawEvent::GridLine {
            grid: 1,
            row: 0,
            col_start: 0,
            cells: vec![
                LineCell {
                    text: "日".into(),
                    hl: Some(0),
                    repeat: 1,
                },
                LineCell {
                    text: String::new(),
                    hl: None,
                    repeat: 1,
                },
                LineCell {
                    text: "x".into(),
                    hl: None,
                    repeat: 1,
                },
            ],
        });
        assert_eq!(grid.row_text(0), "日x ");
        assert_eq!(grid.cell(0, 1).unwrap().text, "");
    }

    #[test]
    fn scroll_up_and_down() {
        let mut grid = grid_with(&["aaa", "bbb", "ccc", "ddd"]);
        grid.apply(&RedrawEvent::GridScroll {
            grid: 1,
            top: 0,
            bot: 4,
            left: 0,
            right: 3,
            rows: 1,
        });
        assert_eq!(texts(&grid), ["bbb", "ccc", "ddd", "ddd"]);

        let mut grid = grid_with(&["aaa", "bbb", "ccc", "ddd"]);
        grid.apply(&RedrawEvent::GridScroll {
            grid: 1,
            top: 1,
            bot: 4,
            left: 0,
            right: 3,
            rows: -2,
        });
        assert_eq!(texts(&grid), ["aaa", "bbb", "ccc", "bbb"]);
    }

    #[test]
    fn scroll_respects_column_region() {
        let mut grid = grid_with(&["abc", "def"]);
        grid.apply(&RedrawEvent::GridScroll {
            grid: 1,
            top: 0,
            bot: 2,
            left: 1,
            right: 2,
            rows: 1,
        });
        assert_eq!(texts(&grid), ["aec", "def"]);
    }

    #[test]
    fn resize_keeps_overlapping_content() {
        let mut grid = grid_with(&["abc", "def"]);
        grid.apply(&RedrawEvent::GridResize {
            grid: 1,
            width: 2,
            height: 3,
        });
        assert_eq!(texts(&grid), ["ab", "de", "  "]);
        assert_eq!(grid.take_damage(), Damage::Full);
    }

    #[test]
    fn clear_resets_cells() {
        let mut grid = grid_with(&["abc"]);
        grid.apply(&RedrawEvent::GridClear { grid: 1 });
        assert_eq!(grid.row_text(0), "   ");
    }

    #[test]
    fn damage_tracks_rows() {
        let mut grid = grid_with(&["aaa", "bbb", "ccc"]);
        grid.apply(&line(2, 0, "x", 0));
        grid.apply(&RedrawEvent::GridCursorGoto {
            grid: 1,
            row: 1,
            col: 0,
        });
        // Cursor moved from row 0 to row 1: both need repainting.
        assert_eq!(grid.take_damage(), Damage::Rows(vec![0, 1, 2]));
        assert_eq!(grid.take_damage(), Damage::Rows(vec![]));
    }

    #[test]
    fn styles_resolve_defaults_and_reverse() {
        let mut grid = Grid::new(1, 1);
        grid.apply(&RedrawEvent::DefaultColorsSet {
            fg: Some(0x111111),
            bg: Some(0x222222),
            sp: None,
        });
        grid.apply(&RedrawEvent::HlAttrDefine {
            id: 1,
            attr: HlAttr {
                fg: Some(0xff0000),
                reverse: true,
                bold: true,
                ..HlAttr::default()
            },
        });
        let style = grid.style(1);
        assert_eq!((style.fg, style.bg), (0x222222, 0xff0000));
        assert!(style.bold);
        let plain = grid.style(0);
        assert_eq!(
            (plain.fg, plain.bg, plain.sp),
            (0x111111, 0x222222, DEFAULT_SP)
        );
    }

    #[test]
    fn cursor_follows_mode() {
        let mut grid = grid_with(&["abc"]);
        grid.apply(&RedrawEvent::ModeInfoSet {
            modes: vec![
                ModeInfo::default(),
                ModeInfo {
                    name: "insert".into(),
                    cursor_shape: CursorShape::Vertical,
                    cell_percentage: 25,
                    attr_id: 0,
                },
                ModeInfo {
                    name: "replace".into(),
                    cursor_shape: CursorShape::Horizontal,
                    cell_percentage: 20,
                    attr_id: 0,
                },
            ],
        });
        grid.apply(&RedrawEvent::GridCursorGoto {
            grid: 1,
            row: 0,
            col: 2,
        });
        assert_eq!(grid.cursor().shape, CursorShape::Block);

        grid.apply(&RedrawEvent::ModeChange {
            name: "insert".into(),
            index: 1,
        });
        let cursor = grid.cursor();
        assert_eq!((cursor.row, cursor.col), (0, 2));
        assert_eq!(cursor.shape, CursorShape::Vertical);
        assert_eq!(cursor.cell_percentage, 25);
        assert_eq!(grid.mode_name(), "insert");

        grid.apply(&RedrawEvent::ModeChange {
            name: "replace".into(),
            index: 2,
        });
        assert_eq!(grid.cursor().shape, CursorShape::Horizontal);
    }

    #[test]
    fn block_cursor_inverts_cell_colors() {
        let grid = grid_with(&["a"]);
        let cursor = grid.cursor();
        assert_eq!(cursor.style.fg, DEFAULT_BG);
        assert_eq!(cursor.style.bg, DEFAULT_FG);
    }

    #[test]
    fn other_grids_are_ignored() {
        let mut grid = grid_with(&["abc"]);
        grid.apply(&RedrawEvent::GridClear { grid: 2 });
        assert_eq!(grid.row_text(0), "abc");
    }

    #[test]
    fn flush_reports_ready() {
        let mut grid = Grid::new(1, 1);
        assert!(!grid.apply(&RedrawEvent::GridClear { grid: 1 }));
        assert!(grid.apply(&RedrawEvent::Flush));
    }
}
