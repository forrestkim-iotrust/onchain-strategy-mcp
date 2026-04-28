//! Phase 5 D-05 — anvil-gated integration tests for `simulate_one`.
//!
//! Skips cleanly (eprintln + early return) when `anvil` is not on PATH and
//! `ANVIL_RPC_URL` is unset. NEVER panics on missing anvil (D-14).
//!
//! Coverage:
//! - `simulate_pure_view_call_passes` — Counter.number() → Pass.
//! - `simulate_increment_counter_passes` — Counter.increment() (state-changing,
//!   but eth_call doesn't write) → Pass.
//! - `simulate_revert_returns_simulation_failure` — RevertCounter (always reverts)
//!   → Fail{Revert{decoded: sanitized}}.
//! - `simulate_unreachable_rpc_returns_transport_or_timeout` — closed port (no anvil)
//!   → Fail{Transport | Timeout}.

#![cfg(feature = "anvil-tests")]

mod common;

use std::sync::Arc;

use alloy::eips::BlockId;
use alloy::network::TransactionBuilder;
use alloy::providers::{DynProvider, Provider};
use alloy::rpc::types::TransactionRequest;
use alloy_primitives::{Address, Bytes};

use common::anvil_fixture::AnvilFixture;
use executor_evm::dyn_abi::encode_call_input;
use executor_evm::{
    EvmConfig, SimulationFailReason, SimulationOutcome, build_provider, simulate_one,
};

const COUNTER_ABI: &str = r#"[
    {"type":"function","name":"number","inputs":[],"outputs":[{"name":"","type":"uint256"}],"stateMutability":"view"},
    {"type":"function","name":"increment","inputs":[],"outputs":[],"stateMutability":"nonpayable"}
]"#;

const COUNTER_BYTECODE: &str = include_str!("fixtures/counter.hex");
const REVERT_BYTECODE: &str = include_str!("fixtures/revert_counter.hex");

/// Deploy bytecode from `deployer` (anvil account[0]). Mirrors Phase-4
/// `read_contract_anvil::deploy_counter`.
async fn deploy_bytecode(
    provider: &Arc<DynProvider>,
    deployer: Address,
    bytecode_hex: &str,
) -> Address {
    let stripped = bytecode_hex
        .trim()
        .strip_prefix("0x")
        .or_else(|| bytecode_hex.trim().strip_prefix("0X"))
        .unwrap_or(bytecode_hex.trim());
    let bytecode: Vec<u8> = (0..stripped.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&stripped[i..i + 2], 16).expect("hex"))
        .collect();
    let tx = TransactionRequest::default()
        .with_from(deployer)
        .with_deploy_code(bytecode);
    let pending = provider.send_transaction(tx).await.expect("send deploy tx");
    let receipt = pending.get_receipt().await.expect("deploy receipt");
    receipt
        .contract_address
        .expect("deploy receipt has contract_address")
}

