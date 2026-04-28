//! BigInt-bridged JS-arg ↔ DynSolValue conversion (Phase 4 D-03).
//!
//! Convention (verbatim D-03):
//! - `uint8..uint32` / `int8..int32` → JSON Number
//! - `uint64+` / `int64+` / `uint256` → JSON String (decimal, no `0x`; `-`
//!   allowed for signed). Hex / scientific / leading-`+` rejected.
//! - `address` → JSON String, any-case 40-hex; on encode we accept lowercase
//!   or EIP-55 (we do NOT enforce checksum here — the action validator at
//!   the MCP boundary owns checksum strictness).
//! - `bytes` / `bytesN` → JSON String 0x-prefixed even-length hex.
//! - `bool` → JSON Bool.
//! - `string` → JSON String.
//! - `tuple` → JSON Array (driven by ABI inner types — Pitfall 10).
//! - `array(dynamic)` → JSON Array.
//! - `array(fixed N)` → JSON Array of EXACT length N.
//!
//! Errors are typed `EvmError::Encode { category, detail_for_log }` with stable
//! categories: `type_mismatch`, `out_of_range`, `bad_hex`, `bad_address`,
//! `bad_decimal`, `bigint_input`, `arity_mismatch`, `abi_type_parse`.

use std::str::FromStr;

use alloy_dyn_abi::{DynSolType, DynSolValue, JsonAbiExt};
use alloy_json_abi::JsonAbi;
use alloy_primitives::{B256, Bytes, I256, U256};

use crate::EvmError;
use crate::action::validate_abi_size;

/// Encode a function call's input parameters into ABI calldata bytes
/// (selector + tail-encoded args).
///
/// Performs the full Phase-4 dry-run encoding flow:
/// 1. [`validate_abi_size`] — D-08 / RESEARCH Pitfall 11 hard cap.
/// 2. Parse `abi` as [`JsonAbi`].
/// 3. Resolve `function` (overload by arg count).
/// 4. Convert each arg via [`js_value_to_dyn_sol`] against the function's
///    input types.
/// 5. Call `Function::abi_encode_input` and **return** the encoded
///    `Bytes`.
///
/// Phase-4 [`crate::action::dry_run_abi_encode`] discards these bytes
/// (validation only); Phase-5 `executor_evm::normalize::normalize_contract_call`
/// keeps them and feeds them into `TransactionRequest.input` (D-03).
///
/// Error categories propagate from the existing helpers — same wire-safe
/// taxonomy as Phase 4 (`abi_oversize`, `abi_parse`, `abi_function_missing`,
/// `abi_arg_count`, `abi_type_parse`, `abi_encode_input`).
pub fn encode_call_input(
    abi: &str,
    function: &str,
    args: &[serde_json::Value],
) -> Result<Bytes, EvmError> {
    validate_abi_size(abi)?;
    let parsed: JsonAbi = serde_json::from_str(abi).map_err(|e| EvmError::Decode {
        category: std::borrow::Cow::Borrowed("abi_parse"),
        detail_for_log: format!("JsonAbi parse: {e}"),
    })?;
    let candidates = match parsed.function(function) {
        Some(fs) if !fs.is_empty() => fs,
        _ => {
            return Err(EvmError::Decode {
                category: std::borrow::Cow::Borrowed("abi_function_missing"),
                detail_for_log: format!("abi does not contain function {function}"),
            });
        }
    };
    let func = candidates
        .iter()
        .find(|f| f.inputs.len() == args.len())
        .ok_or_else(|| EvmError::Encode {
            category: std::borrow::Cow::Borrowed("abi_arg_count"),
            detail_for_log: format!(
                "no overload of {function} accepts {} args",
                args.len()
            ),
        })?;
    let dyn_values: Vec<DynSolValue> = func
        .inputs
        .iter()
        .zip(args)
        .map(|(p, a)| {
            let ty: DynSolType = p.selector_type().parse().map_err(|e| EvmError::Encode {
                category: std::borrow::Cow::Borrowed("abi_type_parse"),
                detail_for_log: format!("DynSolType parse '{}': {e}", p.selector_type()),
            })?;
            js_value_to_dyn_sol(a, &ty)
        })
        .collect::<Result<_, _>>()?;
    let encoded = func.abi_encode_input(&dyn_values).map_err(|e| EvmError::Encode {
        category: std::borrow::Cow::Borrowed("abi_encode_input"),
        detail_for_log: format!("alloy abi_encode_input: {e}"),
    })?;
    Ok(Bytes::from(encoded))
}

