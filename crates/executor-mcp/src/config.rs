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
use executor_policy::{LoadedPolicy, PolicyError};
use executor_signer::{LocalSignerConfig, SignerError};
use serde::Deserialize;
use std::path::Path;

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
    /// Phase 5 Plan 05-03 / D-15: `[policy]` section. Defaults to `path = None`
    /// → server boots with `policy = None` → every `strategy_run` returns
    /// -32017 `policy_not_loaded` (fail-closed). Set `path` to a TOML file
    /// loaded by `executor_policy::load_policy_from_path` at boot.
    #[serde(default)]
    pub policy: PolicyFileSection,
    /// Phase 6 Plan 06-01: `[signer]` stores only the env-var name used later
    /// at the signing boundary; config parsing never reads private-key values.
    #[serde(default)]
    pub signer: SignerSection,
    /// v1.2 spike: EIP-7702 account abstraction. When `delegate` is set,
    /// strategy_run bundles multi-action runs into a single 7702 batch tx.
    #[serde(default)]
    pub aa: AaSection,
    /// v1.2 Stream E: shared knobs for the trigger workers. Today this only
    /// carries the mempool WSS endpoint used by `kind = mempool` workers.
    #[serde(default)]
    pub trigger: TriggerConfig,
}

/// `[trigger]` section — shared trigger-worker config. Optional.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TriggerConfig {
    /// Shared WSS endpoint for mempool subscriptions.
    /// Required for any `kind = mempool` trigger. Without this, the daemon
    /// logs a warn and skips spawning mempool workers.
    pub mempool_wss_url: Option<String>,
}

/// `[aa]` section — EIP-7702 account abstraction config. Optional.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AaSection {
    /// BatchExec contract address. When set AND a run has >=2 actions,
    /// strategy_run sends ONE EIP-7702 transaction delegating burner to
    /// this contract and calling executeBatch(calls) instead of N sequential txs.
    pub delegate: Option<String>,
}

/// `[signer]` section — stores a non-secret environment-variable reference
/// (Phase 6) plus the v1.3 keychain backend selector.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SignerSection {
    pub private_key_env: Option<String>,
    #[serde(default = "default_receipt_timeout_ms")]
    pub receipt_timeout_ms: u64,
    /// v1.3: backend selector — `"keychain"` or `"env"`. Defaults to
    /// `"env"` to preserve Phase 6 behaviour for existing configs.
    #[serde(default = "default_signer_backend")]
    pub backend: String,
    /// v1.3: keychain account / key id (when `backend = "keychain"`).
    /// Defaults to `"default"` per the naming contract.
    #[serde(default = "default_signer_key_id")]
    pub key_id: String,
}

fn default_receipt_timeout_ms() -> u64 {
    120_000
}

fn default_signer_backend() -> String {
    "env".into()
}

fn default_signer_key_id() -> String {
    "default".into()
}

impl Default for SignerSection {
    fn default() -> Self {
        Self {
            private_key_env: None,
            receipt_timeout_ms: default_receipt_timeout_ms(),
            backend: default_signer_backend(),
            key_id: default_signer_key_id(),
        }
    }
}

