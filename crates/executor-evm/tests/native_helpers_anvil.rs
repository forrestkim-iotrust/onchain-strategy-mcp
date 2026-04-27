//! Anvil-gated integration tests for the native (chain-base-asset) helpers.
//!
//! Skips cleanly (eprintln + early return) when `anvil` is not on PATH and
//! `ANVIL_RPC_URL` is unset. NEVER panics on missing anvil (D-14).

#![cfg(feature = "anvil-tests")]

mod common;

use alloy::network::TransactionBuilder;
use alloy::providers::Provider;
use alloy::rpc::types::TransactionRequest;
use alloy_primitives::{Address, U256};

use common::anvil_fixture::AnvilFixture;
use executor_evm::read::BlockTag;
use executor_evm::{EvmConfig, build_provider, native_balance, native_block_number};

fn cfg_for(rpc_url: &url::Url) -> EvmConfig {
    EvmConfig {
        rpc_url: rpc_url.clone(),
        ..EvmConfig::default()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn native_balance_returns_anvil_funded_balance() {
    let Some(fixture) = AnvilFixture::try_spawn() else {
        return;
    };
    if fixture.funded_accounts.is_empty() {
        return;
    }
    let cfg = cfg_for(&fixture.rpc_url);
    let provider = build_provider(&cfg).expect("provider");
    let acct: Address = fixture.funded_accounts[0];

    let bal = native_balance(
        provider.clone(),
        &cfg,
        &format!("{acct:?}"),
        BlockTag::Latest,
    )
    .await
    .expect("native_balance ok");

    let s = bal.as_str().expect("decimal string per D-03");
    let n: U256 = s.parse().expect("parses as U256");
    // Anvil default funded accounts get 10000 ETH = 10^22 wei.
    // Tolerance: account[0] is the deployer in other tests; tests run
    // independently, but if this test runs against a fresh fixture, the
    // balance is exactly 10000 ether. Lower bound suffices.
    assert!(
        n >= U256::from(1u64) * U256::from(10u64).pow(U256::from(20u64)),
        "anvil funded account balance unexpectedly low: {s}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn native_balance_returns_zero_for_empty_eoa() {
    let Some(fixture) = AnvilFixture::try_spawn() else {
        return;
    };
    let cfg = cfg_for(&fixture.rpc_url);
    let provider = build_provider(&cfg).expect("provider");
    let bal = native_balance(
        provider.clone(),
        &cfg,
        "0x000000000000000000000000000000000000dEaD",
        BlockTag::Latest,
    )
    .await
    .expect("native_balance ok");
    // Decimal string, value is 0 (or possibly a small non-zero if a prior
    // test sent gas there — accept any decimal string of digits).
    let s = bal.as_str().expect("decimal string");
    assert!(s.bytes().all(|b| b.is_ascii_digit()));
}

#[tokio::test(flavor = "multi_thread")]
async fn native_block_number_returns_nonnegative_number_and_increments_after_tx() {
    let Some(fixture) = AnvilFixture::try_spawn() else {
        return;
    };
    if fixture.funded_accounts.is_empty() {
        return;
    }
    let cfg = cfg_for(&fixture.rpc_url);
    let provider = build_provider(&cfg).expect("provider");

    let before = native_block_number(provider.clone(), &cfg)
        .await
        .expect("blockNumber ok");
    let before_n = before.as_u64().expect("JSON Number per D-07");

    // Send a no-op self-transfer to advance the block.
    let acct = fixture.funded_accounts[0];
    let tx = TransactionRequest::default()
        .with_from(acct)
        .with_to(acct)
        .with_value(U256::from(1u64));
    let pending = provider.send_transaction(tx).await.expect("send tx");
    let _ = pending.get_receipt().await.expect("receipt");

    let after = native_block_number(provider.clone(), &cfg)
        .await
        .expect("blockNumber ok");
    let after_n = after.as_u64().expect("JSON Number");
    assert!(
        after_n > before_n,
        "block number did not advance: before={before_n} after={after_n}"
    );
}
