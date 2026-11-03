//! Gruvbox theme (F5): six variants, one palette per variant, and the three
//! outputs generated from it (F5.R2): Neovim highlight groups, GTK CSS for
//! the window chrome, and CSS custom properties for the preview.

use std::fmt;
use std::str::FromStr;

use rmpv::Value;

/// Contrast level of a variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Contrast {
    Hard,
    #[default]
    Medium,
    Soft,
}

impl Contrast {
    pub const ALL: [Self; 3] = [Self::Hard, Self::Medium, Self::Soft];

    pub fn name(self) -> &'static str {
        match self {
            Self::Hard => "hard",
            Self::Medium => "medium",
            Self::Soft => "soft",
        }
    }
}

/// One of the six Gruvbox variants (F5.R1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Variant {
    DarkHard,
    #[default]
    DarkMedium,
    DarkSoft,
    LightHard,
    LightMedium,
    LightSoft,
}

impl Variant {
    /// Every variant, in menu order.
    pub const ALL: [Self; 6] = [
        Self::DarkHard,
        Self::DarkMedium,
        Self::DarkSoft,
        Self::LightHard,
        Self::LightMedium,
        Self::LightSoft,
    ];

    /// The variant with the given brightness and contrast.
    pub fn new(dark: bool, contrast: Contrast) -> Self {
        match (dark, contrast) {
            (true, Contrast::Hard) => Self::DarkHard,
            (true, Contrast::Medium) => Self::DarkMedium,
            (true, Contrast::Soft) => Self::DarkSoft,
            (false, Contrast::Hard) => Self::LightHard,
            (false, Contrast::Medium) => Self::LightMedium,
            (false, Contrast::Soft) => Self::LightSoft,
        }
    }

    /// The name used by `:Theme` and the config file, e.g. `dark-medium`.
    pub fn name(self) -> &'static str {
        match self {
            Self::DarkHard => "dark-hard",
            Self::DarkMedium => "dark-medium",
            Self::DarkSoft => "dark-soft",
            Self::LightHard => "light-hard",
            Self::LightMedium => "light-medium",
            Self::LightSoft => "light-soft",
        }
    }

    pub fn is_dark(self) -> bool {
        matches!(self, Self::DarkHard | Self::DarkMedium | Self::DarkSoft)
    }

    pub fn contrast(self) -> Contrast {
        match self {
            Self::DarkHard | Self::LightHard => Contrast::Hard,
            Self::DarkMedium | Self::LightMedium => Contrast::Medium,
            Self::DarkSoft | Self::LightSoft => Contrast::Soft,
        }
    }

    /// The colours of this variant (Appendix A).
    pub fn palette(self) -> Palette {
        let bg0 = match self {
            Self::DarkHard => Color(0x1d2021),
            Self::DarkMedium => Color(0x282828),
            Self::DarkSoft => Color(0x32302f),
            Self::LightHard => Color(0xf9f5d7),
            Self::LightMedium => Color(0xfbf1c7),
            Self::LightSoft => Color(0xf2e5bc),
        };
        if self.is_dark() {
            Palette {
                bg0,
                bg1: Color(0x3c3836),
                bg2: Color(0x504945),
                bg3: Color(0x665c54),
                bg4: Color(0x7c6f64),
                fg0: Color(0xfbf1c7),
                fg1: Color(0xebdbb2),
                fg2: Color(0xd5c4a1),
                gray: Color(0x928374),
                red: Color(0xfb4934),
                green: Color(0xb8bb26),
                yellow: Color(0xfabd2f),
                blue: Color(0x83a598),
                purple: Color(0xd3869b),
                aqua: Color(0x8ec07c),
                orange: Color(0xfe8019),
            }
        } else {
            Palette {
                bg0,
                bg1: Color(0xebdbb2),
                bg2: Color(0xd5c4a1),
                bg3: Color(0xbdae93),
                bg4: Color(0xa89984),
                fg0: Color(0x282828),
                fg1: Color(0x3c3836),
                fg2: Color(0x504945),
                gray: Color(0x928374),
                red: Color(0x9d0006),
                green: Color(0x79740e),
                yellow: Color(0xb57614),
                blue: Color(0x076678),
                purple: Color(0x8f3f71),
                aqua: Color(0x427b58),
                orange: Color(0xaf3a03),
            }
        }
    }
}

