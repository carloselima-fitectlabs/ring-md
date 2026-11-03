//! Commands the bundled config sends to the GUI with `rpcnotify` when the
//! user runs `:Preview`, `:ExportPdf` or `:Theme` (F8.R2, rule A5).

use std::path::PathBuf;

use rmpv::Value;

use crate::model::theme::ThemeChoice;

/// A GUI command issued from Neovim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuiCommand {
    /// `:Preview`
    TogglePreview,
    /// `:ExportPdf [path]`; `None` means "use the default path".
    ExportPdf { path: Option<PathBuf> },
    /// `:Theme <variant>` or `:Theme system[-contrast]`
    SetTheme(ThemeChoice),
}

impl GuiCommand {
    /// Decode a notification. Returns `None` for methods that are not GUI
    /// commands or carry malformed arguments.
    pub fn from_notification(method: &str, params: &[Value]) -> Option<Self> {
        match method {
            "ringmd_preview" => Some(Self::TogglePreview),
            "ringmd_export_pdf" => {
                let path = match params.first() {
                    None | Some(Value::Nil) => None,
                    Some(value) => Some(PathBuf::from(value.as_str()?)),
                };
                Some(Self::ExportPdf { path })
            }
            "ringmd_theme" => params.first()?.as_str()?.parse().ok().map(Self::SetTheme),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_commands() {
        assert_eq!(
            GuiCommand::from_notification("ringmd_preview", &[]),
            Some(GuiCommand::TogglePreview)
        );
        assert_eq!(
            GuiCommand::from_notification("ringmd_export_pdf", &[Value::Nil]),
            Some(GuiCommand::ExportPdf { path: None })
        );
        assert_eq!(
            GuiCommand::from_notification("ringmd_export_pdf", &["/tmp/a.pdf".into()]),
            Some(GuiCommand::ExportPdf {
                path: Some("/tmp/a.pdf".into())
            })
        );
        assert_eq!(
            GuiCommand::from_notification("ringmd_theme", &["light-soft".into()]),
            Some(GuiCommand::SetTheme(ThemeChoice::Fixed(
                crate::model::theme::Variant::LightSoft
            )))
        );
    }

    #[test]
    fn decodes_system_theme() {
        assert_eq!(
            GuiCommand::from_notification("ringmd_theme", &["system".into()]),
            Some(GuiCommand::SetTheme(ThemeChoice::System(
                crate::model::theme::Contrast::Medium
            )))
        );
    }

    #[test]
    fn rejects_other_and_malformed() {
        assert_eq!(GuiCommand::from_notification("redraw", &[]), None);
        assert_eq!(GuiCommand::from_notification("ringmd_theme", &[]), None);
        assert_eq!(
            GuiCommand::from_notification("ringmd_theme", &["nope".into()]),
            None
        );
        assert_eq!(
            GuiCommand::from_notification("ringmd_export_pdf", &[3.into()]),
            None
        );
    }
}
