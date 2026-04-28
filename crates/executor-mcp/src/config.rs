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
    /// Phase 4 D-04: `[evm]` section. Defaults preserve server boot when
    /// absent — the provider is constructed lazily on first `ctx.evm.*` call.
    #[serde(default)]
    pub evm: EvmSection,
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

/// Phase 4 D-04 `[evm]` section. The MCP boundary builds an
/// [`executor_evm::EvmConfig`] from this via [`Config::evm_config`] when the
/// first `ctx.evm.*` call fires (lazy provider).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvmSection {
    #[serde(default = "default_evm_rpc_url")]
    pub rpc_url: String,
    #[serde(default = "default_evm_call_timeout_ms")]
    pub call_timeout_ms: u64,
    /// Phase 5 D-14: `from` address used by the simulation adapter
    /// (`executor-evm::simulate::simulate_one`). Defaults to anvil
    /// account[0] (EIP-55) for devnet ergonomics. Validated at
    /// [`Config::evm_config`] (lenient EIP-55 — mixed-case-bad-checksum
    /// REJECTED with `EvmError::Config`).
    #[serde(default = "default_simulation_from")]
    pub simulation_from: String,
}

fn default_evm_rpc_url() -> String {
    "http://127.0.0.1:8545".into()
}

fn default_evm_call_timeout_ms() -> u64 {
    1_000
}

fn default_simulation_from() -> String {
    "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266".into()
}

impl Default for EvmSection {
    fn default() -> Self {
        Self {
            rpc_url: default_evm_rpc_url(),
            call_timeout_ms: default_evm_call_timeout_ms(),
            simulation_from: default_simulation_from(),
        }
    }
}

impl Config {
    /// Build a typed [`executor_evm::EvmConfig`] from the parsed `[evm]`
    /// section. Validation errors (bad URL, timeout out of range) surface
    /// as [`executor_evm::EvmError::Config`].
    pub fn evm_config(&self) -> Result<executor_evm::EvmConfig, executor_evm::EvmError> {
        executor_evm::EvmConfig::from_raw(
            &self.evm.rpc_url,
            self.evm.call_timeout_ms,
            &self.evm.simulation_from,
        )
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

    // ─────────── Phase 4 D-04 [evm] section ───────────

    #[test]
    fn evm_section_uses_defaults_when_absent() {
        let cfg: Config =
            toml::from_str("[logging]\nlevel = \"info\"\n").unwrap();
        assert_eq!(cfg.evm.rpc_url, "http://127.0.0.1:8545");
        assert_eq!(cfg.evm.call_timeout_ms, 1_000);
    }

    #[test]
    fn evm_section_overrides_defaults() {
        let cfg: Config = toml::from_str(
            "[evm]\nrpc_url = \"http://example:8545\"\ncall_timeout_ms = 500\n",
        )
        .unwrap();
        assert_eq!(cfg.evm.rpc_url, "http://example:8545");
        assert_eq!(cfg.evm.call_timeout_ms, 500);
    }

    #[test]
    fn evm_config_builds_typed_evm_config() {
        let cfg = Config::default();
        let evm = cfg.evm_config().expect("default builds");
        assert_eq!(evm.rpc_url.as_str(), "http://127.0.0.1:8545/");
    }

    #[test]
    fn evm_section_rejects_unknown_fields() {
        let err = toml::from_str::<Config>(
            "[evm]\nrpc_url = \"http://localhost:8545\"\nextra = true\n",
        )
        .unwrap_err();
        assert!(err.to_string().to_lowercase().contains("extra"));
    }

    // ─────────── Phase 5 Plan 05-02 / D-14 [evm.simulation_from] ───────────

    const ANVIL_0: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";

    #[test]
    fn evm_section_default_simulation_from_is_anvil_account_0() {
        // Empty TOML → all defaults; the [evm] default must include the
        // anvil-0 EIP-55 simulation_from per D-14.
        let cfg: Config = toml::from_str("").unwrap();
        assert_eq!(cfg.evm.simulation_from, ANVIL_0);
    }

    #[test]
    fn evm_section_simulation_from_override_is_propagated() {
        let toml_str = "\
            [evm]\n\
            simulation_from = \"0x0000000000000000000000000000000000000001\"\n\
        ";
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(
            cfg.evm.simulation_from,
            "0x0000000000000000000000000000000000000001"
        );
        // Lowercase + zeros parses through validate_address lenient path.
        let evm = cfg.evm_config().expect("override builds");
        // The address parses as the all-zero/one canonical address.
        assert_ne!(
            evm.simulation_from,
            executor_evm::EvmConfig::default().simulation_from
        );
    }

    #[test]
    fn evm_section_simulation_from_bad_checksum_returns_err_at_evm_config() {
        // Capital F at index 0 (after 0x) breaks the EIP-55 checksum.
        let toml_str = "\
            [evm]\n\
            simulation_from = \"0xF39Fd6e51aad88F6F4ce6aB8827279cffFb92266\"\n\
        ";
        let cfg: Config = toml::from_str(toml_str).unwrap();
        // TOML parse OK — the field is a String at this layer.
        assert!(cfg.evm.simulation_from.starts_with("0xF39Fd6"));
        // But evm_config() rejects via the lenient EIP-55 validator.
        let err = cfg.evm_config().unwrap_err();
        assert_eq!(err.data_kind(), "evm_rpc_error");
    }

    #[test]
    fn evm_config_default_simulation_from_round_trips_through_evm_config() {
        // The default `[evm]` section MUST produce a buildable EvmConfig —
        // this guards against the default string drifting out of EIP-55.
        let cfg = Config::default();
        let evm = cfg.evm_config().expect("default evm_config builds");
        assert_eq!(
            evm.simulation_from,
            executor_evm::EvmConfig::default().simulation_from,
        );
    }
}