impl fmt::Display for Variant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Error for an unknown variant or theme name.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown theme `{0}`")]
pub struct UnknownVariant(pub String);

impl FromStr for Variant {
    type Err = UnknownVariant;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|v| v.name() == s)
            .ok_or_else(|| UnknownVariant(s.to_owned()))
    }
}

/// What the user chose: a fixed variant, or follow the desktop's dark/light
/// preference with a given contrast (F5.R5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThemeChoice {
    Fixed(Variant),
    System(Contrast),
}

impl ThemeChoice {
    /// Every name `:Theme` accepts, in completion order.
    pub fn names() -> Vec<String> {
        let mut names: Vec<String> = Variant::ALL.iter().map(|v| v.name().to_owned()).collect();
        names.push("system".into());
        // `system` already means medium contrast.
        names.extend(
            [Contrast::Hard, Contrast::Soft]
                .iter()
                .map(|c| format!("system-{}", c.name())),
        );
        names
    }

    /// The variant to show, given whether the desktop prefers dark.
    pub fn resolve(self, system_prefers_dark: bool) -> Variant {
        match self {
            Self::Fixed(variant) => variant,
            Self::System(contrast) => Variant::new(system_prefers_dark, contrast),
        }
    }

    pub fn name(self) -> String {
        match self {
            Self::Fixed(variant) => variant.name().to_owned(),
            Self::System(Contrast::Medium) => "system".to_owned(),
            Self::System(contrast) => format!("system-{}", contrast.name()),
        }
    }
}

impl Default for ThemeChoice {
    fn default() -> Self {
        Self::Fixed(Variant::default())
    }
}

impl FromStr for ThemeChoice {
    type Err = UnknownVariant;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "system" {
            return Ok(Self::System(Contrast::Medium));
        }
        if let Some(contrast) = s.strip_prefix("system-") {
            return Contrast::ALL
                .into_iter()
                .find(|c| c.name() == contrast)
                .map(Self::System)
                .ok_or_else(|| UnknownVariant(s.to_owned()));
        }
        s.parse().map(Self::Fixed)
    }
}

/// An RGB colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color(pub u32);

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:06x}", self.0)
    }
}

/// The named colours of one variant (Appendix A).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub bg0: Color,
    pub bg1: Color,
    pub bg2: Color,
    pub bg3: Color,
    pub bg4: Color,
    pub fg0: Color,
    pub fg1: Color,
    pub fg2: Color,
    pub gray: Color,
    pub red: Color,
    pub green: Color,
    pub yellow: Color,
    pub blue: Color,
    pub purple: Color,
    pub aqua: Color,
    pub orange: Color,
}

/// One Neovim highlight group definition (`nvim_set_hl`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Highlight {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub sp: Option<Color>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub undercurl: bool,
    pub strikethrough: bool,
    pub reverse: bool,
    pub link: Option<&'static str>,
}

impl Highlight {
    fn fg(color: Color) -> Self {
        Self {
            fg: Some(color),
            ..Self::default()
        }
    }

    fn fg_bg(fg: Color, bg: Color) -> Self {
        Self {
            fg: Some(fg),
            bg: Some(bg),
            ..Self::default()
        }
    }

    fn link(group: &'static str) -> Self {
        Self {
            link: Some(group),
            ..Self::default()
        }
    }

    fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    fn italic(mut self) -> Self {
        self.italic = true;
        self
    }

    fn underline(mut self) -> Self {
        self.underline = true;
        self
    }

    fn undercurl(sp: Color) -> Self {
        Self {
            sp: Some(sp),
            undercurl: true,
            ..Self::default()
        }
    }

    /// The `{val}` map for `nvim_set_hl`.
    pub fn to_value(&self) -> Value {
        if let Some(group) = self.link {
            return Value::Map(vec![("link".into(), group.into())]);
        }
        let mut map: Vec<(Value, Value)> = Vec::new();
        for (key, color) in [("fg", self.fg), ("bg", self.bg), ("sp", self.sp)] {
            if let Some(color) = color {
                map.push((key.into(), color.to_string().into()));
            }
        }
        for (key, on) in [
            ("bold", self.bold),
            ("italic", self.italic),
            ("underline", self.underline),
            ("undercurl", self.undercurl),
            ("strikethrough", self.strikethrough),
            ("reverse", self.reverse),
        ] {
            if on {
                map.push((key.into(), true.into()));
            }
        }
        Value::Map(map)
    }
}

