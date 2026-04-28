//! Phase 5 D-05: per-action `eth_call` simulation adapter.
//!
//! `simulate_one` is an `async fn` that runs a single transaction request
//! through `provider.call(tx).block(block_id)` with a per-call
//! `tokio::time::timeout` (Phase 4 D-04 carry-forward) and produces a
//! [`SimulationOutcome`] that distinguishes Pass / Revert / Transport /
//! Timeout. The orchestrator (Plan 05-04) maps `Fail` outcomes to
//! `executor_mcp::errors::map_simulation_error` (-32017 STRATEGY_RUNTIME_ERROR
//! with `data.kind = "simulation_failure"`) and journal entries.
//!
//! Distinct from `read.rs::read_contract`: simulate does NOT decode the
//! return bytes (Phase 6 signer + Phase 7 examples may want the bytes
//! later, so we keep them in `Pass.return_bytes`). Revert reasons go through
//! [`crate::read::sanitize_revert_reason`] (D-19 promoted to `pub`) before
//! reaching the wire (WR-04 carry-forward).
//!
//! ## WR-01 carry-forward
//!
//! `simulate_one` is `async fn` (returns `impl Future<Output = SimulationOutcome>`).
//! The orchestrator (Plan 05-04) drives it via `Handle::current().block_on(...)`
//! from inside `spawn_blocking`. `simulate_one` itself NEVER calls
//! `tokio::task::block_in_place` — that anti-pattern is the WR-01 invariant.

use std::sync::Arc;

use alloy::eips::BlockId;
use alloy::providers::{DynProvider, Provider};
use alloy::rpc::types::TransactionRequest;
use alloy_primitives::{Address, Bytes};

use crate::EvmConfig;
use crate::read::{sanitize_revert_reason, try_extract_revert_reason};

/// Result of simulating a single transaction request via `eth_call`.
/// Pass / Fail are observable outcomes; the orchestrator (Plan 05-04)
/// decides whether each maps to gate-pass or gate-fail.
#[derive(Debug, Clone)]
pub enum SimulationOutcome {
    /// The eth_call returned successfully. `return_bytes` carries the raw
    /// ABI-encoded return value (caller decodes if needed). `gas_estimate`
    /// is `None` in Phase 5 — Phase 6 owns gas estimation via a separate
    /// `eth_estimateGas` call.
    Pass {
        return_bytes: Bytes,
        gas_estimate: Option<u64>,
    },
    /// The eth_call failed. `reason` is the wire-safe taxonomy enum;
    /// `raw_for_log` carries the raw alloy error text and is intended for
    /// `tracing::warn!` ONLY (MR-01 — never on the wire).
    Fail {
        reason: SimulationFailReason,
        raw_for_log: String,
    },
}

/// Why simulation reported a fail. Mirrors RESEARCH §"Failure-modes mapping".
///
/// Variants intentionally do NOT carry the raw alloy error string — that lives
/// in [`SimulationOutcome::Fail::raw_for_log`] and stays out of the wire shape.
#[derive(Debug, Clone)]
pub enum SimulationFailReason {
    /// Contract reverted. `decoded` is the SANITIZED `Error(string)` reason
    /// when alloy could decode it (WR-04 via `sanitize_revert_reason`); `None`
    /// when alloy stringified the revert without an extractable payload.
    Revert { decoded: Option<String> },
    /// RPC transport failed (anvil down, HTTP 5xx, connection refused).
    Transport,
    /// `tokio::time::timeout` fired before `provider.call` resolved.
    Timeout,
}

/// Simulate a single [`TransactionRequest`] at `block`. The `from` argument
/// SHOULD be `Some(EvmConfig.simulation_from)` to avoid the `msg.sender = 0x0`
/// pitfall (RESEARCH P-1 — alloy's `Provider::call` defaults `from` to ZERO,
/// which causes ERC20 / ownership checks to misbehave on `eth_call`).
///
/// Per-call timeout reuses Phase 4 D-04 `EvmConfig::call_timeout` (default 1s,
/// range 50ms..30s). On timeout, returns `Fail { Timeout }`.
pub async fn simulate_one(
    provider: Arc<DynProvider>,
    cfg: &EvmConfig,
    tx: &TransactionRequest,
    block: BlockId,
    from: Option<Address>,
) -> SimulationOutcome {
    // 1. Apply `from` if provided (P-1 mitigation). Clone is cheap —
    //    TransactionRequest is a thin builder struct.
    let mut tx_with_from = tx.clone();
    if let Some(f) = from {
        tx_with_from.from = Some(f);
    }

    // 2. Per-call timeout (D-04 carry-forward).
    let call_future = provider.call(tx_with_from).block(block);
    let timeout_result = tokio::time::timeout(cfg.call_timeout, call_future).await;

    match timeout_result {
        Ok(Ok(bytes)) => SimulationOutcome::Pass {
            return_bytes: bytes,
            gas_estimate: None, // Phase 6 owns gas estimation.
        },
        Ok(Err(e)) => {
            let raw = e.to_string();
            // Reuse Phase-4 classification heuristics: an alloy transport error
            // whose stringified form embeds the standard `Error(string)`
            // selector or "execution reverted" is a contract revert; anything
            // else is treated as transport.
            if let Some(decoded) = try_extract_revert_reason(&raw) {
                SimulationOutcome::Fail {
                    reason: SimulationFailReason::Revert {
                        decoded: Some(sanitize_revert_reason(&decoded)),
                    },
                    raw_for_log: raw,
                }
            } else if looks_like_revert(&raw) {
                SimulationOutcome::Fail {
                    reason: SimulationFailReason::Revert { decoded: None },
                    raw_for_log: raw,
                }
            } else {
                SimulationOutcome::Fail {
                    reason: SimulationFailReason::Transport,
                    raw_for_log: raw,
                }
            }
        }
        Err(_elapsed) => SimulationOutcome::Fail {
            reason: SimulationFailReason::Timeout,
            raw_for_log: "tokio::time::timeout fired".into(),
        },
    }
}