#[tokio::test(flavor = "multi_thread")]
async fn simulate_pure_view_call_passes() {
    let Some(fixture) = AnvilFixture::try_spawn() else {
        return;
    };
    if fixture.funded_accounts.is_empty() {
        return;
    }
    let cfg = EvmConfig {
        rpc_url: fixture.rpc_url.clone(),
        ..EvmConfig::default()
    };
    let provider = build_provider(&cfg).expect("provider");
    let deployer = fixture.funded_accounts[0];

    let counter = deploy_bytecode(&provider, deployer, COUNTER_BYTECODE).await;
    let calldata = encode_call_input(COUNTER_ABI, "number", &[]).expect("encode number()");
    let tx = TransactionRequest::default()
        .with_to(counter)
        .with_input(calldata);

    let outcome = simulate_one(
        provider,
        &cfg,
        &tx,
        BlockId::latest(),
        Some(cfg.simulation_from),
    )
    .await;
    match outcome {
        SimulationOutcome::Pass {
            return_bytes,
            gas_estimate,
        } => {
            assert_eq!(return_bytes.len(), 32, "uint256 returns 32 bytes");
            assert_eq!(gas_estimate, None, "Phase 5 doesn't populate gas_estimate");
        }
        SimulationOutcome::Fail {
            reason,
            raw_for_log,
        } => panic!("expected Pass; got Fail({reason:?}) raw={raw_for_log}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn simulate_increment_counter_passes() {
    // increment() is a state-changing fn but eth_call doesn't write state;
    // it just returns Ok(empty_bytes). Simulation == "would this revert?".
    let Some(fixture) = AnvilFixture::try_spawn() else {
        return;
    };
    if fixture.funded_accounts.is_empty() {
        return;
    }
    let cfg = EvmConfig {
        rpc_url: fixture.rpc_url.clone(),
        ..EvmConfig::default()
    };
    let provider = build_provider(&cfg).expect("provider");
    let deployer = fixture.funded_accounts[0];

    let counter = deploy_bytecode(&provider, deployer, COUNTER_BYTECODE).await;
    let calldata = encode_call_input(COUNTER_ABI, "increment", &[]).expect("encode increment()");
    let tx = TransactionRequest::default()
        .with_to(counter)
        .with_input(calldata);

    let outcome = simulate_one(
        provider,
        &cfg,
        &tx,
        BlockId::latest(),
        Some(cfg.simulation_from),
    )
    .await;
    assert!(
        matches!(outcome, SimulationOutcome::Pass { .. }),
        "got {outcome:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn simulate_revert_returns_simulation_failure() {
    let Some(fixture) = AnvilFixture::try_spawn() else {
        return;
    };
    if fixture.funded_accounts.is_empty() {
        return;
    }
    let cfg = EvmConfig {
        rpc_url: fixture.rpc_url.clone(),
        ..EvmConfig::default()
    };
    let provider = build_provider(&cfg).expect("provider");
    let deployer = fixture.funded_accounts[0];

    let revert_addr = deploy_bytecode(&provider, deployer, REVERT_BYTECODE).await;
    // RevertCounter has no function dispatch — any calldata triggers REVERT.
    // Use a synthetic 4-byte selector (zeros) to trigger the revert.
    let tx = TransactionRequest::default()
        .with_to(revert_addr)
        .with_input(Bytes::from_static(&[0u8, 0, 0, 0]));

    let outcome = simulate_one(
        provider,
        &cfg,
        &tx,
        BlockId::latest(),
        Some(cfg.simulation_from),
    )
    .await;
    match outcome {
        SimulationOutcome::Fail {
            reason: SimulationFailReason::Revert { decoded },
            raw_for_log: _,
        } => {
            // WR-04 sanitization invariants: when alloy decoded the revert,
            // the reason MUST be control-char-free and capped at 256 bytes.
            if let Some(d) = decoded {
                assert!(d.len() <= 256, "sanitized reason capped at 256 bytes");
                assert!(
                    !d.chars().any(|c| c.is_control()),
                    "no control chars on the wire: {d:?}"
                );
            }
            // `decoded == None` is also acceptable — alloy may stringify the
            // revert without exposing an extractable Error(string) payload.
        }
        other => panic!("expected Fail{{Revert}}; got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn simulate_unreachable_rpc_returns_transport_or_timeout() {
    // No anvil needed — point at a closed port deliberately.
    let cfg = EvmConfig {
        rpc_url: "http://127.0.0.1:1".parse().unwrap(),
        call_timeout: std::time::Duration::from_millis(300),
        ..EvmConfig::default()
    };
    let provider = build_provider(&cfg).expect("provider");
    let tx = TransactionRequest::default()
        .with_to(Address::ZERO)
        .with_input(Bytes::new());
    let outcome = simulate_one(provider, &cfg, &tx, BlockId::latest(), None).await;
    match outcome {
        SimulationOutcome::Fail {
            reason: SimulationFailReason::Transport,
            ..
        }
        | SimulationOutcome::Fail {
            reason: SimulationFailReason::Timeout,
            ..
        } => {}
        other => panic!("expected Transport or Timeout; got {other:?}"),
    }
}
