//! Phase 5 D-05 — `simulate_one` per-call timeout regression.
//!
//! No anvil required: this test points at a closed port (`http://127.0.0.1:1`)
//! with a tight `call_timeout` (200ms) and asserts the outcome is `Fail` with
//! either `Timeout` (timer fired first) or `Transport` (OS surfaced
//! connection-refused before the timer). Both are acceptable — both lead to
//! deny-signing per EXE-04.
//!
//! The wall-clock budget assertion (<2s) proves the per-call timeout safety
//! net is in place — without it, a hung RPC could stall a strategy run past
//! the Phase-3 wall-clock 2s envelope.

use std::time::{Duration, Instant};

use alloy::eips::BlockId;
use alloy::network::TransactionBuilder;
use alloy::rpc::types::TransactionRequest;
use alloy_primitives::{Address, Bytes};

use executor_evm::{
    EvmConfig, SimulationFailReason, SimulationOutcome, build_provider, simulate_one,
};

#[tokio::test(flavor = "multi_thread")]
async fn simulate_timeout_fires_when_rpc_unreachable() {
    let cfg = EvmConfig {
        rpc_url: "http://127.0.0.1:1".parse().unwrap(),
        call_timeout: Duration::from_millis(200),
        ..EvmConfig::default()
    };

    let provider = build_provider(&cfg).expect("provider builds against unreachable URL");
    let tx = TransactionRequest::default()
        .with_to(Address::ZERO)
        .with_input(Bytes::new());

    let started = Instant::now();
    let outcome = simulate_one(
        provider,
        &cfg,
        &tx,
        BlockId::latest(),
        Some(cfg.simulation_from),
    )
    .await;
    let elapsed = started.elapsed();

    // The per-call timeout safety net MUST cap wall-clock at well under
    // the Phase-3 2s envelope (default 2s). 1.5s gives plenty of slop.
    assert!(
        elapsed < Duration::from_millis(1_500),
        "simulate_one hung past timeout: {elapsed:?}",
    );
    match outcome {
        SimulationOutcome::Fail {
            reason: SimulationFailReason::Timeout,
            ..
        }
        | SimulationOutcome::Fail {
            reason: SimulationFailReason::Transport,
            ..
        } => {}
        other => panic!(
            "expected Fail{{Timeout | Transport}}; got {other:?} (elapsed={elapsed:?})"
        ),
    }
}
