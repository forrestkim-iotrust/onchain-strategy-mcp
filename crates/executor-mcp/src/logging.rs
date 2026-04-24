//! tracing subscriber wiring.
//!
//! D-05 / Pitfall 1: the default `tracing_subscriber::fmt::layer()` writer is
//! stdout, which would corrupt the JSON-RPC stream. `with_writer(std::io::stderr)`
//! is load-bearing — **do not remove**.

use anyhow::Result;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

/// Initialise a process-wide tracing subscriber that writes **only to stderr**.
///
/// `EnvFilter::try_from_default_env()` honours `RUST_LOG` when set; otherwise
/// it falls back to the level declared in `config.toml`.
pub fn init(cfg: &crate::config::Config) -> Result<()> {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&cfg.logging.level));
    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer().with_writer(std::io::stderr))
        .init();
    Ok(())
}
