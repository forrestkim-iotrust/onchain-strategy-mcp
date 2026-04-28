//! `PolicyConfig` — TOML schema for the policy DSL (Phase 5 D-06).
//!
//! All structs use `#[serde(deny_unknown_fields)]` so a typo at any level
//! becomes a load-time `PolicyError::Config { category: "unknown_field", .. }`
//! (Plan 05-03 implements the load wrapper). `Default` returns the deny-all
//! shape — empty allowlists, raw_call gate disabled.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// POL-01 chain allowlist.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Chains {
    #[serde(default)]
    pub allow: Vec<u64>,
}

/// POL-02 per-chain contract allowlist. Address strings are parsed lenient
/// EIP-55 at load time (Plan 05-03) — same validator as Phase 4 D-09.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ContractsAllow {
    #[serde(default)]
    pub allow: Vec<String>,
}

/// POL-03 per-(chain, contract) selector allowlist. Each entry is a 4-byte
/// hex `"0xXXXXXXXX"` or the sentinel `"any"`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SelectorsAllow {
    #[serde(default)]
    pub allow: Vec<String>,
}

/// POL-04 per-chain native value cap. Decimal-string per D-03 (parsed to
/// `U256` at load time).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NativeValueCap {
    pub max_per_action: String,
}

/// POL-05 per-(chain, token) ERC20 cumulative spend cap. Decimal-string per
/// D-03.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Erc20SpendCap {
    pub max_per_run: String,
}

/// POL-06 raw_call gate. Default DENY (`allow_global = false`,
/// `allow.is_empty()`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RawCallGate {
    #[serde(default)]
    pub allow_global: bool,
    #[serde(default)]
    pub allow: Vec<RawCallAllowEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RawCallAllowEntry {
    pub chain: u64,
    pub contract: String,
    /// 4-byte hex `"0xXXXXXXXX"` or the sentinel `"any"`.
    pub selector: String,
}

/// Top-level policy config. `Default::default()` is deny-all.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PolicyConfig {
    #[serde(default)]
    pub chains: Chains,
    /// Map keyed by chain id (string).
    #[serde(default)]
    pub contracts: HashMap<String, ContractsAllow>,
    /// Map keyed by `"chain:address"`.
    #[serde(default)]
    pub selectors: HashMap<String, SelectorsAllow>,
    /// Map keyed by chain id (string).
    #[serde(default)]
    pub native_value: HashMap<String, NativeValueCap>,
    /// Map keyed by `"chain:token"`.
    #[serde(default)]
    pub erc20_spend: HashMap<String, Erc20SpendCap>,
    #[serde(default)]
    pub raw_call: RawCallGate,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_rejects_unknown_top_level_fields() {
        let toml_str = r#"
            [chains]
            allow = []
            [bogus]
            anything = 1
        "#;
        let result: Result<PolicyConfig, _> = toml::from_str(toml_str);
        assert!(
            result.is_err(),
            "deny_unknown_fields must reject [bogus] at top level"
        );
    }

    #[test]
    fn config_rejects_unknown_field_in_chains() {
        let toml_str = r#"
            [chains]
            allow = []
            unexpected = "x"
        "#;
        let result: Result<PolicyConfig, _> = toml::from_str(toml_str);
        assert!(
            result.is_err(),
            "deny_unknown_fields must reject unexpected key inside [chains]"
        );
    }

    #[test]
    fn default_is_deny_all() {
        let c = PolicyConfig::default();
        assert!(c.chains.allow.is_empty());
        assert!(c.contracts.is_empty());
        assert!(c.selectors.is_empty());
        assert!(c.native_value.is_empty());
        assert!(c.erc20_spend.is_empty());
        assert!(!c.raw_call.allow_global);
        assert!(c.raw_call.allow.is_empty());
    }

    #[test]
    fn config_round_trips_minimal_shape() {
        let toml_str = r#"
            [chains]
            allow = [31337]

            [contracts.31337]
            allow = ["0x0000000000000000000000000000000000000001"]

            [raw_call]
            allow_global = false
        "#;
        let parsed: PolicyConfig = toml::from_str(toml_str).expect("parse ok");
        assert_eq!(parsed.chains.allow, vec![31337u64]);
        assert_eq!(
            parsed
                .contracts
                .get("31337")
                .map(|c| c.allow.len())
                .unwrap_or(0),
            1
        );
        assert!(!parsed.raw_call.allow_global);
    }
}