/// Convert a `serde_json::Value` (carrying a JS-side argument) into a
/// `DynSolValue` using `ty` as the source of truth (Pitfall 10).
pub fn js_value_to_dyn_sol(
    value: &serde_json::Value,
    ty: &DynSolType,
) -> Result<DynSolValue, EvmError> {
    match ty {
        DynSolType::Bool => match value {
            serde_json::Value::Bool(b) => Ok(DynSolValue::Bool(*b)),
            other => Err(encode_err("type_mismatch", format!("expected bool, got {}", json_kind(other)))),
        },
        DynSolType::String => match value {
            serde_json::Value::String(s) => Ok(DynSolValue::String(s.clone())),
            other => Err(encode_err(
                "type_mismatch",
                format!("expected string, got {}", json_kind(other)),
            )),
        },
        DynSolType::Address => {
            let s = value
                .as_str()
                .ok_or_else(|| encode_err("type_mismatch", "address expects JSON string"))?;
            // WR-08: route ABI-arg addresses through the same lenient EIP-55
            // validator the top-level action `address` field uses. Reject
            // mixed-case-bad-checksum here too so address-typed ABI args can't
            // sail through where the top-level field would be rejected.
            let addr = crate::action::validate_address(s)?;
            Ok(DynSolValue::Address(addr))
        }
        DynSolType::Bytes => {
            let s = value
                .as_str()
                .ok_or_else(|| encode_err("type_mismatch", "bytes expects JSON string"))?;
            let b = parse_hex_bytes(s)?;
            Ok(DynSolValue::Bytes(b))
        }
        DynSolType::FixedBytes(size) => {
            let s = value
                .as_str()
                .ok_or_else(|| encode_err("type_mismatch", "bytesN expects JSON string"))?;
            let bytes = parse_hex_bytes(s)?;
            if bytes.len() != *size {
                return Err(encode_err(
                    "type_mismatch",
                    format!("bytes{size} expects {size}-byte payload, got {}", bytes.len()),
                ));
            }
            let mut buf = [0u8; 32];
            buf[..*size].copy_from_slice(&bytes);
            Ok(DynSolValue::FixedBytes(B256::from(buf), *size))
        }
        DynSolType::Uint(bits) => {
            let bits = *bits;
            let v = parse_uint(value, bits)?;
            Ok(DynSolValue::Uint(v, bits))
        }
        DynSolType::Int(bits) => {
            let bits = *bits;
            let v = parse_int(value, bits)?;
            Ok(DynSolValue::Int(v, bits))
        }
        DynSolType::Tuple(inner_types) => {
            let arr = value.as_array().ok_or_else(|| {
                encode_err("type_mismatch", "tuple expects JSON array")
            })?;
            if arr.len() != inner_types.len() {
                return Err(encode_err(
                    "arity_mismatch",
                    format!(
                        "tuple expects {} elements, got {}",
                        inner_types.len(),
                        arr.len()
                    ),
                ));
            }
            let values: Vec<DynSolValue> = inner_types
                .iter()
                .zip(arr.iter())
                .map(|(t, v)| js_value_to_dyn_sol(v, t))
                .collect::<Result<_, _>>()?;
            Ok(DynSolValue::Tuple(values))
        }
        DynSolType::Array(inner) => {
            let arr = value.as_array().ok_or_else(|| {
                encode_err("type_mismatch", "dynamic array expects JSON array")
            })?;
            let values: Vec<DynSolValue> = arr
                .iter()
                .map(|v| js_value_to_dyn_sol(v, inner))
                .collect::<Result<_, _>>()?;
            Ok(DynSolValue::Array(values))
        }
        DynSolType::FixedArray(inner, n) => {
            let arr = value.as_array().ok_or_else(|| {
                encode_err("type_mismatch", "fixed array expects JSON array")
            })?;
            if arr.len() != *n {
                return Err(encode_err(
                    "arity_mismatch",
                    format!("fixed array expects {n} elements, got {}", arr.len()),
                ));
            }
            let values: Vec<DynSolValue> = arr
                .iter()
                .map(|v| js_value_to_dyn_sol(v, inner))
                .collect::<Result<_, _>>()?;
            Ok(DynSolValue::FixedArray(values))
        }
        // Function pointers / custom structs are out-of-scope for v1.
        other => Err(encode_err(
            "type_mismatch",
            format!("unsupported ABI type for v1: {other:?}"),
        )),
    }
}

