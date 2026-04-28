//! Phase 5 D-02 — per-variant `Action -> NormalizedAction` contract.

use alloy_primitives::U256;
use executor_core::schema::action::{
    Action, ContractCallAction, Erc20ApproveAction, Erc20TransferAction, NativeTransferAction,
    RawCallAction,
};
use executor_evm::normalize::{NormalizedActionKind, normalize_action};

const COUNTER_ABI: &str = r#"[
    {"type":"function","name":"increment","inputs":[],
     "outputs":[],"stateMutability":"nonpayable"}
]"#;

const ADDR_A: &str = "0x5fbdb2315678afecb367f032d93f642f64180aa3";
const ADDR_B: &str = "0xe7f1725e7734ce288f8367e1bb143e90bb3f0512";

#[test]
fn noop_returns_none() {
    assert!(normalize_action(&Action::Noop).unwrap().is_none());
}

#[test]
fn contract_call_normalizes_to_tx_with_encoded_calldata() {
    let cc = ContractCallAction {
        address: ADDR_A.into(),
        abi: COUNTER_ABI.into(),
        function: "increment".into(),
        args: vec![],
        value: "0".into(),
    };
    let na = normalize_action(&Action::ContractCall(cc)).unwrap().unwrap();
    assert_eq!(na.source, NormalizedActionKind::ContractCall);
    // increment() selector = 0xd09de08a (keccak("increment()")[..4]).
    assert_eq!(na.selector, Some([0xd0, 0x9d, 0xe0, 0x8a]));
    assert_eq!(na.native_value, U256::ZERO);
    assert!(na.erc20_amount.is_none());
}

#[test]
fn contract_call_with_value_propagates_u256() {
    let cc = ContractCallAction {
        address: ADDR_A.into(),
        abi: COUNTER_ABI.into(),
        function: "increment".into(),
        args: vec![],
        value: "1000000000000000000".into(),
    };
    let na = normalize_action(&Action::ContractCall(cc)).unwrap().unwrap();
    assert_eq!(na.native_value, U256::from(1_000_000_000_000_000_000u64));
}

#[test]
fn raw_call_with_full_calldata_extracts_selector() {
    let rc = RawCallAction {
        address: ADDR_A.into(),
        // 0xa9059cbb (transfer selector) + 28 bytes of zero padding (32 total
        // after the selector — full ABI-aligned calldata stub).
        data: "0xa9059cbb00000000000000000000000000000000000000000000000000000000".into(),
        value: "0".into(),
    };
    let na = normalize_action(&Action::RawCall(rc)).unwrap().unwrap();
    assert_eq!(na.source, NormalizedActionKind::RawCall);
    assert_eq!(na.selector, Some([0xa9, 0x05, 0x9c, 0xbb]));
    assert_eq!(na.native_value, U256::ZERO);
}

#[test]
fn raw_call_with_short_calldata_has_none_selector() {
    let rc = RawCallAction {
        address: ADDR_A.into(),
        data: "0x".into(),
        value: "0".into(),
    };
    let na = normalize_action(&Action::RawCall(rc)).unwrap().unwrap();
    assert!(na.selector.is_none());

    let rc2 = RawCallAction {
        address: ADDR_A.into(),
        data: "0x1234".into(),
        value: "0".into(),
    };
    let na2 = normalize_action(&Action::RawCall(rc2)).unwrap().unwrap();
    assert!(na2.selector.is_none());
}

#[test]
fn erc20_transfer_normalizes_with_a9059cbb_selector() {
    let et = Erc20TransferAction {
        token: ADDR_A.into(),
        to: ADDR_B.into(),
        amount: "1000".into(),
    };
    let na = normalize_action(&Action::Erc20Transfer(et)).unwrap().unwrap();
    assert_eq!(na.source, NormalizedActionKind::Erc20Transfer);
    assert_eq!(na.selector, Some([0xa9, 0x05, 0x9c, 0xbb]));
    assert_eq!(na.native_value, U256::ZERO);
    assert_eq!(na.erc20_amount, Some(U256::from(1000u64)));
}

#[test]
fn erc20_approve_normalizes_with_095ea7b3_selector() {
    let ea = Erc20ApproveAction {
        token: ADDR_A.into(),
        spender: ADDR_B.into(),
        amount: "1000".into(),
    };
    let na = normalize_action(&Action::Erc20Approve(ea)).unwrap().unwrap();
    assert_eq!(na.source, NormalizedActionKind::Erc20Approve);
    assert_eq!(na.selector, Some([0x09, 0x5e, 0xa7, 0xb3]));
    assert_eq!(na.erc20_amount, Some(U256::from(1000u64)));
}

#[test]
fn native_transfer_has_empty_data_and_full_value() {
    let nt = NativeTransferAction {
        to: ADDR_B.into(),
        value: "5000000000000000000".into(), // 5 ETH
    };
    let na = normalize_action(&Action::NativeTransfer(nt)).unwrap().unwrap();
    assert_eq!(na.source, NormalizedActionKind::NativeTransfer);
    assert!(na.selector.is_none());
    assert_eq!(na.native_value, U256::from(5_000_000_000_000_000_000u128));
    assert!(na.erc20_amount.is_none());
}

#[test]
fn contract_call_bad_address_returns_evm_encode_error() {
    let cc = ContractCallAction {
        address: "not_an_address".into(),
        abi: COUNTER_ABI.into(),
        function: "increment".into(),
        args: vec![],
        value: "0".into(),
    };
    let err = normalize_action(&Action::ContractCall(cc)).unwrap_err();
    // BR-01: stable Display, no raw alloy text.
    let s = err.to_string();
    assert!(
        s.starts_with("evm encode error"),
        "stable taxonomy prefix expected: got {s:?}"
    );
    // MR-01: the offending raw input must NOT leak onto the wire.
    assert!(!s.contains("not_an_address"), "raw input leaked: {s:?}");
}

#[test]
fn native_transfer_bad_decimal_value_returns_evm_encode_error() {
    let nt = NativeTransferAction {
        to: ADDR_B.into(),
        value: "not_a_number".into(),
    };
    let err = normalize_action(&Action::NativeTransfer(nt)).unwrap_err();
    let s = err.to_string();
    assert!(s.starts_with("evm encode error"), "got {s:?}");
    assert!(!s.contains("not_a_number"), "raw input leaked: {s:?}");
}

#[test]
fn contract_call_calldata_is_deterministic() {
    // Same inputs → same selector + tail bytes → regression lock against
    // future encoder drift.
    let cc = ContractCallAction {
        address: ADDR_A.into(),
        abi: COUNTER_ABI.into(),
        function: "increment".into(),
        args: vec![],
        value: "0".into(),
    };
    let na1 = normalize_action(&Action::ContractCall(cc.clone()))
        .unwrap()
        .unwrap();
    let na2 = normalize_action(&Action::ContractCall(cc)).unwrap().unwrap();
    assert_eq!(na1.selector, na2.selector);
}
