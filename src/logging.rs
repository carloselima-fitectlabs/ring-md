//! Logging setup. Verbosity comes from `RINGMD_LOG`. A bare level such as
//! `RINGMD_LOG=debug` applies to RingMD itself (libraries stay at `warn`);
//! anything else is `tracing_subscriber::EnvFilter` syntax, e.g.
//! `RINGMD_LOG=debug,html5ever=warn`. Default: `info`.

use tracing_subscriber::EnvFilter;

pub const ENV_VAR: &str = "RINGMD_LOG";

pub fn init() {
    let spec = std::env::var(ENV_VAR).unwrap_or_default();
    let filter = EnvFilter::try_new(filter_spec(&spec)).unwrap_or_else(|_| EnvFilter::new("info"));
    // Ignore the error if a subscriber is already installed (e.g. in tests).
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();
}

/// The `EnvFilter` directives for a `RINGMD_LOG` value.
fn filter_spec(value: &str) -> String {
    const LEVELS: &[&str] = &["off", "error", "warn", "info", "debug", "trace"];
    let value = value.trim();
    if value.is_empty() {
        "warn,ringmd=info".to_owned()
    } else if LEVELS.contains(&value.to_ascii_lowercase().as_str()) {
        format!("warn,ringmd={value}")
    } else {
        value.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_levels_apply_to_ringmd() {
        assert_eq!(filter_spec(""), "warn,ringmd=info");
        assert_eq!(filter_spec("debug"), "warn,ringmd=debug");
        assert_eq!(filter_spec("TRACE"), "warn,ringmd=TRACE");
        assert_eq!(filter_spec("debug,html5ever=warn"), "debug,html5ever=warn");
        assert_eq!(filter_spec("ringmd::view=trace"), "ringmd::view=trace");
    }
}
