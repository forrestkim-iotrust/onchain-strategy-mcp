//! Config loader for `executor-mcp`.
//!
//! Priority:
//!   1. `--config <path>` or `--config=<path>` CLI argument
//!   2. `EXECUTOR_CONFIG` environment variable
//!   3. `./config.toml` in the current working directory
//!   4. Built-in default
//!
//! Phase 2 extends Phase 1's `[logging]`-only surface with a `[state]`
//! section (D-03a, D-03e) pointing at the SQLite database file.
//! `#[serde(deny_unknown_fields)]` keeps typos noisy.

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub logging: LoggingConfig,
    #[serde(default)]
    pub state: StateConfig,
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

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateConfig {
    #[serde(default = "default_state_path")]
    pub path: String,
}

fn default_state_path() -> String {
    "./state.db".into()
}

impl Default for StateConfig {
    fn default() -> Self {
        Self {
            path: default_state_path(),
        }
    }
}

/// Parse `--config=PATH` or `--config PATH` from an arg vector. Testable
/// helper extracted to fix REVIEW IN-01 (Phase 1 only recognised the
/// space form, silently ignoring `--config=`).
fn parse_cli_config_path(args: &[String]) -> Option<String> {
    let mut i = 1;
    while i < args.len() {
        if let Some(rest) = args[i].strip_prefix("--config=") {
            return Some(rest.to_string());
        }
        if args[i] == "--config" && i + 1 < args.len() {
            return Some(args[i + 1].clone());
        }
        i += 1;
    }
    None
}

/// Load config honouring the priority order documented in the module docs.
///
/// Missing file = return `Config::default()`. Any IO or parse error is wrapped
/// with `anyhow::Context` so the error surface stays structured (never a raw
/// panic that could bleed into stdout).
pub fn load() -> Result<Config> {
    let args: Vec<String> = std::env::args().collect();
    let path_from_cli = parse_cli_config_path(&args);
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
    fn state_section_defaults_to_dot_state_db() {
        assert_eq!(Config::default().state.path, "./state.db");
    }

    #[test]
    fn parses_minimal_logging_section() {
        let cfg: Config = toml::from_str("[logging]\nlevel = \"debug\"\n").unwrap();
        assert_eq!(cfg.logging.level, "debug");
        // absent [state] ⇒ default (D-03e)
        assert_eq!(cfg.state.path, "./state.db");
    }

    #[test]
    fn parses_state_section() {
        let cfg: Config = toml::from_str("[state]\npath = \"/tmp/x.db\"\n").unwrap();
        assert_eq!(cfg.state.path, "/tmp/x.db");
    }

    #[test]
    fn absent_state_section_yields_default() {
        let cfg: Config = toml::from_str("[logging]\nlevel = \"info\"\n").unwrap();
        assert_eq!(cfg.state.path, "./state.db");
    }

    #[test]
    fn rejects_unknown_top_level_fields() {
        // [state] is no longer unknown as of Phase 2 — use a genuinely unknown section.
        let err = toml::from_str::<Config>("[policy]\nsomething = 1\n").unwrap_err();
        assert!(err.to_string().to_lowercase().contains("policy"));
    }

    #[test]
    fn rejects_unknown_logging_fields() {
        let err = toml::from_str::<Config>("[logging]\nlevel = \"info\"\nextra = true\n")
            .unwrap_err();
        assert!(err.to_string().to_lowercase().contains("extra"));
    }

    #[test]
    fn rejects_unknown_state_fields() {
        let err =
            toml::from_str::<Config>("[state]\npath = \".\"\nextra = true\n").unwrap_err();
        assert!(err.to_string().to_lowercase().contains("extra"));
    }

    #[test]
    fn cli_arg_equals_form_parses() {
        let args = vec!["bin".into(), "--config=/tmp/x.toml".into()];
        assert_eq!(parse_cli_config_path(&args).as_deref(), Some("/tmp/x.toml"));
    }

    #[test]
    fn cli_arg_space_form_parses() {
        let args = vec!["bin".into(), "--config".into(), "/tmp/y.toml".into()];
        assert_eq!(parse_cli_config_path(&args).as_deref(), Some("/tmp/y.toml"));
    }

    #[test]
    fn cli_arg_absent_returns_none() {
        let args = vec!["bin".into(), "--other".into()];
        assert!(parse_cli_config_path(&args).is_none());
    }
}