/// Inverse mapping. Output convention (D-03):
/// - `uint*`/`int*` ≤ 32 bits → JSON Number
/// - `uint*`/`int*` ≥ 64 bits → JSON String (decimal)
/// - `address` → JSON String (EIP-55 checksum)
/// - `bytes` / `bytesN` → JSON String, lowercase 0x-hex
pub fn dyn_sol_to_js_value(value: &DynSolValue) -> Result<serde_json::Value, EvmError> {
    match value {
        DynSolValue::Bool(b) => Ok(serde_json::Value::Bool(*b)),
        DynSolValue::String(s) => Ok(serde_json::Value::String(s.clone())),
        DynSolValue::Address(a) => Ok(serde_json::Value::String(a.to_checksum(None))),
        DynSolValue::Bytes(b) => Ok(serde_json::Value::String(format!("0x{}", hex_encode(b)))),
        DynSolValue::FixedBytes(b, size) => Ok(serde_json::Value::String(format!(
            "0x{}",
            hex_encode(&b[..*size])
        ))),
        DynSolValue::Uint(u, bits) => uint_to_json(*u, *bits),
        DynSolValue::Int(i, bits) => int_to_json(*i, *bits),
        DynSolValue::Tuple(items) => {
            let arr: Vec<serde_json::Value> = items
                .iter()
                .map(dyn_sol_to_js_value)
                .collect::<Result<_, _>>()?;
            Ok(serde_json::Value::Array(arr))
        }
        DynSolValue::Array(items) | DynSolValue::FixedArray(items) => {
            let arr: Vec<serde_json::Value> = items
                .iter()
                .map(dyn_sol_to_js_value)
                .collect::<Result<_, _>>()?;
            Ok(serde_json::Value::Array(arr))
        }
        other => Err(EvmError::Decode {
            category: std::borrow::Cow::Borrowed("unsupported_return_type"),
            detail_for_log: format!("unsupported DynSolValue for v1: {other:?}"),
        }),
    }
}

// ---- helpers ----

fn encode_err(category: &'static str, detail: impl Into<String>) -> EvmError {
    EvmError::Encode {
        category: std::borrow::Cow::Borrowed(category),
        detail_for_log: detail.into(),
    }
}

