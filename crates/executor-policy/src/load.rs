//! Phase 5 D-06 / D-15: TOML policy file load + validation.
//!
//! Produces [`LoadedPolicy`] (resolved alloy types) from [`PolicyConfig`]
//! (Plan 05-01 — TOML string forms). The MCP boundary
//! ([`crate::config::Config::policy_config`]) calls this at boot; on failure
//! the server proceeds with `policy = None` (D-15 fail-closed) and every
//! `strategy_run` returns `-32017 policy_not_loaded` until a valid policy
//! is provided.
//!
//! Validation is fail-fast: address parse, U256 decimal parse, selector
//! hex parse, and the Pitfall P-10 invariant (every chain in
//! `[chains.allow]` MUST have a corresponding `[contracts.<chain_id>]`
//! sub-table) all run during [`load_policy_from_path`].

use crate::error::PolicyError;
use crate::model::{
    ChainContract, LoadedPolicy, PolicyConfig, RawCallAllowResolved, SelectorPattern,
};
use alloy_primitives::{Address, U256};
use std::borrow::Cow;
use std::path::Path;
use std::str::FromStr;

/// Defensive size cap on policy.toml (Threat T-05-03-04 — BR-02 carry-forward).
/// A realistic policy is < 10 KiB; 1 MiB is a comfortable upper bound that
/// rejects pathological / DoS inputs without surprising an operator.
pub const MAX_POLICY_FILE_BYTES: u64 = 1024 * 1024;

/// Load + parse + validate the policy file at `path`.
///
/// Errors:
/// - [`PolicyError::FileNotFound`] when `path` does not exist.
///   `data_kind() == "policy_not_loaded"` (D-08 / D-15 fail-closed wire).
/// - [`PolicyError::Io`] for read failures (permission denied, etc).
/// - [`PolicyError::Config`] for TOML parse failures (`category = "toml_parse"`).
/// - [`PolicyError::ValidationError`] for address/U256/selector/Pitfall-P-10
///   failures.
pub fn load_policy_from_path(path: &Path) -> Result<LoadedPolicy, PolicyError> {
    if !path.exists() {
        return Err(PolicyError::FileNotFound {
            detail_for_log: format!("path {} not found", path.display()),
        });
    }
    // BR-02 size cap (T-05-03-04). Check via metadata before read_to_string so
    // a 100 MiB file doesn't pay the read cost first.
    let metadata = std::fs::metadata(path).map_err(|e| PolicyError::Io {
        detail_for_log: format!("metadata {}: {e}", path.display()),
    })?;
    if metadata.len() > MAX_POLICY_FILE_BYTES {
        return Err(PolicyError::Config {
            category: Cow::Borrowed("size_exceeds_cap"),
            detail_for_log: format!(
                "policy file size {} exceeds cap {} bytes",
                metadata.len(),
                MAX_POLICY_FILE_BYTES
            ),
        });
    }
    let raw = std::fs::read_to_string(path).map_err(|e| PolicyError::Io {
        detail_for_log: format!("read {}: {e}", path.display()),
    })?;
    parse_policy_str(&raw)
}

/// Parse + validate from raw TOML. Used by [`load_policy_from_path`] and by
/// tests that want to skip the filesystem step.
pub fn parse_policy_str(raw: &str) -> Result<LoadedPolicy, PolicyError> {
    let parsed: PolicyConfig = toml::from_str(raw).map_err(|e| PolicyError::Config {
        category: Cow::Borrowed("toml_parse"),
        detail_for_log: format!("toml::de: {e}"),
    })?;
    resolve(parsed)
}

/// v1.5 Track 1A: resolve a parsed [`PolicyConfig`] (e.g. loaded from JSON
/// at the `policy_set` MCP boundary) into a [`LoadedPolicy`]. Public so the
/// MCP layer can accept JSON, reuse the same address / U256 / selector
/// validation, and avoid re-implementing the chain-subtable invariant.
pub fn resolve_config(cfg: PolicyConfig) -> Result<LoadedPolicy, PolicyError> {
    resolve(cfg)
}

