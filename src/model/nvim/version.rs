//! Neovim version detection (F2.R3).

use std::fmt;
use std::process::Command;

use super::NvimError;

/// A Neovim release version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

/// Oldest Neovim RingMD supports.
pub const MINIMUM: Version = Version {
    major: 0,
    minor: 10,
    patch: 0,
};

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl Version {
    /// Parse the first line of `nvim --version`, e.g. `NVIM v0.10.4` or
    /// `NVIM v0.11.0-dev-1234+gabcdef`.
    pub fn parse(output: &str) -> Option<Self> {
        let first = output.lines().next()?;
        let rest = first.trim().strip_prefix("NVIM v")?;
        let mut parts = rest.split(|c: char| !c.is_ascii_digit());
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
        Some(Self {
            major,
            minor,
            patch,
        })
    }
}

/// Run `<program> --version` and make sure it is at least [`MINIMUM`].
pub fn check(program: &str) -> Result<Version, NvimError> {
    let output = Command::new(program)
        .arg("--version")
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                NvimError::NotFound {
                    program: program.to_owned(),
                }
            } else {
                NvimError::Spawn(e.to_string())
            }
        })?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let version = Version::parse(&stdout)
        .ok_or_else(|| NvimError::UnknownVersion(stdout.lines().next().unwrap_or("").into()))?;
    if version < MINIMUM {
        return Err(NvimError::TooOld { found: version });
    }
    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(major: u32, minor: u32, patch: u32) -> Version {
        Version {
            major,
            minor,
            patch,
        }
    }

    #[test]
    fn parses_release_and_dev_versions() {
        assert_eq!(
            Version::parse("NVIM v0.10.4\nBuild type: Release"),
            Some(v(0, 10, 4))
        );
        assert_eq!(
            Version::parse("NVIM v0.11.0-dev-1234+gabcdef"),
            Some(v(0, 11, 0))
        );
        assert_eq!(Version::parse("vim 9.1"), None);
        assert_eq!(Version::parse(""), None);
    }

    #[test]
    fn compares_against_minimum() {
        assert!(v(0, 9, 5) < MINIMUM);
        assert!(v(0, 10, 0) >= MINIMUM);
        assert!(v(1, 0, 0) > MINIMUM);
    }

    #[test]
    fn missing_program_is_not_found() {
        let err = check("ringmd-definitely-not-nvim").unwrap_err();
        assert!(matches!(err, NvimError::NotFound { .. }), "{err:?}");
    }
}
