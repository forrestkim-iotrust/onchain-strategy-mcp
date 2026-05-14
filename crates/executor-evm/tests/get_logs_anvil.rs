//! Anvil-gated integration tests for `get_logs`.
//!
//! Mirrors the skip-cleanly contract of `read_contract_anvil.rs` (D-14):
//! `eprintln!` + early return when `anvil` is missing.
//!
//! The fixture re-uses `tests/fixtures/erc20.hex` (MockERC20 with constructor
//! `uint256 initialSupply`). Deploying the token emits one `Transfer(0x0 →
//! deployer, initialSupply)` log, which is enough to assert `get_logs`
//! returns a sane row.

#![cfg(feature = "anvil-tests")]

mod common;

use std::str::FromStr;
use std::sync::Arc;

use alloy::network::TransactionBuilder;
use alloy::providers::Provider;
use alloy::rpc::types::TransactionRequest;
use alloy_primitives::{Address, U256, keccak256};

use common::anvil_fixture::AnvilFixture;
use executor_evm::read::{GetLogsInput, LogBlockTag, TopicSlot};
use executor_evm::{EvmConfig, build_provider, get_logs, parse_b256};

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
async fn get_logs_returns_transfer_emitted_on_deploy() {
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

    // Transfer(address,address,uint256) topic0.
    let transfer_topic = keccak256(b"Transfer(address,address,uint256)");
    let topic_hex = format!("0x{}", hex_encode_lower(transfer_topic.as_slice()));

    let input = GetLogsInput {
        addresses: vec![format!("{token:?}")],
        from_block: LogBlockTag::Earliest,
        to_block: LogBlockTag::Latest,
        topics: vec![TopicSlot::One(parse_b256(&topic_hex).unwrap())],
    };
    let result = get_logs(provider.clone(), &cfg, input).await.expect("get_logs ok");
    let arr = result.as_array().expect("logs array");
    assert!(
        !arr.is_empty(),
        "expected at least the constructor Transfer log, got {arr:?}"
    );
    // Verify the shape contract for the first row.
    let row = &arr[0];
    assert_eq!(
        row["address"].as_str().unwrap().to_lowercase(),
        format!("{token:?}").to_lowercase()
    );
    let topics = row["topics"].as_array().expect("topics array");
    assert_eq!(topics[0].as_str().unwrap(), topic_hex);
    // topic1 = from (zero address, indexed)
    assert!(topics.len() >= 3, "Transfer has 3 indexed topics");
    let topic1 = topics[1].as_str().unwrap();
    // from = 0x000…0 (mint = mint-from-zero)
    assert!(
        topic1.ends_with(&"0".repeat(40)),
        "topic1 should encode the zero address for a mint: {topic1}"
    );
    // data is hex-prefixed and even-length
    let data = row["data"].as_str().expect("data string");
    assert!(data.starts_with("0x"));
    assert!(data.len() % 2 == 0);
    assert_eq!(row["removed"].as_bool().unwrap(), false);
    let _block_no = row["blockNumber"].as_u64().expect("blockNumber u64");
    let _log_index = row["logIndex"].as_u64().expect("logIndex u64");
    let tx_hash = row["txHash"].as_str().expect("txHash hex");
    assert!(tx_hash.starts_with("0x") && tx_hash.len() == 2 + 64);
}

#[tokio::test(flavor = "multi_thread")]
async fn get_logs_topic_mismatch_returns_empty_array() {
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

    // A topic that is NOT the Transfer signature — no logs should match.
    let bogus =
        "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef".to_string();
    let input = GetLogsInput {
        addresses: vec![format!("{token:?}")],
        from_block: LogBlockTag::Earliest,
        to_block: LogBlockTag::Latest,
        topics: vec![TopicSlot::One(parse_b256(&bogus).unwrap())],
    };
    let result = get_logs(provider, &cfg, input).await.expect("get_logs ok");
    let arr = result.as_array().expect("array");
    assert!(
        arr.is_empty(),
        "expected zero matches for bogus topic, got {arr:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn get_logs_multi_address_or_filter_works() {
    let Some(fixture) = AnvilFixture::try_spawn() else {
        return;
    };
    if fixture.funded_accounts.is_empty() {
        return;
    }
    let cfg = cfg_for(&fixture.rpc_url);
    let provider = build_provider(&cfg).expect("provider");
    let deployer = fixture.funded_accounts[0];
    let token_a = deploy_erc20(&provider, deployer).await;
    let token_b = deploy_erc20(&provider, deployer).await;

    let transfer_topic = keccak256(b"Transfer(address,address,uint256)");
    let topic_hex = format!("0x{}", hex_encode_lower(transfer_topic.as_slice()));
    let input = GetLogsInput {
        addresses: vec![format!("{token_a:?}"), format!("{token_b:?}")],
        from_block: LogBlockTag::Earliest,
        to_block: LogBlockTag::Latest,
        topics: vec![TopicSlot::One(parse_b256(&topic_hex).unwrap())],
    };
    let result = get_logs(provider, &cfg, input).await.expect("get_logs ok");
    let arr = result.as_array().expect("array");
    // Each token mints once on deploy → expect ≥ 2 rows.
    assert!(arr.len() >= 2, "expected ≥2 Transfer rows, got {arr:?}");
    // Both addresses should appear.
    let lowers: Vec<String> = arr
        .iter()
        .filter_map(|r| r["address"].as_str().map(|s| s.to_lowercase()))
        .collect();
    let a_lower = format!("{token_a:?}").to_lowercase();
    let b_lower = format!("{token_b:?}").to_lowercase();
    assert!(lowers.contains(&a_lower), "missing token_a in {lowers:?}");
    assert!(lowers.contains(&b_lower), "missing token_b in {lowers:?}");
    // Sanity-check the parsed Address shape we produced for the assertion.
    let _ = Address::from_str(&a_lower).expect("token_a parseable");
}

fn hex_encode_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
}
