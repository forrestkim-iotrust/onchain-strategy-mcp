//! Config loader for `executor-mcp`.
//!
//! Priority (D-06b):
//!   1. `--config <path>` CLI argument
//!   2. `EXECUTOR_CONFIG` environment variable
//!   3. `./config.toml` in the current working directory
//!   4. Built-in default (`logging.level = "info"`)
//!
//! `#[serde(deny_unknown_fields)]` makes accidental typos or Phase 2+ field
//! additions fail loudly instead of silently degrading (D-06).

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub logging: LoggingConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoggingConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
}

fn default_log_level() -> String {
    "info".into()
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
        }
    }
}

/// Load config honouring the priority order documented in the module docs.
///
/// Missing file = return `Config::default()`. Any IO or parse error is wrapped
/// with `anyhow::Context` so the error surface stays structured (never a raw
/// panic that could bleed into stdout — cf. D-05).
pub fn load() -> Result<Config> {
    // --config <path> CLI arg.
    let args: Vec<String> = std::env::args().collect();
    let mut path_from_cli: Option<String> = None;
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--config" && i + 1 < args.len() {
            path_from_cli = Some(args[i + 1].clone());
            break;
        }
        i += 1;
    }

    let path_from_env = std::env::var("EXECUTOR_CONFIG").ok();
    let default_path = std::path::PathBuf::from("config.toml");

    let path = path_from_cli
        .or(path_from_env)
        .map(std::path::PathBuf::from)
        .or_else(|| default_path.exists().then_some(default_path));

    match path {
        None => Ok(Config::default()),
        Some(p) => {
            let text = std::fs::read_to_string(&p)
                .with_context(|| format!("reading config from {}", p.display()))?;
            toml::from_str::<Config>(&text)
                .with_context(|| format!("parsing config at {}", p.display()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_log_level_is_info() {
        let cfg = Config::default();
        assert_eq!(cfg.logging.level, "info");
    }

    #[test]
    fn parses_minimal_logging_section() {
        let cfg: Config = toml::from_str("[logging]\nlevel = \"debug\"\n").unwrap();
        assert_eq!(cfg.logging.level, "debug");
    }

    #[test]
    fn rejects_unknown_top_level_fields() {
        let err = toml::from_str::<Config>("[state]\nsomething = 1\n").unwrap_err();
        // deny_unknown_fields should surface a message mentioning the unknown key.
        assert!(err.to_string().to_lowercase().contains("state"));
    }

    #[test]
    fn rejects_unknown_logging_fields() {
        let err = toml::from_str::<Config>("[logging]\nlevel = \"info\"\nextra = true\n")
            .unwrap_err();
        assert!(err.to_string().to_lowercase().contains("extra"));
    }
}
