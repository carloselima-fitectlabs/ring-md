//! Converting between Neovim register contents and clipboard text (T1.9).
//!
//! Neovim's clipboard provider passes registers as a list of lines plus a
//! register type: `"v"` (characterwise), `"V"` (linewise) or `"b"` (block).

/// Register contents as Neovim sees them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Register {
    pub lines: Vec<String>,
    pub regtype: String,
}

/// Text to put on the system clipboard. Neovim already appends an empty
/// last line to linewise registers, so they end with a newline.
pub fn to_clipboard_text(register: &Register) -> String {
    register.lines.join("\n")
}

/// Register contents for clipboard text. Text ending in a newline pastes
/// linewise, like it does in terminal Neovim (which drops the empty last
/// line again).
pub fn from_clipboard_text(text: &str) -> Register {
    let text = text.replace("\r\n", "\n");
    Register {
        lines: text.split('\n').map(str::to_owned).collect(),
        regtype: if text.ends_with('\n') { "V" } else { "v" }.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reg(lines: &[&str], regtype: &str) -> Register {
        Register {
            lines: lines.iter().map(|l| (*l).to_owned()).collect(),
            regtype: regtype.into(),
        }
    }

    #[test]
    fn linewise_roundtrip() {
        let register = reg(&["# Title", "", "text", ""], "V");
        let text = to_clipboard_text(&register);
        assert_eq!(text, "# Title\n\ntext\n");
        assert_eq!(from_clipboard_text(&text), register);
    }

    #[test]
    fn charwise_roundtrip() {
        let register = reg(&["hello", "wor"], "v");
        let text = to_clipboard_text(&register);
        assert_eq!(text, "hello\nwor");
        assert_eq!(from_clipboard_text(&text), register);
    }

    #[test]
    fn windows_line_endings() {
        assert_eq!(from_clipboard_text("a\r\nb\r\n"), reg(&["a", "b", ""], "V"));
    }

    #[test]
    fn empty_text() {
        assert_eq!(from_clipboard_text(""), reg(&[""], "v"));
    }
}
