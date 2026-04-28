//! `EvmConfig` — Phase 4 D-04 defaults.
//!
//! Phase 5 D-14: extended with `simulation_from`, the `from` address used by
//! `executor_evm::simulate::simulate_one` to avoid the alloy `Provider::call`
//! default-zero-sender pitfall (RESEARCH P-1). Default is anvil account[0]
//! for devnet ergonomics; non-anvil RPCs MUST set `[evm].simulation_from`
//! explicitly.

use std::str::FromStr;
use std::time::Duration;

use alloy_primitives::Address;
use url::Url;

use crate::EvmError;

/// EVM provider configuration. The MCP boundary builds this from the
/// `[evm]` section of `ExecutorConfig` via [`EvmConfig::from_raw`].
#[derive(Debug, Clone)]
pub struct EvmConfig {
    pub rpc_url: Url,
    pub call_timeout: Duration,
    /// Phase 5 D-14: `from` address used by the simulation adapter
    /// (`executor-evm::simulate::simulate_one`). Defaults to anvil
    /// account[0] for devnet ergonomics; non-anvil RPCs should set
    /// this explicitly via `[evm.simulation_from]`.
    pub simulation_from: Address,
}

/// Anvil account[0] — the default funded deployer in `anvil --chain-id 31337`.
/// EIP-55 checksummed.
const DEFAULT_SIMULATION_FROM: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";

impl Default for EvmConfig {
    fn default() -> Self {
        Self {
            rpc_url: "http://127.0.0.1:8545"
                .parse()
                .expect("static URL parses"),
            call_timeout: Duration::from_millis(1_000),
            simulation_from: Address::parse_checksummed(DEFAULT_SIMULATION_FROM, None)
                .expect("static EIP-55 anvil-0 address parses"),
        }
    }
}

impl EvmConfig {
    /// Construct from raw config inputs. Validation:
    /// - `rpc_url` must parse via the `url` crate.
    /// - `call_timeout_ms` must lie within `[50, 30_000]` — anything below
    ///   50ms is below normal RTT for a localhost JSON-RPC roundtrip; anything
    ///   above 30s exceeds the Phase-3 wall-clock 2s envelope by a safety
    ///   margin that suggests config error.
    /// - `simulation_from` (Phase 5 D-14): lenient EIP-55 — strict
    ///   `parse_checksummed` accepted unconditionally; uniformly-cased
    ///   (all-lower / all-upper / no-alpha) 40-hex falls through to
    ///   `Address::from_str`; mixed-case-bad-checksum REJECTED.
    pub fn from_raw(
        rpc_url: &str,
        call_timeout_ms: u64,
        simulation_from: &str,
    ) -> Result<Self, EvmError> {
        let rpc_url: Url = rpc_url.parse().map_err(|e: url::ParseError| EvmError::Config {
            detail_for_log: format!("rpc_url parse: {e}"),
        })?;
        if !(50..=30_000).contains(&call_timeout_ms) {
            return Err(EvmError::Config {
                detail_for_log: format!(
                    "call_timeout_ms {call_timeout_ms} not in 50..=30000"
                ),
            });
        }
        let simulation_from = parse_simulation_from(simulation_from)?;
        Ok(Self {
            rpc_url,
            call_timeout: Duration::from_millis(call_timeout_ms),
            simulation_from,
        })
    }
}

