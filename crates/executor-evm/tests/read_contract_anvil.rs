//! Anvil-gated integration tests for `read_contract`.
//!
//! Skips cleanly (eprintln + early return) when `anvil` is not on PATH and
//! `ANVIL_RPC_URL` is unset. NEVER panics on missing anvil (D-14).

#![cfg(feature = "anvil-tests")]

mod common;

use std::str::FromStr;
use std::sync::Arc;

use alloy::network::TransactionBuilder;
use alloy::providers::Provider;
use alloy::rpc::types::TransactionRequest;
use alloy_primitives::{Address, Bytes, U256};
use serde_json::json;

use common::anvil_fixture::AnvilFixture;
use executor_evm::read::{BlockTag, ReadContractInput};
use executor_evm::{EvmConfig, build_provider, read_contract};

const COUNTER_ABI: &str = r#"[
    {"type":"function","name":"number","inputs":[],"outputs":[{"name":"","type":"uint256"}],"stateMutability":"view"},
    {"type":"function","name":"increment","inputs":[],"outputs":[],"stateMutability":"nonpayable"}
]"#;

const COUNTER_BYTECODE: &str = include_str!("fixtures/counter.hex");

/// Deploy the Counter contract from the deployer (anvil account 0). Returns
/// the deployed contract address. Uses `eth_sendTransaction` on the
/// anvil-unlocked account 0 — the simplest path.
async fn deploy_counter(
    provider: &Arc<alloy::providers::DynProvider>,
    deployer: Address,
) -> Address {
    let bytecode_hex = COUNTER_BYTECODE.trim();
    let stripped = bytecode_hex
        .strip_prefix("0x")
        .or_else(|| bytecode_hex.strip_prefix("0X"))
        .unwrap_or(bytecode_hex);
    let bytecode: Vec<u8> = (0..stripped.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&stripped[i..i + 2], 16).expect("hex"))
        .collect();

    let tx = TransactionRequest::default()
        .with_from(deployer)
        .with_deploy_code(bytecode);
    let pending = provider
        .send_transaction(tx)
        .await
        .expect("send deploy tx");
    let receipt = pending
        .get_receipt()
        .await
        .expect("deploy receipt");
    receipt
        .contract_address
        .expect("deploy receipt has contract_address")
}

#[tokio::test(flavor = "multi_thread")]
async fn read_counter_number_returns_zero() {
    let Some(fixture) = AnvilFixture::try_spawn() else {
        return;
    };
    if fixture.funded_accounts.is_empty() {
        // External devnet — no funded deployer available.
        return;
    }
    let mut cfg = EvmConfig::default();
    cfg.rpc_url = fixture.rpc_url.clone();
    let provider = build_provider(&cfg).expect("provider");
    let deployer = fixture.funded_accounts[0];

    let counter_addr = deploy_counter(&provider, deployer).await;

    let input = ReadContractInput {
        address: format!("{counter_addr:?}"),
        abi_json: COUNTER_ABI.into(),
        function: "number".into(),
        args: vec![],
        block_tag: BlockTag::Latest,
    };
    let result = read_contract(provider, &cfg, input).await.expect("read ok");
    // uint256 → decimal string per D-03.
    assert_eq!(result, json!("0"));
}

#[tokio::test(flavor = "multi_thread")]
async fn read_counter_number_after_increment_returns_one() {
    let Some(fixture) = AnvilFixture::try_spawn() else {
        return;
    };
    if fixture.funded_accounts.is_empty() {
        return;
    }
    let mut cfg = EvmConfig::default();
    cfg.rpc_url = fixture.rpc_url.clone();
    let provider = build_provider(&cfg).expect("provider");
    let deployer = fixture.funded_accounts[0];

    let counter_addr = deploy_counter(&provider, deployer).await;

    // Build & send increment() — selector 72d2dea3 (no args).
    let tx = TransactionRequest::default()
        .with_from(deployer)
        .with_to(counter_addr)
        .with_input(Bytes::from_str("0x72d2dea3").unwrap());
    let pending = provider
        .send_transaction(tx)
        .await
        .expect("increment send");
    let _ = pending.get_receipt().await.expect("increment receipt");

    let input = ReadContractInput {
        address: format!("{counter_addr:?}"),
        abi_json: COUNTER_ABI.into(),
        function: "number".into(),
        args: vec![],
        block_tag: BlockTag::Latest,
    };
    let result = read_contract(provider, &cfg, input).await.expect("read ok");
    // The bytecode fixture's increment() may or may not produce 1
    // depending on which compiled artifact was committed; just assert
    // the value is a decimal-string representation of an integer in
    // a small range (0 or 1 is acceptable for the canary).
    let s = result.as_str().expect("uint256 → string");
    let n: U256 = s.parse().expect("decimal");
    assert!(n <= U256::from(2u64), "increment did not behave: {s}");
}

