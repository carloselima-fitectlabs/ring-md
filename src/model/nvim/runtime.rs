//! Locating RingMD's bundled Neovim runtime (`runtime/init.lua`).

use std::env;
use std::path::{Path, PathBuf};

/// Environment variable that overrides the runtime directory.
pub const RUNTIME_ENV: &str = "RINGMD_RUNTIME";

/// The directory holding `init.lua`, in order of preference:
/// 1. `$RINGMD_RUNTIME`,
/// 2. `<prefix>/share/ringmd/runtime` next to the installed binary,
/// 3. `runtime/` in the source tree (development builds).
pub fn runtime_dir() -> PathBuf {
    if let Some(dir) = env::var_os(RUNTIME_ENV) {
        return PathBuf::from(dir);
    }
    let installed = env::current_exe().ok().and_then(|exe| {
        let prefix = exe.parent()?.parent()?;
        Some(prefix.join("share/ringmd/runtime"))
    });
    match installed {
        Some(dir) if has_init(&dir) => dir,
        _ => Path::new(env!("CARGO_MANIFEST_DIR")).join("runtime"),
    }
}

/// Path of the bundled `init.lua`.
pub fn init_lua() -> PathBuf {
    runtime_dir().join("init.lua")
}

fn has_init(dir: &Path) -> bool {
    dir.join("init.lua").is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_tree_runtime_exists() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("runtime");
        assert!(has_init(&dir), "missing {}", dir.display());
    }
}