impl Palette {
    /// Highlight groups for the editor: UI, syntax, diagnostics, Markdown
    /// and Treesitter Markdown captures (F5.R3).
    pub fn highlights(&self) -> Vec<(&'static str, Highlight)> {
        use Highlight as H;
        let p = self;
        let mut groups = vec![
            // Editor UI.
            ("Normal", H::fg_bg(p.fg1, p.bg0)),
            ("NormalNC", H::link("Normal")),
            ("NormalFloat", H::fg_bg(p.fg1, p.bg1)),
            ("FloatBorder", H::fg_bg(p.bg4, p.bg1)),
            ("EndOfBuffer", H::fg(p.bg2)),
            ("NonText", H::fg(p.bg2)),
            ("Whitespace", H::fg(p.bg2)),
            ("SpecialKey", H::fg(p.bg4)),
            ("LineNr", H::fg(p.bg4)),
            ("CursorLineNr", H::fg(p.yellow).bold()),
            (
                "CursorLine",
                H {
                    bg: Some(p.bg1),
                    ..H::default()
                },
            ),
            ("CursorColumn", H::link("CursorLine")),
            (
                "ColorColumn",
                H {
                    bg: Some(p.bg1),
                    ..H::default()
                },
            ),
            ("SignColumn", H::fg(p.fg1)),
            ("Folded", H::fg_bg(p.gray, p.bg1).italic()),
            ("FoldColumn", H::fg(p.gray)),
            (
                "Cursor",
                H {
                    reverse: true,
                    ..H::default()
                },
            ),
            (
                "Visual",
                H {
                    bg: Some(p.bg3),
                    ..H::default()
                },
            ),
            ("VisualNOS", H::link("Visual")),
            ("Search", H::fg_bg(p.bg0, p.yellow)),
            ("IncSearch", H::fg_bg(p.bg0, p.orange)),
            ("CurSearch", H::link("IncSearch")),
            ("Substitute", H::link("IncSearch")),
            (
                "MatchParen",
                H {
                    bg: Some(p.bg3),
                    bold: true,
                    ..H::default()
                },
            ),
            ("StatusLine", H::fg_bg(p.fg1, p.bg2)),
            ("StatusLineNC", H::fg_bg(p.gray, p.bg1)),
            ("WinSeparator", H::fg(p.bg3)),
            ("VertSplit", H::link("WinSeparator")),
            ("TabLine", H::fg_bg(p.gray, p.bg1)),
            (
                "TabLineFill",
                H {
                    bg: Some(p.bg1),
                    ..H::default()
                },
            ),
            ("TabLineSel", H::fg_bg(p.fg1, p.bg0).bold()),
            ("Pmenu", H::fg_bg(p.fg1, p.bg1)),
            ("PmenuSel", H::fg_bg(p.bg1, p.blue).bold()),
            (
                "PmenuSbar",
                H {
                    bg: Some(p.bg2),
                    ..H::default()
                },
            ),
            (
                "PmenuThumb",
                H {
                    bg: Some(p.bg4),
                    ..H::default()
                },
            ),
            ("WildMenu", H::link("PmenuSel")),
            ("ModeMsg", H::fg(p.yellow).bold()),
            ("MoreMsg", H::fg(p.yellow).bold()),
            ("Question", H::fg(p.orange).bold()),
            ("WarningMsg", H::fg(p.red).bold()),
            ("ErrorMsg", H::fg_bg(p.bg0, p.red).bold()),
            ("Title", H::fg(p.green).bold()),
            ("Directory", H::fg(p.green).bold()),
            ("Conceal", H::fg(p.blue)),
            ("SpellBad", H::undercurl(p.red)),
            ("SpellCap", H::undercurl(p.blue)),
            ("SpellRare", H::undercurl(p.purple)),
            ("SpellLocal", H::undercurl(p.aqua)),
            ("DiffAdd", H::fg_bg(p.green, p.bg1)),
            ("DiffChange", H::fg_bg(p.aqua, p.bg1)),
            ("DiffDelete", H::fg_bg(p.red, p.bg1)),
            ("DiffText", H::fg_bg(p.yellow, p.bg1)),
            // Syntax.
            ("Comment", H::fg(p.gray).italic()),
            ("Constant", H::fg(p.purple)),
            ("String", H::fg(p.green)),
            ("Character", H::fg(p.purple)),
            ("Number", H::fg(p.purple)),
            ("Boolean", H::fg(p.purple)),
            ("Float", H::fg(p.purple)),
            ("Identifier", H::fg(p.blue)),
            ("Function", H::fg(p.green).bold()),
            ("Statement", H::fg(p.red)),
            ("Keyword", H::fg(p.red)),
            ("Conditional", H::fg(p.red)),
            ("Repeat", H::fg(p.red)),
            ("Label", H::fg(p.red)),
            ("Exception", H::fg(p.red)),
            ("Operator", H::fg(p.fg1)),
            ("PreProc", H::fg(p.aqua)),
            ("Include", H::fg(p.aqua)),
            ("Define", H::fg(p.aqua)),
            ("Macro", H::fg(p.aqua)),
            ("Type", H::fg(p.yellow)),
            ("StorageClass", H::fg(p.orange)),
            ("Structure", H::fg(p.aqua)),
            ("Typedef", H::fg(p.yellow)),
            ("Special", H::fg(p.orange)),
            ("SpecialChar", H::fg(p.red)),
            ("Tag", H::fg(p.aqua)),
            ("Delimiter", H::fg(p.orange)),
            ("Underlined", H::fg(p.blue).underline()),
            ("Error", H::fg(p.red).bold()),
            ("Todo", H::fg(p.fg0).bold().italic()),
            // Diagnostics.
            ("DiagnosticError", H::fg(p.red)),
            ("DiagnosticWarn", H::fg(p.yellow)),
            ("DiagnosticInfo", H::fg(p.blue)),
            ("DiagnosticHint", H::fg(p.aqua)),
            ("DiagnosticOk", H::fg(p.green)),
            ("DiagnosticUnderlineError", H::undercurl(p.red)),
            ("DiagnosticUnderlineWarn", H::undercurl(p.yellow)),
            ("DiagnosticUnderlineInfo", H::undercurl(p.blue)),
            ("DiagnosticUnderlineHint", H::undercurl(p.aqua)),
            // Markdown (regex syntax).
            ("markdownH1", H::fg(p.orange).bold()),
            ("markdownH2", H::fg(p.yellow).bold()),
            ("markdownH3", H::fg(p.green).bold()),
            ("markdownH4", H::fg(p.aqua).bold()),
            ("markdownH5", H::fg(p.blue).bold()),
            ("markdownH6", H::fg(p.purple).bold()),
            ("markdownHeadingDelimiter", H::fg(p.gray)),
            ("markdownHeadingRule", H::fg(p.gray)),
            ("markdownRule", H::fg(p.gray)),
            ("markdownCode", H::fg(p.aqua)),
            ("markdownCodeBlock", H::fg(p.aqua)),
            ("markdownCodeDelimiter", H::fg(p.gray)),
            ("markdownBlockquote", H::fg(p.gray)),
            ("markdownListMarker", H::fg(p.orange)),
            ("markdownOrderedListMarker", H::fg(p.orange)),
            ("markdownLinkText", H::fg(p.blue).underline()),
            ("markdownLinkDelimiter", H::fg(p.gray)),
            ("markdownLinkTextDelimiter", H::fg(p.gray)),
            ("markdownUrl", H::fg(p.purple).underline()),
            ("markdownItalic", H::fg(p.fg2).italic()),
            ("markdownBold", H::fg(p.fg0).bold()),
            ("markdownBoldItalic", H::fg(p.fg0).bold().italic()),
            // Treesitter captures used by the markdown parsers.
            ("@markup.heading.1", H::link("markdownH1")),
            ("@markup.heading.2", H::link("markdownH2")),
            ("@markup.heading.3", H::link("markdownH3")),
            ("@markup.heading.4", H::link("markdownH4")),
            ("@markup.heading.5", H::link("markdownH5")),
            ("@markup.heading.6", H::link("markdownH6")),
            ("@markup.strong", H::link("markdownBold")),
            ("@markup.italic", H::link("markdownItalic")),
            (
                "@markup.strikethrough",
                H {
                    fg: Some(p.gray),
                    strikethrough: true,
                    ..H::default()
                },
            ),
            ("@markup.underline", H::default().underline()),
            ("@markup.quote", H::fg(p.gray).italic()),
            ("@markup.math", H::fg(p.aqua)),
            ("@markup.link", H::fg(p.blue)),
            ("@markup.link.label", H::link("markdownLinkText")),
            ("@markup.link.url", H::link("markdownUrl")),
            ("@markup.raw", H::fg(p.aqua)),
            ("@markup.raw.block", H::fg(p.aqua)),
            ("@markup.list", H::fg(p.orange)),
            ("@markup.list.checked", H::fg(p.green)),
            ("@markup.list.unchecked", H::fg(p.gray)),
            ("@punctuation.special", H::fg(p.gray)),
            ("@punctuation.delimiter", H::fg(p.gray)),
            ("@label", H::fg(p.aqua)),
        ];
        groups.shrink_to_fit();
        groups
    }