/// Lenient EIP-55 parser for `simulation_from` (Phase 5 D-14, mirrors the
/// Phase 4 D-09 `validate_address` pattern). Accepts:
/// - Strict EIP-55 checksum (any mixed-case input that checksums correctly).
/// - Uniformly-cased 40-hex (all-lower, all-upper, or no-alpha).
///
/// REJECTS mixed-case-bad-checksum with a stable taxonomy detail.
fn parse_simulation_from(s: &str) -> Result<Address, EvmError> {
    if let Ok(addr) = Address::parse_checksummed(s, None) {
        return Ok(addr);
    }
    let body = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    if !body.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(EvmError::Config {
            detail_for_log: format!("simulation_from contains non-hex characters: {s}"),
        });
    }
    let has_alpha = body.chars().any(|c| c.is_ascii_alphabetic());
    let all_lower = body.chars().all(|c| !c.is_ascii_alphabetic() || c.is_ascii_lowercase());
    let all_upper = body.chars().all(|c| !c.is_ascii_alphabetic() || c.is_ascii_uppercase());
    if !has_alpha || all_lower || all_upper {
        return Address::from_str(s).map_err(|e| EvmError::Config {
            detail_for_log: format!("simulation_from parse: {e}"),
        });
    }
    Err(EvmError::Config {
        detail_for_log: format!(
            "simulation_from looks checksummed but checksum is invalid: {s}"
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANVIL_0: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";

    #[test]
    fn default_matches_d04_lock() {
        let cfg = EvmConfig::default();
        assert_eq!(cfg.rpc_url.as_str(), "http://127.0.0.1:8545/");
        assert_eq!(cfg.call_timeout, Duration::from_millis(1_000));
    }

    #[test]
    fn default_simulation_from_is_anvil_account_0() {
        let cfg = EvmConfig::default();
        assert_eq!(
            cfg.simulation_from,
            Address::parse_checksummed(ANVIL_0, None).unwrap(),
        );
    }

    #[test]
    fn from_raw_accepts_valid_inputs() {
        let cfg = EvmConfig::from_raw("http://localhost:8545", 500, ANVIL_0).unwrap();
        assert_eq!(cfg.call_timeout, Duration::from_millis(500));
        assert_eq!(
            cfg.simulation_from,
            Address::parse_checksummed(ANVIL_0, None).unwrap(),
        );
    }

    #[test]
    fn from_raw_accepts_lowercase_simulation_from() {
        let lc = ANVIL_0.to_lowercase();
        let cfg = EvmConfig::from_raw("http://127.0.0.1:8545", 1000, &lc).unwrap();
        assert_eq!(
            cfg.simulation_from,
            Address::parse_checksummed(ANVIL_0, None).unwrap(),
        );
    }

    #[test]
    fn from_raw_accepts_uppercase_simulation_from() {
        let uc = format!("0x{}", ANVIL_0.trim_start_matches("0x").to_uppercase());
        let cfg = EvmConfig::from_raw("http://127.0.0.1:8545", 1000, &uc).unwrap();
        assert_eq!(
            cfg.simulation_from,
            Address::parse_checksummed(ANVIL_0, None).unwrap(),
        );
    }

    #[test]
    fn from_raw_rejects_mixed_case_bad_checksum_simulation_from() {
        // Capital F at index 0 (after 0x) breaks the EIP-55 checksum.
        let bad = "0xF39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
        let err = EvmConfig::from_raw("http://127.0.0.1:8545", 1000, bad).unwrap_err();
        assert!(matches!(err, EvmError::Config { .. }));
        assert_eq!(err.data_kind(), "evm_rpc_error");
    }

    #[test]
    fn from_raw_rejects_non_hex_simulation_from() {
        let err = EvmConfig::from_raw("http://127.0.0.1:8545", 1000, "not-an-address").unwrap_err();
        assert!(matches!(err, EvmError::Config { .. }));
    }

    #[test]
    fn from_raw_rejects_bad_url() {
        let err = EvmConfig::from_raw("not a url", 1000, ANVIL_0).unwrap_err();
        assert_eq!(err.data_kind(), "evm_rpc_error");
        assert!(matches!(err, EvmError::Config { .. }));
    }

    #[test]
    fn from_raw_rejects_timeout_below_min() {
        assert!(EvmConfig::from_raw("http://127.0.0.1:8545", 10, ANVIL_0).is_err());
    }

    #[test]
    fn from_raw_rejects_timeout_above_max() {
        assert!(EvmConfig::from_raw("http://127.0.0.1:8545", 60_000, ANVIL_0).is_err());
    }
}
