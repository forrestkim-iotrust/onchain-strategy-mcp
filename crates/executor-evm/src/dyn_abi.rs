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

use alloy_dyn_abi::{DynSolType, DynSolValue};
use alloy_primitives::{Address, B256, I256, U256};

use crate::EvmError;

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
            // Accept lowercase / uppercase / EIP-55 — the action validator at
            // the MCP boundary enforces checksum strictness for D-09.
            let addr = Address::from_str(s)
                .map_err(|e| encode_err("bad_address", format!("address parse: {e}")))?;
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
            category: "unsupported_return_type",
            detail_for_log: format!("unsupported DynSolValue for v1: {other:?}"),
        }),
    }
}

// ---- helpers ----

fn encode_err(category: &'static str, detail: impl Into<String>) -> EvmError {
    EvmError::Encode {
        category,
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
                category: "uint_too_wide_for_number",
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
            category: "int_too_wide_for_number",
            detail_for_log: format!("int{bits} value did not fit i64"),
        })?;
        Ok(serde_json::Value::Number(v.into()))
    } else {
        Ok(serde_json::Value::String(i.to_string()))
    }
}
