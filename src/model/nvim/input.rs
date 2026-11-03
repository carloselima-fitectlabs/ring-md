//! Translating GUI input into Neovim's key and mouse notation (F2.R4, F2.R5).
//!
//! The View reports keys as toolkit-neutral [`KeyPress`] values (the GDK key
//! name plus its character), so this mapping is plain, testable Rust.

/// Modifier keys held during an input event.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    /// Super / Windows / Command key.
    pub logo: bool,
}

impl Modifiers {
    pub const NONE: Self = Self {
        shift: false,
        ctrl: false,
        alt: false,
        logo: false,
    };

    pub const CTRL: Self = Self {
        ctrl: true,
        ..Self::NONE
    };

    /// `C-S-` style prefix for keys. `shift` is left out when the character
    /// already reflects it.
    fn key_prefix(self, include_shift: bool) -> String {
        let mut prefix = String::new();
        if self.ctrl {
            prefix.push_str("C-");
        }
        if include_shift && self.shift {
            prefix.push_str("S-");
        }
        if self.alt {
            prefix.push_str("A-");
        }
        if self.logo {
            prefix.push_str("D-");
        }
        prefix
    }

    /// Modifier string for `nvim_input_mouse`, e.g. `"CS"`.
    pub fn mouse_prefix(self) -> String {
        let mut prefix = String::new();
        if self.ctrl {
            prefix.push('C');
        }
        if self.shift {
            prefix.push('S');
        }
        if self.alt {
            prefix.push('A');
        }
        if self.logo {
            prefix.push('D');
        }
        prefix
    }
}

/// A key press as reported by the View.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyPress {
    /// Toolkit key name (GDK keyval name), e.g. `"Return"`, `"a"`, `"F5"`.
    pub name: String,
    /// The character the key produces with the current modifiers, if any.
    pub ch: Option<char>,
    pub modifiers: Modifiers,
}

impl KeyPress {
    pub fn new(name: &str, ch: Option<char>, modifiers: Modifiers) -> Self {
        Self {
            name: name.to_owned(),
            ch,
            modifiers,
        }
    }
}

/// Neovim's name for non-printing keys, keyed by GDK key name.
fn special_key(name: &str) -> Option<&'static str> {
    Some(match name {
        "Return" | "KP_Enter" | "ISO_Enter" => "CR",
        "Escape" => "Esc",
        "BackSpace" => "BS",
        "Tab" | "ISO_Left_Tab" | "KP_Tab" => "Tab",
        "Delete" | "KP_Delete" => "Del",
        "Insert" | "KP_Insert" => "Insert",
        "Home" | "KP_Home" => "Home",
        "End" | "KP_End" => "End",
        "Page_Up" | "KP_Page_Up" | "KP_Prior" => "PageUp",
        "Page_Down" | "KP_Page_Down" | "KP_Next" => "PageDown",
        "Up" | "KP_Up" => "Up",
        "Down" | "KP_Down" => "Down",
        "Left" | "KP_Left" => "Left",
        "Right" | "KP_Right" => "Right",
        "space" | "KP_Space" => "Space",
        "Help" => "Help",
        "Undo" => "Undo",
        "F1" => "F1",
        "F2" => "F2",
        "F3" => "F3",
        "F4" => "F4",
        "F5" => "F5",
        "F6" => "F6",
        "F7" => "F7",
        "F8" => "F8",
        "F9" => "F9",
        "F10" => "F10",
        "F11" => "F11",
        "F12" => "F12",
        _ => return None,
    })
}

/// Neovim's name for printable characters that need escaping in `<…>`.
fn char_name(ch: char) -> Option<&'static str> {
    Some(match ch {
        '<' => "lt",
        '\\' => "Bslash",
        '|' => "Bar",
        _ => return None,
    })
}

/// Encode a key press for `nvim_input`. Returns `None` for keys that do
/// nothing on their own (bare modifiers, dead keys).
pub fn encode_key(key: &KeyPress) -> Option<String> {
    let mods = key.modifiers;
    if let Some(special) = special_key(&key.name) {
        // `ISO_Left_Tab` is how GDK reports Shift+Tab.
        let mods = if key.name == "ISO_Left_Tab" {
            Modifiers {
                shift: true,
                ..mods
            }
        } else {
            mods
        };
        let prefix = mods.key_prefix(true);
        if prefix.is_empty() && special == "Space" {
            return Some(" ".into());
        }
        return Some(format!("<{prefix}{special}>"));
    }

    let ch = key.ch.filter(|c| !c.is_control())?;
    let prefix = mods.key_prefix(false);
    if prefix.is_empty() {
        return Some(escape_text(&ch.to_string()));
    }
    let name = char_name(ch).map_or_else(|| ch.to_string(), str::to_owned);
    Some(format!("<{prefix}{name}>"))
}

/// Escape literal text (e.g. committed by an input method) for `nvim_input`.
pub fn escape_text(text: &str) -> String {
    text.replace('<', "<lt>")
}

/// A GUI shortcut the editor handles itself instead of sending to Neovim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shortcut {
    ZoomIn,
    ZoomOut,
    ZoomReset,
}

