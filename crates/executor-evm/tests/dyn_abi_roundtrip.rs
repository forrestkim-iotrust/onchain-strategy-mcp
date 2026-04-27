//! Round-trip contract for the Phase 4 D-03 BigInt convention.

use alloy_dyn_abi::DynSolType;
use executor_evm::dyn_abi::{dyn_sol_to_js_value, js_value_to_dyn_sol};
use serde_json::{Value, json};

#[test]
fn uint256_decimal_string_roundtrip() {
    let ty: DynSolType = "uint256".parse().unwrap();
    let v = js_value_to_dyn_sol(
        &json!("123456789012345678901234567890"),
        &ty,
    )
    .unwrap();
    let back = dyn_sol_to_js_value(&v).unwrap();
    assert_eq!(back, json!("123456789012345678901234567890"));
}

#[test]
fn uint32_uses_json_number() {
    let ty: DynSolType = "uint32".parse().unwrap();
    let v = js_value_to_dyn_sol(&json!(1_000_000), &ty).unwrap();
    let back = dyn_sol_to_js_value(&v).unwrap();
    assert_eq!(back, json!(1_000_000));
}

#[test]
fn int256_signed_decimal_roundtrip() {
    let ty: DynSolType = "int256".parse().unwrap();
    let v = js_value_to_dyn_sol(&json!("-12345678901234567890"), &ty).unwrap();
    let back = dyn_sol_to_js_value(&v).unwrap();
    assert_eq!(back, json!("-12345678901234567890"));
}

#[test]
fn address_lowercase_and_eip55_both_accepted() {
    let ty: DynSolType = "address".parse().unwrap();
    let lower = "0x52908400098527886e0f7030069857d2e4169ee7";
    let cs = "0x52908400098527886E0F7030069857D2E4169EE7";
    let v1 = js_value_to_dyn_sol(&json!(lower), &ty).unwrap();
    let v2 = js_value_to_dyn_sol(&json!(cs), &ty).unwrap();
    // Output is always EIP-55 canonical (alloy_primitives::Address::to_checksum).
    let out1 = dyn_sol_to_js_value(&v1).unwrap();
    let out2 = dyn_sol_to_js_value(&v2).unwrap();
    assert_eq!(out1, out2);
    assert_eq!(out1, Value::String(cs.to_string()));
}

#[test]
fn bytes_hex_roundtrip() {
    let ty: DynSolType = "bytes".parse().unwrap();
    let v = js_value_to_dyn_sol(&json!("0xdeadbeef"), &ty).unwrap();
    assert_eq!(dyn_sol_to_js_value(&v).unwrap(), json!("0xdeadbeef"));
}

#[test]
fn fixed_bytes32_roundtrip() {
    let ty: DynSolType = "bytes32".parse().unwrap();
    let hex32 = "0x".to_string() + &"ab".repeat(32);
    let v = js_value_to_dyn_sol(&json!(hex32), &ty).unwrap();
    assert_eq!(dyn_sol_to_js_value(&v).unwrap(), json!(hex32));
}

#[test]
fn tuple_driven_by_abi_not_json_shape() {
    // Pitfall 10: distinguishing tuple (uint256,address) from uint256[2]
    // requires the ABI type. Same JSON shape.
    let ty_tuple: DynSolType = "(uint256,address)".parse().unwrap();
    let v = js_value_to_dyn_sol(
        &json!(["1", "0x52908400098527886e0f7030069857d2e4169ee7"]),
        &ty_tuple,
    )
    .unwrap();
    match v {
        alloy_dyn_abi::DynSolValue::Tuple(_) => {}
        other => panic!("expected Tuple, got {other:?}"),
    }
}

#[test]
fn dynamic_uint256_array_recurses() {
    let ty: DynSolType = "uint256[]".parse().unwrap();
    let v = js_value_to_dyn_sol(&json!(["1", "2", "3"]), &ty).unwrap();
    assert_eq!(dyn_sol_to_js_value(&v).unwrap(), json!(["1", "2", "3"]));
}

#[test]
fn fixed_address_array_enforces_length() {
    let ty: DynSolType = "address[3]".parse().unwrap();
    let too_few = json!(["0x0000000000000000000000000000000000000001"]);
    let err = js_value_to_dyn_sol(&too_few, &ty).unwrap_err();
    assert_eq!(err.data_kind(), "evm_decode_error");
    assert!(err.to_string().starts_with("evm encode error"));
}

#[test]
fn js_value_bigint_is_rejected() {
    // D-03: JS BigInt has no JSON form; the host's qjs_value_to_json layer
    // rejects it before this walker. We pin the contract here too: a
    // non-decimal string against a uint256 surfaces a stable wire-safe error.
    let ty: DynSolType = "uint256".parse().unwrap();
    let bad = serde_json::Value::String("not_a_decimal".into());
    let err = js_value_to_dyn_sol(&bad, &ty).unwrap_err();
    assert!(err.to_string().starts_with("evm encode error"));
    // Stable category for a non-decimal string is `bad_decimal`.
    match err {
        executor_evm::EvmError::Encode { category, .. } => {
            assert_eq!(category, "bad_decimal");
        }
        other => panic!("expected Encode(bad_decimal), got {other:?}"),
    }
}

#[test]
fn uint64_rejects_json_number_per_d03() {
    // uint64+ requires decimal-string per D-03.
    let ty: DynSolType = "uint64".parse().unwrap();
    let err = js_value_to_dyn_sol(&json!(123_456_789_012u64), &ty).unwrap_err();
    assert!(err.to_string().starts_with("evm encode error"));
}