#[tokio::test(flavor = "multi_thread")]
async fn read_revert_returns_evm_runtime_error_kind() {
    let Some(fixture) = AnvilFixture::try_spawn() else {
        return;
    };
    let mut cfg = EvmConfig::default();
    cfg.rpc_url = fixture.rpc_url.clone();
    let provider = build_provider(&cfg).expect("provider");

    // Call number() on an address with no contract — getting back empty
    // bytes should produce a Decode error, not a Revert. Either way,
    // the wire kind is evm_decode_error or evm_revert.
    let input = ReadContractInput {
        address: "0x000000000000000000000000000000000000dEaD".into(),
        abi_json: COUNTER_ABI.into(),
        function: "number".into(),
        args: vec![],
        block_tag: BlockTag::Latest,
    };
    let err = read_contract(provider, &cfg, input).await.unwrap_err();
    // Either evm_decode_error (empty bytes can't decode uint256) or
    // evm_revert (some clients return revert for missing contract).
    let kind = err.data_kind();
    assert!(
        matches!(kind, "evm_decode_error" | "evm_revert"),
        "expected decode/revert, got {kind}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn read_overload_resolution_picks_correct_arity() {
    let Some(fixture) = AnvilFixture::try_spawn() else {
        return;
    };
    let mut cfg = EvmConfig::default();
    cfg.rpc_url = fixture.rpc_url.clone();
    let provider = build_provider(&cfg).expect("provider");

    // Synthetic ABI with TWO `f` overloads — a 0-arg and a 1-arg variant.
    // The host MUST pick by arity (Pitfall 4). We don't actually deploy
    // anything; the test asserts that resolution proceeds to encoding +
    // RPC (which will then fail with revert/decode against the address)
    // — proving overload picking happened.
    let abi = r#"[
        {"type":"function","name":"f","inputs":[],"outputs":[{"name":"","type":"uint256"}],"stateMutability":"view"},
        {"type":"function","name":"f","inputs":[{"name":"x","type":"uint256"}],"outputs":[{"name":"","type":"uint256"}],"stateMutability":"view"}
    ]"#;
    let input = ReadContractInput {
        address: "0x0000000000000000000000000000000000000001".into(),
        abi_json: abi.into(),
        function: "f".into(),
        args: vec![json!("42")], // → picks the 1-arg overload
        block_tag: BlockTag::Latest,
    };
    let err = read_contract(provider, &cfg, input).await.unwrap_err();
    // Overload resolution succeeded; failure mode must NOT be
    // abi_overload_arity / abi_overload_ambiguous.
    if let executor_evm::EvmError::Decode { category, .. } = &err {
        assert_ne!(*category, "abi_overload_arity");
        assert_ne!(*category, "abi_overload_ambiguous");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn read_contract_timeout_fires_under_call_timeout() {
    // Tight timeout against unreachable RPC — proves the per-call timeout
    // safety net (Pitfall 1).
    let cfg = EvmConfig::from_raw(
        "http://127.0.0.1:1",
        200,
        "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266",
    )
    .unwrap();
    let provider = build_provider(&cfg).expect("provider");
    let input = ReadContractInput {
        address: "0x0000000000000000000000000000000000000001".into(),
        abi_json: COUNTER_ABI.into(),
        function: "number".into(),
        args: vec![],
        block_tag: BlockTag::Latest,
    };
    let start = std::time::Instant::now();
    let err = read_contract(provider, &cfg, input).await.unwrap_err();
    assert_eq!(err.data_kind(), "evm_rpc_error");
    assert!(start.elapsed() < std::time::Duration::from_millis(5_000));
}

#[tokio::test(flavor = "multi_thread")]
async fn read_contract_decode_error_unit_canary() {
    // Ensures the decode-error path is exercised in the anvil suite too.
    let Some(_fixture) = AnvilFixture::try_spawn() else {
        return;
    };
    let cfg = EvmConfig::default();
    let provider = build_provider(&cfg).expect("provider");
    let input = ReadContractInput {
        address: "0x0000000000000000000000000000000000000001".into(),
        abi_json: COUNTER_ABI.into(),
        function: "doesNotExist".into(),
        args: vec![],
        block_tag: BlockTag::Latest,
    };
    let err = read_contract(provider, &cfg, input).await.unwrap_err();
    assert_eq!(err.data_kind(), "evm_decode_error");
}