/// Phase 5 D-07 — convenience wrapper around [`simulate_one`] that pins the
/// block tag to `latest`. Lets the orchestrator (executor-mcp) stay alloy-free
/// by avoiding a direct `alloy::eips::BlockId` import.
pub async fn simulate_one_latest(
    provider: Arc<DynProvider>,
    cfg: &EvmConfig,
    tx: &TransactionRequest,
    from: Option<Address>,
) -> SimulationOutcome {
    simulate_one(provider, cfg, tx, BlockId::latest(), from).await
}

/// Heuristic: does this raw alloy error string look like a contract revert?
///
/// Used as a defensive fallback when `try_extract_revert_reason` cannot find
/// an `Error(string)` payload but the error is clearly a revert (e.g. anvil
/// surfaces `"execution reverted"` without an embedded `0x08c379a0` blob).
fn looks_like_revert(raw: &str) -> bool {
    let r = raw.to_ascii_lowercase();
    r.contains("execution reverted") || r.contains("revert") || r.contains("0x08c379a0")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send<T: Send>() {}

    #[test]
    fn simulation_outcome_is_send() {
        assert_send::<SimulationOutcome>();
        assert_send::<SimulationFailReason>();
    }

    #[test]
    fn simulate_one_signature_compiles() {
        // Compile-time check that the signature has the shape Plan 05-04 expects.
        #[allow(dead_code, clippy::manual_async_fn)]
        fn _shape_check(
            provider: Arc<DynProvider>,
            cfg: &'static EvmConfig,
            tx: &'static TransactionRequest,
        ) -> impl std::future::Future<Output = SimulationOutcome> + Send {
            simulate_one(provider, cfg, tx, BlockId::latest(), None)
        }
    }

    #[test]
    fn sanitize_revert_reason_is_pub_callable_from_simulate() {
        // D-19 visibility verification — the call would not compile if
        // `sanitize_revert_reason` were still `pub(crate)`.
        let s = crate::read::sanitize_revert_reason("\x1bbad\nstuff");
        assert!(!s.contains('\x1b'));
        assert!(!s.contains('\n'));
        assert_eq!(s, "badstuff");
    }

    #[test]
    fn looks_like_revert_matches_known_phrasings() {
        assert!(looks_like_revert(
            "execution reverted: ERC20: insufficient balance"
        ));
        assert!(looks_like_revert(
            "VM Exception while processing transaction: revert"
        ));
        assert!(looks_like_revert("returndata: 0x08c379a0..."));
        assert!(!looks_like_revert("connection refused"));
        assert!(!looks_like_revert("HTTP 502 Bad Gateway"));
    }

    #[test]
    fn pass_constructor_round_trips() {
        let outcome = SimulationOutcome::Pass {
            return_bytes: Bytes::from_static(&[1, 2, 3]),
            gas_estimate: None,
        };
        match outcome {
            SimulationOutcome::Pass {
                return_bytes,
                gas_estimate,
            } => {
                assert_eq!(return_bytes.as_ref(), &[1u8, 2, 3]);
                assert_eq!(gas_estimate, None);
            }
            other => panic!("expected Pass, got {other:?}"),
        }
    }

    #[test]
    fn fail_revert_carries_sanitized_decoded() {
        let outcome = SimulationOutcome::Fail {
            reason: SimulationFailReason::Revert {
                decoded: Some(sanitize_revert_reason("ERC20:\ninsufficient")),
            },
            raw_for_log: "raw alloy text".into(),
        };
        match outcome {
            SimulationOutcome::Fail {
                reason: SimulationFailReason::Revert { decoded: Some(d) },
                ..
            } => {
                assert!(!d.contains('\n'));
                assert_eq!(d, "ERC20:insufficient");
            }
            other => panic!("expected Revert with decoded, got {other:?}"),
        }
    }
}
