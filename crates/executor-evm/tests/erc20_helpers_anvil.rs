//! Anvil-gated integration tests for the ERC20 read helpers.
//!
//! Skips cleanly (eprintln + early return) when `anvil` is not on PATH and
//! `ANVIL_RPC_URL` is unset. NEVER panics on missing anvil (D-14).
//!
//! The fixture (`tests/fixtures/erc20.hex`) is a MockERC20 whose constructor
//! takes one `uint256 initialSupply`. The deploy helper appends a 32-byte
//! abi-encoded constructor argument to the initcode before sending the tx.

#![cfg(feature = "anvil-tests")]

mod common;

use std::sync::Arc;

use alloy::network::TransactionBuilder;
use alloy::providers::Provider;
use alloy::rpc::types::TransactionRequest;
use alloy_primitives::{Address, U256};
use serde_json::json;

use common::anvil_fixture::AnvilFixture;
use executor_evm::erc20::{
    erc20_allowance, erc20_balance_of, erc20_decimals, erc20_name, erc20_symbol,
    erc20_total_supply,
};
use executor_evm::read::BlockTag;
use executor_evm::{EvmConfig, build_provider};

/// Initial supply baked into deploy helpers: 1_000_000 * 10^18.
const INITIAL_SUPPLY_DECIMAL: &str = "1000000000000000000000000";

const ERC20_BYTECODE: &str = include_str!("fixtures/erc20.hex");

async fn deploy_erc20(
    provider: &Arc<alloy::providers::DynProvider>,
    deployer: Address,
) -> Address {
    let bytecode_hex = ERC20_BYTECODE.trim();
    let stripped = bytecode_hex
        .strip_prefix("0x")
        .or_else(|| bytecode_hex.strip_prefix("0X"))
        .unwrap_or(bytecode_hex);
    let mut bytecode: Vec<u8> = (0..stripped.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&stripped[i..i + 2], 16).expect("hex"))
        .collect();
    // Append the 32-byte abi-encoded constructor arg `initialSupply`.
    let supply: U256 = INITIAL_SUPPLY_DECIMAL.parse().unwrap();
    bytecode.extend_from_slice(&supply.to_be_bytes::<32>());

    let tx = TransactionRequest::default()
        .with_from(deployer)
        .with_deploy_code(bytecode);
    let pending = provider.send_transaction(tx).await.expect("deploy send");
    let receipt = pending.get_receipt().await.expect("deploy receipt");
    receipt
        .contract_address
        .expect("deploy receipt has contract_address")
}