    /// CSS for the GTK window chrome (F5.R2).
    pub fn gtk_css(&self) -> String {
        let p = self;
        format!(
            "window, window.background {{ background-color: {bg0}; color: {fg1}; }}\n\
             headerbar {{ background: {bg1}; color: {fg1}; box-shadow: none; border-bottom: 1px solid {bg2}; }}\n\
             headerbar button {{ color: {fg1}; }}\n\
             paned > separator {{ background: {bg2}; min-width: 1px; min-height: 1px; }}\n\
             popover > contents, popover > arrow {{ background: {bg1}; color: {fg1}; }}\n\
             window.dialog, .message-dialog {{ background: {bg1}; color: {fg1}; }}\n\
             .dim-label {{ color: {gray}; }}\n",
            bg0 = p.bg0,
            bg1 = p.bg1,
            bg2 = p.bg2,
            fg1 = p.fg1,
            gray = p.gray,
        )
    }

    /// The preview's colour custom properties (see `preview.css`).
    pub fn preview_css(&self, dark: bool) -> String {
        let p = self;
        format!(
            ":root {{ --bg: {}; --bg1: {}; --bg2: {}; --fg: {}; --fg-strong: {}; \
             --muted: {}; --red: {}; --green: {}; --yellow: {}; --blue: {}; \
             --purple: {}; --aqua: {}; --orange: {}; color-scheme: {}; }}",
            p.bg0,
            p.bg1,
            p.bg2,
            p.fg1,
            p.fg0,
            p.gray,
            p.red,
            p.green,
            p.yellow,
            p.blue,
            p.purple,
            p.aqua,
            p.orange,
            if dark { "dark" } else { "light" },
        )
    }
}

