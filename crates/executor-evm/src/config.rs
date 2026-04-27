//! `EvmConfig` — Phase 4 D-04 defaults.

use std::time::Duration;
use url::Url;

use crate::EvmError;

/// EVM provider configuration. The MCP boundary builds this from the
/// `[evm]` section of `ExecutorConfig` via [`EvmConfig::from_raw`].
#[derive(Debug, Clone)]
pub struct EvmConfig {
    pub rpc_url: Url,
    pub call_timeout: Duration,
}

impl Default for EvmConfig {
    fn default() -> Self {
        Self {
            rpc_url: "http://127.0.0.1:8545"
                .parse()
                .expect("static URL parses"),
            call_timeout: Duration::from_millis(1_000),
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
    pub fn from_raw(rpc_url: &str, call_timeout_ms: u64) -> Result<Self, EvmError> {
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
        Ok(Self {
            rpc_url,
            call_timeout: Duration::from_millis(call_timeout_ms),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_d04_lock() {
        let cfg = EvmConfig::default();
        assert_eq!(cfg.rpc_url.as_str(), "http://127.0.0.1:8545/");
        assert_eq!(cfg.call_timeout, Duration::from_millis(1_000));
    }

    #[test]
    fn from_raw_accepts_valid_inputs() {
        let cfg = EvmConfig::from_raw("http://localhost:8545", 500).unwrap();
        assert_eq!(cfg.call_timeout, Duration::from_millis(500));
    }

    #[test]
    fn from_raw_rejects_bad_url() {
        let err = EvmConfig::from_raw("not a url", 1000).unwrap_err();
        assert_eq!(err.data_kind(), "evm_rpc_error");
        assert!(matches!(err, EvmError::Config { .. }));
    }

    #[test]
    fn from_raw_rejects_timeout_below_min() {
        assert!(EvmConfig::from_raw("http://127.0.0.1:8545", 10).is_err());
    }

    #[test]
    fn from_raw_rejects_timeout_above_max() {
        assert!(EvmConfig::from_raw("http://127.0.0.1:8545", 60_000).is_err());
    }
}
