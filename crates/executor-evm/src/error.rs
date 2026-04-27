//! Typed EVM errors (Phase 4 D-12). The MCP boundary maps these to -32017
//! STRATEGY_RUNTIME_ERROR with `data.kind ∈ {evm_rpc_error, evm_decode_error,
//! evm_revert}` and STABLE `data.detail` strings (HR/MR-01 carry-forward —
//! raw alloy / reqwest text goes to `tracing::warn!` only).
//!
//! `Display` is wire-safe: it emits ONLY the stable taxonomy strings. Raw
//! transport / decode / revert text lives in the per-variant `detail_for_log`
//! field and is intended for `tracing::warn!` consumption at the MCP boundary
//! — NEVER `format!` it into a wire response.

#[derive(Debug, thiserror::Error)]
pub enum EvmError {
    /// Transport-level RPC failure (anvil down, HTTP 500, connection refused).
    /// Raw text MUST go to `tracing::warn!`; Display is wire-safe.
    #[error("evm rpc error: transport")]
    Transport { detail_for_log: String },

    /// Host-side decode failure (wrong ABI for data, malformed return bytes,
    /// JSON parse failure, function-not-found).
    #[error("evm decode error: {category}")]
    Decode {
        category: &'static str,
        detail_for_log: String,
    },

    /// Contract reverted. `reason` is decoded if available (e.g. via the
    /// standard `Error(string)` selector); raw bytes go to tracing.
    #[error("evm revert: {reason}")]
    Revert {
        reason: String,
        detail_for_log: String,
    },

    /// Per-call timeout fired (D-04 — call_timeout_ms).
    #[error("evm rpc error: timeout")]
    Timeout,

    /// Encoding-side input rejected before transport.
    #[error("evm encode error: {category}")]
    Encode {
        category: &'static str,
        detail_for_log: String,
    },

    /// Provider build / config failure (URL parse, timeout out of range).
    #[error("evm provider config error")]
    Config { detail_for_log: String },
}

impl EvmError {
    /// `data.kind` dispatch — Phase 4 D-12 taxonomy.
    pub fn data_kind(&self) -> &'static str {
        match self {
            Self::Transport { .. } | Self::Timeout | Self::Config { .. } => "evm_rpc_error",
            Self::Decode { .. } | Self::Encode { .. } => "evm_decode_error",
            Self::Revert { .. } => "evm_revert",
        }
    }

    /// Operator-only diagnostic text. Routed to `tracing::warn!` at the MCP
    /// boundary; NEVER formatted onto the wire.
    pub fn detail_for_log(&self) -> &str {
        match self {
            Self::Transport { detail_for_log } => detail_for_log,
            Self::Decode { detail_for_log, .. } => detail_for_log,
            Self::Revert { detail_for_log, .. } => detail_for_log,
            Self::Encode { detail_for_log, .. } => detail_for_log,
            Self::Config { detail_for_log } => detail_for_log,
            Self::Timeout => "tokio::time::timeout fired",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_kind_groups_variants() {
        assert_eq!(
            EvmError::Transport {
                detail_for_log: "x".into()
            }
            .data_kind(),
            "evm_rpc_error"
        );
        assert_eq!(EvmError::Timeout.data_kind(), "evm_rpc_error");
        assert_eq!(
            EvmError::Config {
                detail_for_log: "x".into()
            }
            .data_kind(),
            "evm_rpc_error"
        );
        assert_eq!(
            EvmError::Decode {
                category: "abi_parse",
                detail_for_log: "x".into()
            }
            .data_kind(),
            "evm_decode_error"
        );
        assert_eq!(
            EvmError::Encode {
                category: "type_mismatch",
                detail_for_log: "x".into()
            }
            .data_kind(),
            "evm_decode_error"
        );
        assert_eq!(
            EvmError::Revert {
                reason: "unknown".into(),
                detail_for_log: "x".into()
            }
            .data_kind(),
            "evm_revert"
        );
    }

    #[test]
    fn display_strings_are_stable_and_wire_safe() {
        // Wire-safe means: NO raw transport text, NO addresses, NO bytes —
        // only the stable taxonomy prefix + (where applicable) a typed
        // category or decoded reason.
        let raw = "Reqwest::Error(connection refused)".to_string();
        let e = EvmError::Transport {
            detail_for_log: raw.clone(),
        };
        let wire = e.to_string();
        assert_eq!(wire, "evm rpc error: transport");
        assert!(!wire.contains("Reqwest"), "raw text leaked: {wire}");
        assert!(!wire.contains("connection refused"), "raw text leaked: {wire}");

        let e = EvmError::Decode {
            category: "abi_parse",
            detail_for_log: "JsonAbi: at line 1 col 0".into(),
        };
        assert_eq!(e.to_string(), "evm decode error: abi_parse");

        let e = EvmError::Revert {
            reason: "ERC20: insufficient balance".into(),
            detail_for_log: "0x08c379a0...".into(),
        };
        assert_eq!(
            e.to_string(),
            "evm revert: ERC20: insufficient balance"
        );

        assert_eq!(EvmError::Timeout.to_string(), "evm rpc error: timeout");
    }

    #[test]
    fn detail_for_log_returns_raw_text() {
        let e = EvmError::Transport {
            detail_for_log: "Reqwest::Error(boom)".into(),
        };
        assert_eq!(e.detail_for_log(), "Reqwest::Error(boom)");
        assert_eq!(EvmError::Timeout.detail_for_log(), "tokio::time::timeout fired");
    }
}