/// `[policy]` section — points at a TOML file consumed by
/// `executor_policy::load_policy_from_path`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PolicyFileSection {
    /// Path to policy.toml. `None` → policy NOT loaded (D-15 fail-closed).
    pub path: Option<String>,
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

    /// Build a non-secret local signer config from `[signer]`, if configured.
    ///
    /// v1.3 dispatch:
    /// - `backend = "env"` (or absent): legacy path — requires `private_key_env`.
    /// - `backend = "keychain"`: uses `key_id` (default `"default"`).
    pub fn signer_config(&self) -> Result<Option<LocalSignerConfig>, SignerError> {
        match self.signer.backend.as_str() {
            "keychain" => executor_signer::LocalSignerConfig::new_keychain(
                self.signer.key_id.clone(),
                self.signer.receipt_timeout_ms,
            )
            .map(Some),
            "env" => {
                let Some(env) = self.signer.private_key_env.as_deref() else {
                    return Ok(None);
                };
                LocalSignerConfig::new(env.to_string(), self.signer.receipt_timeout_ms).map(Some)
            }
            other => Err(SignerError::Config {
                detail: format!(
                    "unknown [signer].backend = {other:?} (expected \"keychain\" or \"env\")"
                ),
            }),
        }
    }

    /// Plan 05-03 / D-15: load + parse + validate the policy file at
    /// `[policy].path`, if set.
    ///
    /// Returns:
    /// - `Ok(None)` when `[policy].path` is absent — server proceeds with
    ///   `policy = None`; `strategy_run` will fail-closed on every call.
    /// - `Ok(Some(loaded))` when path is set + file parses + validates.
    /// - `Err(_)` on any IO / parse / validation failure. The MCP boundary
    ///   ([`crate::server::ExecutorServer::new_with_config`]) catches the
    ///   error, logs it via `tracing::error!`, and stores `None` (D-15
    ///   fail-closed — server still boots).
    pub fn policy_config(&self) -> Result<Option<LoadedPolicy>, PolicyError> {
        let Some(path_str) = self.policy.path.as_deref() else {
            return Ok(None);
        };
        executor_policy::load_policy_from_path(Path::new(path_str)).map(Some)
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
        // Phase 2 made `[state]` legal; Phase 5 Plan 05-03 makes `[policy]`
        // legal. Use a still-unreserved section name as the canary.
        let err = toml::from_str::<Config>("[bogus]\nsomething = 1\n").unwrap_err();
        assert!(err.to_string().to_lowercase().contains("bogus"));
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

    // ─────────── Phase 5 Plan 05-03 / D-15 [policy] ───────────

    #[test]
    fn policy_section_absent_yields_none_path() {
        let cfg: Config = toml::from_str("").unwrap();
        assert!(cfg.policy.path.is_none());
    }

    #[test]
    fn policy_section_path_propagates() {
        let toml_str = "[policy]\npath = \"/tmp/policy.toml\"\n";
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.policy.path.as_deref(), Some("/tmp/policy.toml"));
    }

    #[test]
    fn policy_config_returns_ok_none_when_path_absent() {
        let cfg: Config = toml::from_str("").unwrap();
        assert!(matches!(cfg.policy_config(), Ok(None)));
    }

    #[test]
    fn policy_config_loads_when_path_valid() {
        // Reference the fixture committed by Plan 05-03 Task 1. cargo test
        // runs each crate from its crate root; use the workspace-relative
        // path.
        let fixture = "../executor-policy/tests/fixtures/policy.permissive.toml";
        let toml_str = format!("[policy]\npath = \"{fixture}\"\n");
        let cfg: Config = toml::from_str(&toml_str).unwrap();
        match cfg.policy_config() {
            Ok(Some(loaded)) => {
                assert!(loaded.chains_allow.contains(&31337));
            }
            other => panic!("expected Ok(Some(_)); got {other:?}"),
        }
    }

    #[test]
    fn policy_config_returns_err_when_path_missing() {
        let toml_str = "[policy]\npath = \"/no/such/__missing_policy__.toml\"\n";
        let cfg: Config = toml::from_str(toml_str).unwrap();
        let err = cfg.policy_config().unwrap_err();
        assert!(matches!(err, PolicyError::FileNotFound { .. }));
        assert_eq!(err.data_kind(), "policy_not_loaded");
    }

    #[test]
    fn policy_section_rejects_unknown_field() {
        let err =
            toml::from_str::<Config>("[policy]\npath = \"/tmp/p.toml\"\nextra = true\n")
                .unwrap_err();
        assert!(err.to_string().to_lowercase().contains("extra"));
    }

    // ─────────── Phase 6 Plan 06-01 [signer] ───────────

    #[test]
    fn signer_section_absent_has_no_private_key_env() {
        let cfg: Config = toml::from_str("").unwrap();
        assert!(cfg.signer.private_key_env.is_none());
        assert_eq!(cfg.signer.receipt_timeout_ms, 120_000);
        assert!(matches!(cfg.signer_config(), Ok(None)));
    }

    #[test]
    fn signer_section_path_propagates_env_name() {
        let cfg: Config = toml::from_str(
            "[signer]\nprivate_key_env = \"EXECUTOR_PRIVATE_KEY\"\nreceipt_timeout_ms = 42\n",
        )
        .unwrap();
        assert_eq!(
            cfg.signer.private_key_env.as_deref(),
            Some("EXECUTOR_PRIVATE_KEY")
        );
        let signer = cfg.signer_config().unwrap().unwrap();
        assert_eq!(signer.private_key_env, "EXECUTOR_PRIVATE_KEY");
        assert_eq!(signer.receipt_timeout_ms, 42);
    }

    #[test]
    fn signer_section_rejects_unknown_field() {
        let err = toml::from_str::<Config>(
            "[signer]\nprivate_key_env = \"EXECUTOR_PRIVATE_KEY\"\nextra = true\n",
        )
        .unwrap_err();
        assert!(err.to_string().to_lowercase().contains("extra"));
    }

    #[test]
    fn signer_config_does_not_default_to_anvil_key() {
        let cfg = Config::default();
        assert!(cfg.signer.private_key_env.is_none());
        assert!(matches!(cfg.signer_config(), Ok(None)));
    }

    // ─────────── v1.2 Stream E [trigger] section ───────────

    #[test]
    fn trigger_section_absent_yields_none_mempool_wss_url() {
        let cfg: Config = toml::from_str("").unwrap();
        assert!(cfg.trigger.mempool_wss_url.is_none());
    }

    #[test]
    fn trigger_section_mempool_wss_url_propagates() {
        let cfg: Config = toml::from_str(
            "[trigger]\nmempool_wss_url = \"wss://example/v2/key\"\n",
        )
        .unwrap();
        assert_eq!(
            cfg.trigger.mempool_wss_url.as_deref(),
            Some("wss://example/v2/key")
        );
    }

    #[test]
    fn trigger_section_rejects_unknown_fields() {
        let err = toml::from_str::<Config>(
            "[trigger]\nmempool_wss_url = \"wss://x\"\nextra = true\n",
        )
        .unwrap_err();
        assert!(err.to_string().to_lowercase().contains("extra"));
    }

    #[test]
    fn signer_config_does_not_read_private_key_env_value() {
        let cfg: Config = toml::from_str(
            "[signer]\nprivate_key_env = \"EXECUTOR_PRIVATE_KEY\"\nreceipt_timeout_ms = 120000\n",
        )
        .unwrap();
        let signer = cfg.signer_config().expect("does not read secret");
        assert_eq!(
            signer.unwrap().private_key_env,
            "EXECUTOR_PRIVATE_KEY".to_string()
        );
    }
}