fn resolve(cfg: PolicyConfig) -> Result<LoadedPolicy, PolicyError> {
    let mut loaded = LoadedPolicy {
        chains_allow: cfg.chains.allow.clone(),
        ..LoadedPolicy::default()
    };

    // contracts: chain id (string) → Vec<String> → Vec<Address>
    for (chain_str, contracts_allow) in &cfg.contracts {
        let chain_id: u64 = chain_str.parse().map_err(|_| PolicyError::ValidationError {
            category: Cow::Borrowed("bad_chain_id"),
            detail_for_log: format!("contracts.<{chain_str}>: not a u64"),
        })?;
        let mut addrs = Vec::with_capacity(contracts_allow.allow.len());
        for s in &contracts_allow.allow {
            addrs.push(parse_address_lenient(s)?);
        }
        loaded.contracts_by_chain.insert(chain_id, addrs);
    }

    // Pitfall P-10: every chain in chains.allow MUST have a contracts.<id>
    // sub-table.
    for &chain_id in &loaded.chains_allow {
        if !loaded.contracts_by_chain.contains_key(&chain_id) {
            return Err(PolicyError::ValidationError {
                category: Cow::Borrowed("chain_missing_contracts_subtable"),
                detail_for_log: format!(
                    "[chains.allow] includes {chain_id} but no [contracts.{chain_id}] sub-table"
                ),
            });
        }
    }

    // selectors: "chain:address" → Vec<String> → Vec<SelectorPattern>
    for (key, sel_allow) in &cfg.selectors {
        let (chain_str, addr_str) = key.split_once(':').ok_or_else(|| {
            PolicyError::ValidationError {
                category: Cow::Borrowed("bad_selector_key"),
                detail_for_log: format!("selectors.{key}: expected `chain:address` form"),
            }
        })?;
        let chain_id: u64 = chain_str.parse().map_err(|_| PolicyError::ValidationError {
            category: Cow::Borrowed("bad_chain_id"),
            detail_for_log: format!("selectors.{key}: not a u64 chain"),
        })?;
        let addr = parse_address_lenient(addr_str)?;
        let mut patterns = Vec::with_capacity(sel_allow.allow.len());
        for s in &sel_allow.allow {
            patterns.push(parse_selector_pattern(s)?);
        }
        loaded
            .selectors_by_chain_contract
            .insert(ChainContract::new(chain_id, addr), patterns);
    }

    // native_value: chain id (string) → max_per_action (decimal) → U256
    for (chain_str, cap) in &cfg.native_value {
        let chain_id: u64 = chain_str.parse().map_err(|_| PolicyError::ValidationError {
            category: Cow::Borrowed("bad_chain_id"),
            detail_for_log: format!("native_value.<{chain_str}>: not a u64"),
        })?;
        let v = parse_u256_decimal(&cap.max_per_action)?;
        loaded.native_value_by_chain.insert(chain_id, v);
    }

    // erc20_spend: "chain:token" → max_per_run (decimal) → U256
    for (key, cap) in &cfg.erc20_spend {
        let (chain_str, tok_str) = key.split_once(':').ok_or_else(|| {
            PolicyError::ValidationError {
                category: Cow::Borrowed("bad_erc20_spend_key"),
                detail_for_log: format!("erc20_spend.{key}: expected `chain:token` form"),
            }
        })?;
        let chain_id: u64 = chain_str.parse().map_err(|_| PolicyError::ValidationError {
            category: Cow::Borrowed("bad_chain_id"),
            detail_for_log: format!("erc20_spend.{key}: not a u64 chain"),
        })?;
        let token = parse_address_lenient(tok_str)?;
        let v = parse_u256_decimal(&cap.max_per_run)?;
        loaded
            .erc20_spend_by_chain_token
            .insert(ChainContract::new(chain_id, token), v);
    }

    // raw_call
    loaded.raw_call_allow_global = cfg.raw_call.allow_global;
    for entry in &cfg.raw_call.allow {
        loaded.raw_call_allow.push(RawCallAllowResolved {
            chain: entry.chain,
            contract: parse_address_lenient(&entry.contract)?,
            selector: parse_selector_pattern(&entry.selector)?,
        });
    }

    Ok(loaded)
}

