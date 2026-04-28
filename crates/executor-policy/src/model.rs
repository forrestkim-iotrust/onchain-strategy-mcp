//! `PolicyConfig` — TOML schema for the policy DSL (Phase 5 D-06).
//!
//! All structs use `#[serde(deny_unknown_fields)]` so a typo at any level
//! becomes a load-time `PolicyError::Config { category: "unknown_field", .. }`
//! (Plan 05-03 implements the load wrapper). `Default` returns the deny-all
//! shape — empty allowlists, raw_call gate disabled.

use alloy_primitives::{Address, U256};
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

// ───────────── Plan 05-03 — LoadedPolicy resolved type ─────────────

/// Resolved (post-load) policy. Addresses parsed to alloy types; decimal
/// strings parsed to `U256`; selectors parsed to typed enum.
/// `evaluate(&LoadedPolicy, ...)` (Plan 05-03 Task 2) is the consumer.
///
/// `Default::default()` is **deny-all** — empty allowlists. The orchestrator
/// (Plan 05-04) builds this once at boot via [`crate::load::load_policy_from_path`].
#[derive(Debug, Clone, Default, Serialize)]
pub struct LoadedPolicy {
    pub chains_allow: Vec<u64>,
    pub contracts_by_chain: HashMap<u64, Vec<Address>>,
    pub selectors_by_chain_contract: HashMap<ChainContract, Vec<SelectorPattern>>,
    pub native_value_by_chain: HashMap<u64, U256>,
    pub erc20_spend_by_chain_token: HashMap<ChainContract, U256>,
    pub raw_call_allow_global: bool,
    pub raw_call_allow: Vec<RawCallAllowResolved>,
}

/// Composite key for selectors / erc20 spend caps. Serialised as
/// `"<chain>:<address>"` so `policy_get` and tracing keep stable strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChainContract {
    pub chain: u64,
    pub contract: Address,
}

impl ChainContract {
    pub fn new(chain: u64, contract: Address) -> Self {
        Self { chain, contract }
    }
}

impl Serialize for ChainContract {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("{}:{}", self.chain, self.contract))
    }
}

/// 4-byte selector pattern. `Any` admits all selectors at the matching
/// `(chain, contract)` key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectorPattern {
    Specific(#[serde(serialize_with = "serialize_selector_hex")] [u8; 4]),
    Any,
}

fn serialize_selector_hex<S: serde::Serializer>(
    s: &[u8; 4],
    ser: S,
) -> Result<S::Ok, S::Error> {
    ser.serialize_str(&format!("0x{:02x}{:02x}{:02x}{:02x}", s[0], s[1], s[2], s[3]))
}

#[derive(Debug, Clone, Serialize)]
pub struct RawCallAllowResolved {
    pub chain: u64,
    #[serde(serialize_with = "serialize_address")]
    pub contract: Address,
    pub selector: SelectorPattern,
}

fn serialize_address<S: serde::Serializer>(a: &Address, ser: S) -> Result<S::Ok, S::Error> {
    ser.serialize_str(&format!("{a}"))
}

impl LoadedPolicy {
    /// POL-01 — chain ∈ allowlist.
    pub fn allows_chain(&self, chain_id: u64) -> bool {
        self.chains_allow.contains(&chain_id)
    }

    /// POL-02 — chain has a `[contracts.<id>]` sub-table AND `addr` is in the
    /// allow list. Empty subtable / missing subtable → deny.
    pub fn allows_contract(&self, chain_id: u64, addr: &Address) -> bool {
        self.contracts_by_chain
            .get(&chain_id)
            .is_some_and(|list| list.contains(addr))
    }

    /// POL-03 — selector match at `(chain, contract)`. The `Any` sentinel
    /// admits every selector. Skipped by `eval` for `RawCall` actions
    /// (POL-06 is the exclusive gate for raw calls — D-06).
    pub fn allows_selector(
        &self,
        chain_id: u64,
        contract: &Address,
        selector: &[u8; 4],
    ) -> bool {
        let key = ChainContract::new(chain_id, *contract);
        self.selectors_by_chain_contract
            .get(&key)
            .is_some_and(|list| {
                list.iter().any(|p| match p {
                    SelectorPattern::Any => true,
                    SelectorPattern::Specific(s) => s == selector,
                })
            })
    }

    /// POL-04 — per-action native value cap. **Cap absent for a chain ⇒ 0**
    /// (deny-by-default for any non-zero value on that chain).
    pub fn native_value_cap(&self, chain_id: u64) -> U256 {
        self.native_value_by_chain
            .get(&chain_id)
            .copied()
            .unwrap_or(U256::ZERO)
    }

    /// POL-05 — per-(chain, token) cumulative spend cap. `None` means no cap
    /// is configured for that token (researcher A-7: cap absent means
    /// uncapped on that token specifically; documented behaviour).
    pub fn erc20_spend_cap(&self, chain_id: u64, token: &Address) -> Option<U256> {
        let key = ChainContract::new(chain_id, *token);
        self.erc20_spend_by_chain_token.get(&key).copied()
    }

    /// POL-06 — raw_call gate. Returns `true` iff `allow_global == true` OR
    /// some `(chain, contract, selector)` entry matches. The `Any` selector
    /// sentinel admits every selector at the listed contract; a `Specific`
    /// pattern requires both `Some(s)` calldata and exact byte match.
    /// Sub-4-byte calldata (`selector = None`) requires either `allow_global`
    /// or an `Any` entry at the matching contract.
    pub fn raw_call_allows(
        &self,
        chain_id: u64,
        contract: &Address,
        selector: Option<&[u8; 4]>,
    ) -> bool {
        if self.raw_call_allow_global {
            return true;
        }
        self.raw_call_allow.iter().any(|e| {
            if e.chain != chain_id || &e.contract != contract {
                return false;
            }
            match (&e.selector, selector) {
                (SelectorPattern::Any, _) => true,
                (SelectorPattern::Specific(a), Some(b)) => a == b,
                (SelectorPattern::Specific(_), None) => false,
            }
        })
    }
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