fn cfg_for(rpc_url: &url::Url) -> EvmConfig {
    EvmConfig {
        rpc_url: rpc_url.clone(),
        ..EvmConfig::default()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn erc20_balance_of_returns_initial_supply_for_deployer() {
    let Some(fixture) = AnvilFixture::try_spawn() else {
        return;
    };
    if fixture.funded_accounts.is_empty() {
        return;
    }
    let cfg = cfg_for(&fixture.rpc_url);
    let provider = build_provider(&cfg).expect("provider");
    let deployer = fixture.funded_accounts[0];
    let token = deploy_erc20(&provider, deployer).await;

    let bal = erc20_balance_of(
        provider.clone(),
        &cfg,
        &format!("{token:?}"),
        &format!("{deployer:?}"),
        BlockTag::Latest,
    )
    .await
    .expect("balanceOf ok");
    assert_eq!(bal, json!(INITIAL_SUPPLY_DECIMAL));
}

#[tokio::test(flavor = "multi_thread")]
async fn erc20_decimals_returns_18() {
    let Some(fixture) = AnvilFixture::try_spawn() else {
        return;
    };
    if fixture.funded_accounts.is_empty() {
        return;
    }
    let cfg = cfg_for(&fixture.rpc_url);
    let provider = build_provider(&cfg).expect("provider");
    let deployer = fixture.funded_accounts[0];
    let token = deploy_erc20(&provider, deployer).await;

    let dec = erc20_decimals(
        provider.clone(),
        &cfg,
        &format!("{token:?}"),
        BlockTag::Latest,
    )
    .await
    .expect("decimals ok");
    // uint8 → JSON Number per D-03 (≤32-bit width).
    assert_eq!(dec, json!(18));
}

#[tokio::test(flavor = "multi_thread")]
async fn erc20_symbol_and_name_return_strings() {
    let Some(fixture) = AnvilFixture::try_spawn() else {
        return;
    };
    if fixture.funded_accounts.is_empty() {
        return;
    }
    let cfg = cfg_for(&fixture.rpc_url);
    let provider = build_provider(&cfg).expect("provider");
    let deployer = fixture.funded_accounts[0];
    let token = deploy_erc20(&provider, deployer).await;

    let sym = erc20_symbol(
        provider.clone(),
        &cfg,
        &format!("{token:?}"),
        BlockTag::Latest,
    )
    .await
    .expect("symbol ok");
    let nm = erc20_name(
        provider.clone(),
        &cfg,
        &format!("{token:?}"),
        BlockTag::Latest,
    )
    .await
    .expect("name ok");
    assert_eq!(sym, json!("MOCK"));
    assert_eq!(nm, json!("MockToken"));
}

#[tokio::test(flavor = "multi_thread")]
async fn erc20_total_supply_matches_balance_of_deployer() {
    let Some(fixture) = AnvilFixture::try_spawn() else {
        return;
    };
    if fixture.funded_accounts.is_empty() {
        return;
    }
    let cfg = cfg_for(&fixture.rpc_url);
    let provider = build_provider(&cfg).expect("provider");
    let deployer = fixture.funded_accounts[0];
    let token = deploy_erc20(&provider, deployer).await;

    let supply = erc20_total_supply(
        provider.clone(),
        &cfg,
        &format!("{token:?}"),
        BlockTag::Latest,
    )
    .await
    .expect("totalSupply ok");
    let bal = erc20_balance_of(
        provider.clone(),
        &cfg,
        &format!("{token:?}"),
        &format!("{deployer:?}"),
        BlockTag::Latest,
    )
    .await
    .expect("balanceOf ok");
    assert_eq!(supply, bal);
    assert_eq!(supply, json!(INITIAL_SUPPLY_DECIMAL));
}

#[tokio::test(flavor = "multi_thread")]
async fn erc20_allowance_returns_zero_for_unapproved_spender() {
    let Some(fixture) = AnvilFixture::try_spawn() else {
        return;
    };
    if fixture.funded_accounts.is_empty() {
        return;
    }
    let cfg = cfg_for(&fixture.rpc_url);
    let provider = build_provider(&cfg).expect("provider");
    let deployer = fixture.funded_accounts[0];
    let token = deploy_erc20(&provider, deployer).await;
    let spender: Address = "0x000000000000000000000000000000000000dEaD"
        .parse()
        .unwrap();

    let allow = erc20_allowance(
        provider.clone(),
        &cfg,
        &format!("{token:?}"),
        &format!("{deployer:?}"),
        &format!("{spender:?}"),
        BlockTag::Latest,
    )
    .await
    .expect("allowance ok");
    assert_eq!(allow, json!("0"));
}

#[tokio::test(flavor = "multi_thread")]
async fn erc20_balance_of_against_eoa_surfaces_decode_or_revert() {
    // Calling balanceOf against an address with no code → decode error
    // (empty bytes can't decode uint256) OR revert (some clients return
    // a JSON-RPC error). Both are wire-safe taxonomies.
    let Some(fixture) = AnvilFixture::try_spawn() else {
        return;
    };
    let cfg = cfg_for(&fixture.rpc_url);
    let provider = build_provider(&cfg).expect("provider");
    let err = erc20_balance_of(
        provider.clone(),
        &cfg,
        "0x000000000000000000000000000000000000dEaD",
        "0x0000000000000000000000000000000000000001",
        BlockTag::Latest,
    )
    .await
    .unwrap_err();
    let kind = err.data_kind();
    assert!(
        matches!(kind, "evm_decode_error" | "evm_revert"),
        "expected decode/revert, got {kind} ({err:?})"
    );
}
