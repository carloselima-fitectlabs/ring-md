//! `config.toml` (F9, T6.1): loading with per-key validation, and saving
//! single settings without disturbing the rest of the file.
//!
//! Every key is optional. An invalid value is reported as a warning and
//! replaced by its default, so a broken config never stops the app
//! (F9.R3). Saving edits only the changed key, keeping the user's comments
//! and layout (F9.R4).

use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item, Table, value};

use super::theme::ThemeChoice;

/// Paper sizes supported by PDF export (F6.R3).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PageSize {
    #[default]
    A4,
    Letter,
}

/// All settings, with defaults for anything missing or invalid.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub theme: ThemeChoice,
    pub font: String,
    pub font_size: f64,
    pub preview_visible: bool,
    pub preview_debounce_ms: u32,
    pub preview_sync_scroll: bool,
    pub pdf_page_size: PageSize,
    pub pdf_margin_mm: f64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: ThemeChoice::default(),
            font: "Monospace".into(),
            font_size: 12.0,
            preview_visible: true,
            preview_debounce_ms: 50,
            preview_sync_scroll: true,
            pdf_page_size: PageSize::A4,
            pdf_margin_mm: 20.0,
        }
    }
}

/// A setting changed from inside the app that should persist (F9.R4).
#[derive(Debug, Clone, PartialEq)]
pub enum Setting {
    Theme(ThemeChoice),
    FontSize(f64),
    PreviewVisible(bool),
}

/// Errors from reading or writing the config file.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not write {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path} is not valid TOML: {message}")]
    Syntax { path: PathBuf, message: String },
}

/// `$XDG_CONFIG_HOME/ringmd/config.toml` (F9.R1), or `~/.config/...`.
pub fn default_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("ringmd").join("config.toml")
}

impl Config {
    /// Load `path`. A missing file gives the defaults; warnings list every
    /// value that was ignored.
    pub fn load(path: &Path) -> (Self, Vec<String>) {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Self::default(), Vec::new()),
            Err(source) => {
                let error = ConfigError::Read {
                    path: path.into(),
                    source,
                };
                (Self::default(), vec![error.to_string()])
            }
        }
    }

    /// Parse config text, falling back to defaults key by key.
    pub fn parse(text: &str) -> (Self, Vec<String>) {
        let mut config = Self::default();
        let mut warnings = Vec::new();
        let doc = match text.parse::<DocumentMut>() {
            Ok(doc) => doc,
            Err(e) => {
                warnings.push(format!("config is not valid TOML, using defaults: {e}"));
                return (config, warnings);
            }
        };
        let mut reader = Reader {
            warnings: &mut warnings,
        };
        let root = doc.as_table();
        for (key, item) in root.iter() {
            match key {
                "theme" => {
                    if let Some(theme) = reader.parse_str(key, item, |s| s.parse().ok()) {
                        config.theme = theme;
                    }
                }
                "font" => {
                    if let Some(font) =
                        reader.parse_str(key, item, |s| (!s.trim().is_empty()).then(|| s.into()))
                    {
                        config.font = font;
                    }
                }
                "font_size" => {
                    if let Some(size) = reader.number(key, item, 6.0..=72.0) {
                        config.font_size = size;
                    }
                }
                "preview" => {
                    for (sub, item) in reader.table(key, item) {
                        let name = format!("preview.{sub}");
                        match sub {
                            "visible" => {
                                if let Some(b) = reader.boolean(&name, item) {
                                    config.preview_visible = b;
                                }
                            }
                            "debounce_ms" => {
                                if let Some(ms) = reader.number(&name, item, 0.0..=2000.0) {
                                    config.preview_debounce_ms = ms as u32;
                                }
                            }
                            "sync_scroll" => {
                                if let Some(b) = reader.boolean(&name, item) {
                                    config.preview_sync_scroll = b;
                                }
                            }
                            _ => reader.unknown(&name),
                        }
                    }
                }
                "pdf" => {
                    for (sub, item) in reader.table(key, item) {
                        let name = format!("pdf.{sub}");
                        match sub {
                            "page_size" => {
                                if let Some(size) = reader.parse_str(&name, item, parse_page_size) {
                                    config.pdf_page_size = size;
                                }
                            }
                            "margin_mm" => {
                                if let Some(mm) = reader.number(&name, item, 0.0..=50.0) {
                                    config.pdf_margin_mm = mm;
                                }
                            }
                            _ => reader.unknown(&name),
                        }
                    }
                }
                _ => reader.unknown(key),
            }
        }
        (config, warnings)
    }
}

fn parse_page_size(s: &str) -> Option<PageSize> {
    match s.to_ascii_lowercase().as_str() {
        "a4" => Some(PageSize::A4),
        "letter" => Some(PageSize::Letter),
        _ => None,
    }
}

/// Reads typed values, recording a warning for each bad one.
struct Reader<'a> {
    warnings: &'a mut Vec<String>,
}

