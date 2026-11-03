//! Command-line interface: `ringmd [FILE]` (F1.R2).

use std::path::PathBuf;

use clap::Parser;

#[derive(Debug, Clone, PartialEq, Eq, Parser)]
#[command(name = "ringmd", version, about)]
pub struct Cli {
    /// Markdown file to open. It is created on first save if it does not exist.
    pub file: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_arguments_opens_unnamed_buffer() {
        assert_eq!(Cli::try_parse_from(["ringmd"]).unwrap().file, None);
    }

    #[test]
    fn file_argument() {
        let cli = Cli::try_parse_from(["ringmd", "notes.md"]).unwrap();
        assert_eq!(cli.file, Some(PathBuf::from("notes.md")));
    }

    #[test]
    fn rejects_extra_arguments() {
        assert!(Cli::try_parse_from(["ringmd", "a.md", "b.md"]).is_err());
    }

    #[test]
    fn clap_definition_is_valid() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
