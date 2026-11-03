//! Logging setup. Verbosity comes from `RINGMD_LOG` (e.g. `RINGMD_LOG=debug`),
//! using `tracing_subscriber::EnvFilter` syntax. Default: `info`.

use tracing_subscriber::EnvFilter;

pub const ENV_VAR: &str = "RINGMD_LOG";

pub fn init() {
    let filter = EnvFilter::try_from_env(ENV_VAR).unwrap_or_else(|_| EnvFilter::new("info"));
    // Ignore the error if a subscriber is already installed (e.g. in tests).
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();
}
