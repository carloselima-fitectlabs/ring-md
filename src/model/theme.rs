//! Gruvbox theme variants (F5). M4 adds the palettes and generated CSS.

use std::fmt;
use std::str::FromStr;

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
}

impl fmt::Display for Variant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Error for an unknown variant name.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for variant in Variant::ALL {
            assert_eq!(variant.name().parse(), Ok(variant));
        }
        assert_eq!(Variant::default().name(), "dark-medium");
        assert_eq!(
            "gruvbox".parse::<Variant>(),
            Err(UnknownVariant("gruvbox".into()))
        );
    }
}