fn json_kind(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

fn parse_hex_bytes(s: &str) -> Result<Vec<u8>, EvmError> {
    let stripped = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .ok_or_else(|| encode_err("bad_hex", "expected 0x-prefixed hex"))?;
    if stripped.len() % 2 != 0 {
        return Err(encode_err("bad_hex", "odd-length hex"));
    }
    if !stripped.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(encode_err("bad_hex", "non-hex character"));
    }
    let mut out = Vec::with_capacity(stripped.len() / 2);
    for chunk in stripped.as_bytes().chunks(2) {
        let s = std::str::from_utf8(chunk).expect("ascii");
        let b = u8::from_str_radix(s, 16)
            .map_err(|e| encode_err("bad_hex", format!("hex parse: {e}")))?;
        out.push(b);
    }
    Ok(out)
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn parse_uint(value: &serde_json::Value, bits: usize) -> Result<U256, EvmError> {
    // D-03: <= 32 bit ⇒ Number; >= 64 bit ⇒ decimal String.
    // 33..=63 bit widths are a Solidity oddity; treat them as String to stay
    // safely above f64 precision.
    let use_number = bits <= 32;
    let parsed: U256 = match value {
        serde_json::Value::Number(n) => {
            if !use_number {
                return Err(encode_err(
                    "type_mismatch",
                    format!("uint{bits} requires decimal string, not JSON Number (D-03)"),
                ));
            }
            let u = n.as_u64().ok_or_else(|| {
                encode_err(
                    "out_of_range",
                    format!("uint{bits}: JSON Number not a non-negative integer"),
                )
            })?;
            U256::from(u)
        }
        serde_json::Value::String(s) => {
            // Reject hex; only base-10 decimals.
            if s.is_empty() || s.starts_with('-') || s.starts_with('+') {
                return Err(encode_err(
                    "bad_decimal",
                    format!("uint{bits}: empty / signed input '{s}'"),
                ));
            }
            if !s.bytes().all(|b| b.is_ascii_digit()) {
                return Err(encode_err(
                    "bad_decimal",
                    format!("uint{bits}: non-digit in '{s}'"),
                ));
            }
            U256::from_str_radix(s, 10)
                .map_err(|e| encode_err("bad_decimal", format!("uint{bits}: {e}")))?
        }
        other => {
            return Err(encode_err(
                "type_mismatch",
                format!("uint{bits} expects {} value, got {}",
                    if use_number { "JSON Number or decimal String" } else { "decimal String" },
                    json_kind(other)),
            ));
        }
    };
    if bits < 256 {
        // Bound check: U256 may hold a value too wide for `bits`.
        let max = (U256::from(1u64) << bits) - U256::from(1u64);
        if parsed > max {
            return Err(encode_err(
                "out_of_range",
                format!("value exceeds uint{bits} max"),
            ));
        }
    }
    Ok(parsed)
}

fn parse_int(value: &serde_json::Value, bits: usize) -> Result<I256, EvmError> {
    let use_number = bits <= 32;
    let parsed: I256 = match value {
        serde_json::Value::Number(n) => {
            if !use_number {
                return Err(encode_err(
                    "type_mismatch",
                    format!("int{bits} requires decimal string, not JSON Number (D-03)"),
                ));
            }
            let i = n.as_i64().ok_or_else(|| {
                encode_err(
                    "out_of_range",
                    format!("int{bits}: JSON Number not an i64"),
                )
            })?;
            I256::try_from(i).map_err(|e| {
                encode_err("out_of_range", format!("int{bits}: {e}"))
            })?
        }
        serde_json::Value::String(s) => {
            if s.is_empty() {
                return Err(encode_err("bad_decimal", "int: empty string"));
            }
            // Allow leading '-'.
            let body = s.strip_prefix('-').unwrap_or(s);
            if body.is_empty() || !body.bytes().all(|b| b.is_ascii_digit()) {
                return Err(encode_err(
                    "bad_decimal",
                    format!("int{bits}: invalid decimal '{s}'"),
                ));
            }
            I256::from_str(s)
                .map_err(|e| encode_err("bad_decimal", format!("int{bits}: {e}")))?
        }
        other => {
            return Err(encode_err(
                "type_mismatch",
                format!("int{bits} expects {} value, got {}",
                    if use_number { "JSON Number or decimal String" } else { "decimal String" },
                    json_kind(other)),
            ));
        }
    };
    // Bound check for narrower widths skipped — alloy's abi_encode_input
    // catches range violations at encode time. The wire-safe error is
    // surfaced via Encode { category: "abi_encode_input" }.
    Ok(parsed)
}

fn uint_to_json(u: U256, bits: usize) -> Result<serde_json::Value, EvmError> {
    if bits <= 32 {
        // u <= u32::MAX guaranteed by the source ABI bound.
        let n: u64 = u
            .try_into()
            .map_err(|_| EvmError::Decode {
                category: std::borrow::Cow::Borrowed("uint_too_wide_for_number"),
                detail_for_log: format!("uint{bits} value did not fit u64 (impossible at <=32)"),
            })?;
        Ok(serde_json::Value::Number(n.into()))
    } else {
        Ok(serde_json::Value::String(u.to_string()))
    }
}

fn int_to_json(i: I256, bits: usize) -> Result<serde_json::Value, EvmError> {
    if bits <= 32 {
        let v: i64 = i.try_into().map_err(|_| EvmError::Decode {
            category: std::borrow::Cow::Borrowed("int_too_wide_for_number"),
            detail_for_log: format!("int{bits} value did not fit i64"),
        })?;
        Ok(serde_json::Value::Number(v.into()))
    } else {
        Ok(serde_json::Value::String(i.to_string()))
    }
}

#[cfg(test)]
mod encode_call_input_tests {
    use super::*;
    use serde_json::json;

    const SAMPLE_ABI: &str = r#"[
        {"type":"function","name":"f","inputs":[{"name":"x","type":"uint256"}],
         "outputs":[],"stateMutability":"nonpayable"}
    ]"#;

    fn cat_of(e: &EvmError) -> &str {
        match e {
            EvmError::Encode { category, .. } => category.as_ref(),
            EvmError::Decode { category, .. } => category.as_ref(),
            _ => "OTHER",
        }
    }

    #[test]
    fn encode_call_input_is_pub_and_returns_bytes() {
        let bytes = encode_call_input(SAMPLE_ABI, "f", &[json!("1")]).expect("ok");
        // 4-byte selector + 32-byte uint256 tail = 36 bytes.
        assert_eq!(bytes.len(), 36);
        // Determinism: same input → same selector bytes.
        let bytes2 = encode_call_input(SAMPLE_ABI, "f", &[json!("1")]).expect("ok");
        assert_eq!(&bytes[..4], &bytes2[..4]);
    }

    #[test]
    fn encode_call_input_rejects_missing_function() {
        let err = encode_call_input(SAMPLE_ABI, "nonexistent", &[]).unwrap_err();
        assert_eq!(cat_of(&err), "abi_function_missing");
        // Wire taxonomy: stable Display, no raw alloy text.
        assert!(err.to_string().starts_with("evm decode error"));
    }

    #[test]
    fn encode_call_input_rejects_arg_count_mismatch() {
        let err = encode_call_input(SAMPLE_ABI, "f", &[]).unwrap_err();
        assert_eq!(cat_of(&err), "abi_arg_count");
    }

    #[test]
    fn encode_call_input_rejects_unparseable_abi() {
        let err = encode_call_input("not json", "f", &[]).unwrap_err();
        assert_eq!(cat_of(&err), "abi_parse");
    }
}