/// Lenient EIP-55 address parser (Phase 4 D-09 carry-forward).
///
/// - Strict checksum (`Address::parse_checksummed`) accepted unconditionally.
/// - Uniform-case (all-lowercase or all-uppercase, after `0x` strip) falls
///   back to `Address::from_str` (case-insensitive).
/// - Mixed-case-with-bad-checksum is REJECTED.
fn parse_address_lenient(s: &str) -> Result<Address, PolicyError> {
    if let Ok(addr) = Address::parse_checksummed(s, None) {
        return Ok(addr);
    }
    let stripped = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    if !stripped.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(PolicyError::ValidationError {
            category: Cow::Borrowed("bad_address"),
            detail_for_log: format!("address {s} contains non-hex characters"),
        });
    }
    let no_alpha = stripped.bytes().all(|b| !b.is_ascii_alphabetic());
    let all_lower = stripped
        .bytes()
        .all(|b| !b.is_ascii_alphabetic() || b.is_ascii_lowercase());
    let all_upper = stripped
        .bytes()
        .all(|b| !b.is_ascii_alphabetic() || b.is_ascii_uppercase());
    if (no_alpha || all_lower || all_upper)
        && let Ok(addr) = Address::from_str(s)
    {
        return Ok(addr);
    }
    Err(PolicyError::ValidationError {
        category: Cow::Borrowed("bad_address"),
        detail_for_log: format!("address {s} not valid (or bad checksum)"),
    })
}

fn parse_u256_decimal(s: &str) -> Result<U256, PolicyError> {
    if s.starts_with('-') {
        return Err(PolicyError::ValidationError {
            category: Cow::Borrowed("bad_u256_negative"),
            detail_for_log: format!("U256 cap {s} is negative"),
        });
    }
    if s.is_empty() {
        return Err(PolicyError::ValidationError {
            category: Cow::Borrowed("bad_u256"),
            detail_for_log: "U256 cap is empty".into(),
        });
    }
    if s.starts_with("0x") || s.starts_with("0X") {
        return Err(PolicyError::ValidationError {
            category: Cow::Borrowed("bad_u256"),
            detail_for_log: format!("U256 cap {s} must be decimal (no 0x prefix)"),
        });
    }
    if !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(PolicyError::ValidationError {
            category: Cow::Borrowed("bad_u256"),
            detail_for_log: format!("U256 cap {s} contains non-digit characters"),
        });
    }
    U256::from_str_radix(s, 10).map_err(|e| PolicyError::ValidationError {
        category: Cow::Borrowed("bad_u256"),
        detail_for_log: format!("U256 parse {s}: {e}"),
    })
}

fn parse_selector_pattern(s: &str) -> Result<SelectorPattern, PolicyError> {
    if s.eq_ignore_ascii_case("any") {
        return Ok(SelectorPattern::Any);
    }
    let stripped = s.strip_prefix("0x").ok_or_else(|| PolicyError::ValidationError {
        category: Cow::Borrowed("bad_selector_hex"),
        detail_for_log: format!("selector {s}: missing 0x prefix"),
    })?;
    if stripped.len() != 8 || !stripped.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(PolicyError::ValidationError {
            category: Cow::Borrowed("bad_selector_hex"),
            detail_for_log: format!("selector {s}: expected 0x + 8 hex chars"),
        });
    }
    let mut bytes = [0u8; 4];
    for i in 0..4 {
        bytes[i] = u8::from_str_radix(&stripped[i * 2..i * 2 + 2], 16)
            .expect("hex parse already validated above");
    }
    Ok(SelectorPattern::Specific(bytes))
}