/// GUI shortcuts intercepted in every mode (§5): `Ctrl+=`, `Ctrl+-`, `Ctrl+0`.
pub fn shortcut(key: &KeyPress) -> Option<Shortcut> {
    let mods = key.modifiers;
    if !mods.ctrl || mods.alt || mods.logo {
        return None;
    }
    match key.name.as_str() {
        "equal" | "plus" | "KP_Add" => Some(Shortcut::ZoomIn),
        "minus" | "underscore" | "KP_Subtract" => Some(Shortcut::ZoomOut),
        "0" | "KP_0" => Some(Shortcut::ZoomReset),
        _ => None,
    }
}

/// Mouse buttons understood by `nvim_input_mouse`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    Wheel,
    /// Pointer motion with no button held.
    Move,
}

impl MouseButton {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Middle => "middle",
            Self::Right => "right",
            Self::Wheel => "wheel",
            Self::Move => "move",
        }
    }
}

/// Mouse actions understood by `nvim_input_mouse`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseAction {
    Press,
    Drag,
    Release,
    WheelUp,
    WheelDown,
    WheelLeft,
    WheelRight,
}

impl MouseAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Press => "press",
            Self::Drag => "drag",
            Self::Release => "release",
            Self::WheelUp => "up",
            Self::WheelDown => "down",
            Self::WheelLeft => "left",
            Self::WheelRight => "right",
        }
    }
}

/// A mouse event at a grid cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseEvent {
    pub button: MouseButton,
    pub action: MouseAction,
    pub modifiers: Modifiers,
    pub row: u32,
    pub col: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(name: &str, ch: Option<char>, modifiers: Modifiers) -> Option<String> {
        encode_key(&KeyPress::new(name, ch, modifiers))
    }

    const SHIFT: Modifiers = Modifiers {
        shift: true,
        ..Modifiers::NONE
    };
    const ALT: Modifiers = Modifiers {
        alt: true,
        ..Modifiers::NONE
    };

    #[test]
    fn printable_characters() {
        assert_eq!(key("a", Some('a'), Modifiers::NONE).as_deref(), Some("a"));
        assert_eq!(key("A", Some('A'), SHIFT).as_deref(), Some("A"));
        assert_eq!(key("less", Some('<'), SHIFT).as_deref(), Some("<lt>"));
        assert_eq!(
            key("eacute", Some('é'), Modifiers::NONE).as_deref(),
            Some("é")
        );
        assert_eq!(
            key("space", Some(' '), Modifiers::NONE).as_deref(),
            Some(" ")
        );
    }

    #[test]
    fn special_keys() {
        assert_eq!(
            key("Return", Some('\r'), Modifiers::NONE).as_deref(),
            Some("<CR>")
        );
        assert_eq!(
            key("Escape", None, Modifiers::NONE).as_deref(),
            Some("<Esc>")
        );
        assert_eq!(
            key("BackSpace", None, Modifiers::NONE).as_deref(),
            Some("<BS>")
        );
        assert_eq!(key("ISO_Left_Tab", None, SHIFT).as_deref(), Some("<S-Tab>"));
        assert_eq!(key("Up", None, SHIFT).as_deref(), Some("<S-Up>"));
        assert_eq!(key("F5", None, Modifiers::CTRL).as_deref(), Some("<C-F5>"));
        assert_eq!(
            key("space", Some(' '), Modifiers::CTRL).as_deref(),
            Some("<C-Space>")
        );
        assert_eq!(
            key("KP_Enter", None, Modifiers::NONE).as_deref(),
            Some("<CR>")
        );
    }

    #[test]
    fn modified_characters() {
        assert_eq!(
            key("r", Some('r'), Modifiers::CTRL).as_deref(),
            Some("<C-r>")
        );
        assert_eq!(key("x", Some('x'), ALT).as_deref(), Some("<A-x>"));
        assert_eq!(
            key("less", Some('<'), Modifiers::CTRL).as_deref(),
            Some("<C-lt>")
        );
        assert_eq!(
            key("bracketright", Some(']'), Modifiers::CTRL).as_deref(),
            Some("<C-]>")
        );
    }

    #[test]
    fn modifier_only_and_dead_keys_send_nothing() {
        assert_eq!(key("Shift_L", None, SHIFT), None);
        assert_eq!(key("Control_L", None, Modifiers::CTRL), None);
        assert_eq!(key("dead_acute", None, Modifiers::NONE), None);
    }

    #[test]
    fn escapes_committed_text() {
        assert_eq!(escape_text("a<b>"), "a<lt>b>");
    }

    #[test]
    fn zoom_shortcuts() {
        let ctrl = |name| shortcut(&KeyPress::new(name, None, Modifiers::CTRL));
        assert_eq!(ctrl("equal"), Some(Shortcut::ZoomIn));
        assert_eq!(ctrl("plus"), Some(Shortcut::ZoomIn));
        assert_eq!(ctrl("minus"), Some(Shortcut::ZoomOut));
        assert_eq!(ctrl("0"), Some(Shortcut::ZoomReset));
        assert_eq!(ctrl("r"), None);
        assert_eq!(
            shortcut(&KeyPress::new("equal", Some('='), Modifiers::NONE)),
            None
        );
    }

    #[test]
    fn mouse_modifiers() {
        let mods = Modifiers {
            ctrl: true,
            shift: true,
            ..Modifiers::NONE
        };
        assert_eq!(mods.mouse_prefix(), "CS");
        assert_eq!(Modifiers::NONE.mouse_prefix(), "");
    }
}