/// Lua run with `nvim_exec_lua(code, [groups, background])` to apply a
/// variant: it resets highlights and defines every group in one go.
pub const APPLY_HIGHLIGHTS_LUA: &str = "\
local groups, background = ...
vim.g.colors_name = nil
vim.o.background = background
vim.cmd('highlight clear')
for name, spec in pairs(groups) do
  vim.api.nvim_set_hl(0, name, spec)
end
vim.g.colors_name = 'ringmd-gruvbox'
";

/// The `nvim_exec_lua` arguments that apply `variant`.
pub fn apply_highlights_args(variant: Variant) -> Vec<Value> {
    let groups = variant
        .palette()
        .highlights()
        .into_iter()
        .map(|(name, hl)| (Value::from(name), hl.to_value()))
        .collect();
    let background = if variant.is_dark() { "dark" } else { "light" };
    vec![
        APPLY_HIGHLIGHTS_LUA.into(),
        Value::Array(vec![Value::Map(groups), background.into()]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for variant in Variant::ALL {
            assert_eq!(variant.name().parse(), Ok(variant));
            assert_eq!(Variant::new(variant.is_dark(), variant.contrast()), variant);
        }
        assert_eq!(Variant::default().name(), "dark-medium");
        assert_eq!(
            "gruvbox".parse::<Variant>(),
            Err(UnknownVariant("gruvbox".into()))
        );
    }

    #[test]
    fn theme_choices() {
        for name in ThemeChoice::names() {
            let choice: ThemeChoice = name.parse().unwrap();
            assert_eq!(choice.name(), name);
        }
        assert_eq!(
            "system-medium".parse::<ThemeChoice>().unwrap().name(),
            "system"
        );
        assert!("system-bright".parse::<ThemeChoice>().is_err());
        let soft = ThemeChoice::System(Contrast::Soft);
        assert_eq!(soft.resolve(true), Variant::DarkSoft);
        assert_eq!(soft.resolve(false), Variant::LightSoft);
        assert_eq!(
            ThemeChoice::Fixed(Variant::LightHard).resolve(true),
            Variant::LightHard
        );
    }

    /// F5-AC3: hex values match Appendix A.
    #[test]
    fn palettes_match_the_reference() {
        let bg0: Vec<String> = Variant::ALL
            .iter()
            .map(|v| v.palette().bg0.to_string())
            .collect();
        assert_eq!(
            bg0,
            [
                "#1d2021", "#282828", "#32302f", "#f9f5d7", "#fbf1c7", "#f2e5bc"
            ]
        );
        let dark = Variant::DarkMedium.palette();
        let dark_colors = [
            dark.bg1,
            dark.bg2,
            dark.bg3,
            dark.bg4,
            dark.fg0,
            dark.fg1,
            dark.fg2,
            dark.gray,
            dark.red,
            dark.green,
            dark.yellow,
            dark.blue,
            dark.purple,
            dark.aqua,
            dark.orange,
        ]
        .map(|c| c.to_string());
        assert_eq!(
            dark_colors,
            [
                "#3c3836", "#504945", "#665c54", "#7c6f64", "#fbf1c7", "#ebdbb2", "#d5c4a1",
                "#928374", "#fb4934", "#b8bb26", "#fabd2f", "#83a598", "#d3869b", "#8ec07c",
                "#fe8019",
            ]
        );
        let light = Variant::LightSoft.palette();
        let light_colors = [
            light.bg1,
            light.bg2,
            light.bg3,
            light.bg4,
            light.fg0,
            light.fg1,
            light.fg2,
            light.gray,
            light.red,
            light.green,
            light.yellow,
            light.blue,
            light.purple,
            light.aqua,
            light.orange,
        ]
        .map(|c| c.to_string());
        assert_eq!(
            light_colors,
            [
                "#ebdbb2", "#d5c4a1", "#bdae93", "#a89984", "#282828", "#3c3836", "#504945",
                "#928374", "#9d0006", "#79740e", "#b57614", "#076678", "#8f3f71", "#427b58",
                "#af3a03",
            ]
        );
    }

    #[test]
    fn highlights_cover_ui_and_markdown() {
        let groups = Variant::DarkMedium.palette().highlights();
        let names: Vec<&str> = groups.iter().map(|(n, _)| *n).collect();
        for required in [
            "Normal",
            "StatusLine",
            "Visual",
            "Comment",
            "markdownH1",
            "markdownCode",
            "@markup.heading.1",
            "@markup.link.url",
            "@markup.raw.block",
            "@markup.quote",
            "@markup.list",
        ] {
            assert!(names.contains(&required), "missing {required}");
        }
        let unique: std::collections::HashSet<_> = names.iter().collect();
        assert_eq!(unique.len(), names.len(), "duplicate group");
    }

    #[test]
    fn highlight_values() {
        let normal = Variant::DarkMedium.palette().highlights()[0].1.to_value();
        assert_eq!(
            normal,
            Value::Map(vec![
                ("fg".into(), "#ebdbb2".into()),
                ("bg".into(), "#282828".into()),
            ])
        );
        assert_eq!(
            Highlight::link("Normal").to_value(),
            Value::Map(vec![("link".into(), "Normal".into())])
        );
        let args = apply_highlights_args(Variant::LightSoft);
        assert_eq!(args[0], Value::from(APPLY_HIGHLIGHTS_LUA));
        assert_eq!(args[1].as_array().unwrap()[1], Value::from("light"));
    }

    #[test]
    fn css_outputs_use_the_palette() {
        let p = Variant::LightHard.palette();
        assert!(p.gtk_css().contains("background-color: #f9f5d7"));
        let css = p.preview_css(false);
        assert!(css.contains("--bg: #f9f5d7"), "{css}");
        assert!(css.contains("--blue: #076678"), "{css}");
        assert!(css.contains("color-scheme: light"), "{css}");
    }
}