impl Reader<'_> {
    fn invalid(&mut self, key: &str, item: &Item) {
        let shown = item.to_string();
        self.warnings.push(format!(
            "ignoring invalid value for `{key}`: {}",
            shown.trim()
        ));
    }

    fn unknown(&mut self, key: &str) {
        self.warnings
            .push(format!("ignoring unknown setting `{key}`"));
    }

    fn parse_str<T>(
        &mut self,
        key: &str,
        item: &Item,
        parse: impl Fn(&str) -> Option<T>,
    ) -> Option<T> {
        let parsed = item.as_str().and_then(parse);
        if parsed.is_none() {
            self.invalid(key, item);
        }
        parsed
    }

    fn number(
        &mut self,
        key: &str,
        item: &Item,
        range: std::ops::RangeInclusive<f64>,
    ) -> Option<f64> {
        let n = item
            .as_float()
            .or_else(|| item.as_integer().map(|i| i as f64))
            .filter(|n| range.contains(n));
        if n.is_none() {
            self.invalid(key, item);
        }
        n
    }

    fn boolean(&mut self, key: &str, item: &Item) -> Option<bool> {
        let b = item.as_bool();
        if b.is_none() {
            self.invalid(key, item);
        }
        b
    }

    fn table<'i>(&mut self, key: &str, item: &'i Item) -> Vec<(&'i str, &'i Item)> {
        match item.as_table_like() {
            Some(table) => table.iter().collect(),
            None => {
                self.invalid(key, item);
                Vec::new()
            }
        }
    }
}

/// Write one setting into the config file at `path`, creating it (and its
/// directory) if needed and keeping everything else as it was.
pub fn save_setting(path: &Path, setting: &Setting) -> Result<(), ConfigError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(source) => {
            return Err(ConfigError::Read {
                path: path.into(),
                source,
            });
        }
    };
    let mut doc = text
        .parse::<DocumentMut>()
        .map_err(|e| ConfigError::Syntax {
            path: path.into(),
            message: e.to_string(),
        })?;
    apply_setting(&mut doc, setting);

    let write_err = |source| ConfigError::Write {
        path: path.into(),
        source,
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(write_err)?;
    }
    // Write a sibling file and rename it, so a crash never leaves a
    // half-written config.
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, doc.to_string()).map_err(write_err)?;
    std::fs::rename(&tmp, path).map_err(write_err)
}

fn apply_setting(doc: &mut DocumentMut, setting: &Setting) {
    match setting {
        Setting::Theme(choice) => doc["theme"] = value(choice.name()),
        Setting::FontSize(size) => doc["font_size"] = value(*size),
        Setting::PreviewVisible(visible) => {
            if !doc.contains_table("preview") {
                doc["preview"] = Item::Table(Table::new());
            }
            doc["preview"]["visible"] = value(*visible);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::theme::{Contrast, Variant};

    #[test]
    fn empty_config_is_all_defaults() {
        let (config, warnings) = Config::parse("");
        assert_eq!(config, Config::default());
        assert!(warnings.is_empty());
    }

    #[test]
    fn every_key_is_read() {
        let (config, warnings) = Config::parse(
            r#"
            theme = "system-soft"
            font = "Iosevka"
            font_size = 14
            [preview]
            visible = false
            debounce_ms = 120
            sync_scroll = false
            [pdf]
            page_size = "Letter"
            margin_mm = 15.5
            "#,
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            config,
            Config {
                theme: ThemeChoice::System(Contrast::Soft),
                font: "Iosevka".into(),
                font_size: 14.0,
                preview_visible: false,
                preview_debounce_ms: 120,
                preview_sync_scroll: false,
                pdf_page_size: PageSize::Letter,
                pdf_margin_mm: 15.5,
            }
        );
    }

    /// F9-AC2: an invalid theme falls back to dark-medium with a warning.
    #[test]
    fn invalid_values_fall_back_with_warnings() {
        let (config, warnings) = Config::parse(
            r#"
            theme = "purple"
            font_size = 500
            colour = "x"
            preview = 3
            [pdf]
            margin_mm = "wide"
            "#,
        );
        assert_eq!(config, Config::default());
        assert_eq!(warnings.len(), 5, "{warnings:?}");
        assert!(warnings[0].contains("`theme`"), "{warnings:?}");
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("unknown setting `colour`"))
        );
        assert_eq!(config.theme, ThemeChoice::Fixed(Variant::DarkMedium));
    }

    #[test]
    fn broken_toml_gives_defaults() {
        let (config, warnings) = Config::parse("theme = ");
        assert_eq!(config, Config::default());
        assert_eq!(warnings.len(), 1);
    }

    fn temp_path(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ringmd-config-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("ringmd").join("config.toml")
    }

    /// F9-AC1: no file means defaults.
    #[test]
    fn missing_file_is_defaults() {
        let path = temp_path("missing");
        assert_eq!(Config::load(&path), (Config::default(), Vec::new()));
    }

    #[test]
    fn saving_keeps_comments_and_other_keys() {
        let path = temp_path("save");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "# my settings\nfont = \"Iosevka\" # nice\n").unwrap();

        save_setting(
            &path,
            &Setting::Theme(ThemeChoice::Fixed(Variant::LightSoft)),
        )
        .unwrap();
        save_setting(&path, &Setting::PreviewVisible(false)).unwrap();
        save_setting(&path, &Setting::FontSize(13.0)).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.starts_with("# my settings\nfont = \"Iosevka\" # nice\n"),
            "{text}"
        );
        let (config, warnings) = Config::load(&path);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(config.theme, ThemeChoice::Fixed(Variant::LightSoft));
        assert!(!config.preview_visible);
        assert_eq!(config.font_size, 13.0);
        assert_eq!(config.font, "Iosevka");
        std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap()).unwrap();
    }

    #[test]
    fn saving_creates_the_file() {
        let path = temp_path("create");
        save_setting(&path, &Setting::Theme(ThemeChoice::System(Contrast::Hard))).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "theme = \"system-hard\"\n"
        );
        std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap()).unwrap();
    }
}
