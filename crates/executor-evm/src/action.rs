//! Phase-4 D-09 — pure-function builder validators.
//!
//! Used by both the JS-side `ctx.actions.*` builders (sandbox) and the
//! `executor-mcp` JSON-output gate (`validate_strategy_output`). All
//! functions are sync, allocate at worst a single Bytes / U256 / JsonAbi
//! parse, and return [`EvmError`] with stable wire-safe taxonomy strings.
//!
//! Stable error category strings (used by tests + tracing):
//! - `bad_address` — address is not 40-hex or fails EIP-55 lenient check.
//! - `bad_calldata` — calldata is missing `0x`, odd-length, or non-hex.
//! - `bad_decimal` — amount is negative, hex-prefixed, or non-decimal.
//! - `abi_oversize` — abi exceeds [`MAX_ABI_BYTES`].
//! - `abi_parse` — abi is not parseable as `JsonAbi`.
//! - `abi_function_missing` — function name not in abi.
//! - `abi_arg_count` — no overload accepts the supplied arg count.
//! - `abi_type_parse` — ABI input type cannot be parsed as DynSolType.
//! - `abi_encode_input` — alloy refused the dry-run encode (type mismatch).
//!
//! Bytes from `dry_run_abi_encode` are DISCARDED — Phase 5 owns canonical
//! calldata encoding for broadcast.

use std::str::FromStr;

use alloy_json_abi::JsonAbi;
use alloy_primitives::{Address, Bytes, U256};
use executor_core::schema::action::MAX_ABI_BYTES;

use crate::EvmError;

fn encode_err(category: &'static str, detail: impl Into<String>) -> EvmError {
    EvmError::Encode {
        category,
        detail_for_log: detail.into(),
    }
}

fn decode_err(category: &'static str, detail: impl Into<String>) -> EvmError {
    EvmError::Decode {
        category,
        detail_for_log: detail.into(),
    }
}

/// Lenient EIP-55 address validator (D-09).
///
/// - Strict checksum (`Address::parse_checksummed`) accepted unconditionally.
/// - All-lowercase or all-uppercase 40-hex falls through to `Address::from_str`.
/// - Mixed-case-with-bad-checksum is REJECTED with `category="bad_address"`.
pub fn validate_address(s: &str) -> Result<Address, EvmError> {
    if let Ok(a) = Address::parse_checksummed(s, None) {
        return Ok(a);
    }
    let body = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    let has_alpha = body.chars().any(|c| c.is_ascii_alphabetic());
    let all_lower = body.chars().all(|c| !c.is_ascii_alphabetic() || c.is_ascii_lowercase());
    let all_upper = body.chars().all(|c| !c.is_ascii_alphabetic() || c.is_ascii_uppercase());
    if !has_alpha || all_lower || all_upper {
        return Address::from_str(s)
            .map_err(|e| encode_err("bad_address", format!("address parse: {e}")));
    }
    Err(encode_err(
        "bad_address",
        format!("address looks checksummed but checksum is invalid: {s}"),
    ))
}

/// Validate calldata hex (D-09). Must be 0x-prefixed, even-length, hex-only.
pub fn validate_calldata(s: &str) -> Result<Bytes, EvmError> {
    if !(s.starts_with("0x") || s.starts_with("0X")) {
        return Err(encode_err(
            "bad_calldata",
            format!("calldata must be 0x-prefixed: {s}"),
        ));
    }
    let body = &s[2..];
    if !body.len().is_multiple_of(2) {
        return Err(encode_err(
            "bad_calldata",
            "calldata hex length must be even".to_string(),
        ));
    }
    if !body.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(encode_err(
            "bad_calldata",
            "calldata contains non-hex characters".to_string(),
        ));
    }
    Bytes::from_str(s).map_err(|e| encode_err("bad_calldata", format!("hex parse: {e}")))
}

/// Validate a non-negative decimal amount string fits in U256 (D-09 / D-03).
///
/// Rejects: negative numbers, `0x`-prefixed hex, scientific notation, leading
/// `+`, embedded whitespace.
pub fn validate_decimal_amount(s: &str) -> Result<U256, EvmError> {
    if s.is_empty() {
        return Err(encode_err("bad_decimal", "amount must be non-empty".to_string()));
    }
    if s.starts_with('-') {
        return Err(encode_err(
            "bad_decimal",
            "amount must be non-negative".to_string(),
        ));
    }
    if s.starts_with('+') {
        return Err(encode_err(
            "bad_decimal",
            "amount must not have leading '+'".to_string(),
        ));
    }
    if s.starts_with("0x") || s.starts_with("0X") {
        return Err(encode_err(
            "bad_decimal",
            "amount must be decimal (no 0x prefix); use ctx.units.parseUnits".to_string(),
        ));
    }
    if !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(encode_err(
            "bad_decimal",
            format!("amount must be decimal digits only: {s}"),
        ));
    }
    U256::from_str_radix(s, 10).map_err(|e| encode_err("bad_decimal", format!("U256 parse: {e}")))
}

/// Hard-cap ABI string size (D-08 / RESEARCH Pitfall 11).
pub fn validate_abi_size(s: &str) -> Result<(), EvmError> {
    if s.len() > MAX_ABI_BYTES {
        return Err(encode_err(
            "abi_oversize",
            format!("abi size {} exceeds {}", s.len(), MAX_ABI_BYTES),
        ));
    }
    Ok(())
}

/// Dry-run ABI encode at builder time (D-09).
///
/// Sequence:
/// 1. [`validate_abi_size`].
/// 2. Parse `abi` as [`JsonAbi`].
/// 3. Resolve `function` (overload by arg count).
/// 4. Convert each arg via `js_value_to_dyn_sol` against the function's input
///    types.
/// 5. Call `Function::abi_encode_input`. The encoded bytes are DISCARDED —
///    Phase 5 owns canonical encoding.
pub fn dry_run_abi_encode(
    abi: &str,
    function: &str,
    args: &[serde_json::Value],
) -> Result<(), EvmError> {
    use alloy_dyn_abi::JsonAbiExt;

    validate_abi_size(abi)?;
    let parsed: JsonAbi = serde_json::from_str(abi)
        .map_err(|e| decode_err("abi_parse", format!("JsonAbi parse: {e}")))?;
    let candidates = match parsed.function(function) {
        Some(fs) if !fs.is_empty() => fs,
        _ => {
            return Err(decode_err(
                "abi_function_missing",
                format!("abi does not contain function {function}"),
            ));
        }
    };
    let func = candidates
        .iter()
        .find(|f| f.inputs.len() == args.len())
        .ok_or_else(|| {
            encode_err(
                "abi_arg_count",
                format!(
                    "no overload of {function} accepts {} args",
                    args.len()
                ),
            )
        })?;
    let dyn_values: Vec<alloy_dyn_abi::DynSolValue> = func
        .inputs
        .iter()
        .zip(args)
        .map(|(p, a)| {
            let ty: alloy_dyn_abi::DynSolType = p
                .selector_type()
                .parse()
                .map_err(|e| {
                    encode_err(
                        "abi_type_parse",
                        format!("DynSolType parse '{}': {e}", p.selector_type()),
                    )
                })?;
            crate::dyn_abi::js_value_to_dyn_sol(a, &ty)
        })
        .collect::<Result<_, _>>()?;
    let _bytes = func
        .abi_encode_input(&dyn_values)
        .map_err(|e| encode_err("abi_encode_input", format!("alloy abi_encode_input: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cat_of(e: &EvmError) -> &'static str {
        match e {
            EvmError::Encode { category, .. } => category,
            EvmError::Decode { category, .. } => category,
            _ => "OTHER",
        }
    }

    #[test]
    fn validate_address_accepts_lowercase_and_eip55_rejects_bad_checksum() {
        // All-lowercase address with mixed alpha/digit body.
        let lower = "0xdeadbeefcafebabedeadbeefcafebabedeadbeef";
        assert!(validate_address(lower).is_ok());

        // Strict EIP-55: compute the canonical checksum for the same body.
        let strict = Address::from_str(lower).unwrap().to_checksum(None);
        assert!(validate_address(&strict).is_ok());
        // Sanity: the canonical checksum form has at least one uppercase alpha.
        assert!(strict.chars().any(|c| c.is_ascii_uppercase()));

        // Mixed-case-with-bad-checksum: invert one specific char in the
        // canonical form. Flipping a single case bit guarantees the EIP-55
        // checksum mismatches.
        let bytes: Vec<char> = strict.chars().collect();
        let mut idx = None;
        for (i, c) in bytes.iter().enumerate().skip(2) {
            if c.is_ascii_alphabetic() {
                idx = Some(i);
                break;
            }
        }
        let i = idx.expect("at least one alpha char");
        let mut flipped: Vec<char> = bytes.clone();
        flipped[i] = if bytes[i].is_ascii_uppercase() {
            bytes[i].to_ascii_lowercase()
        } else {
            bytes[i].to_ascii_uppercase()
        };
        let bad: String = flipped.into_iter().collect();
        let r = validate_address(&bad);
        assert!(
            r.is_err(),
            "expected rejection of mixed-case-bad-checksum: {bad}"
        );
        assert_eq!(cat_of(&r.unwrap_err()), "bad_address");
    }

    #[test]
    fn validate_address_rejects_too_short() {
        let r = validate_address("0xdead");
        assert!(r.is_err());
        assert_eq!(cat_of(&r.unwrap_err()), "bad_address");
    }

    #[test]
    fn validate_calldata_requires_0x_prefix() {
        // No prefix -> err
        let r = validate_calldata("deadbeef");
        assert!(r.is_err());
        assert_eq!(cat_of(&r.unwrap_err()), "bad_calldata");

        // Odd length -> err
        let r = validate_calldata("0xdea");
        assert!(r.is_err());
        assert_eq!(cat_of(&r.unwrap_err()), "bad_calldata");

        // Non-hex -> err
        let r = validate_calldata("0xZZZZ");
        assert!(r.is_err());
        assert_eq!(cat_of(&r.unwrap_err()), "bad_calldata");

        // Empty body OK (selector-less call)
        assert!(validate_calldata("0x").is_ok());

        // Even-hex OK
        assert!(validate_calldata("0xdeadbeef").is_ok());
    }

    #[test]
    fn validate_decimal_amount_rejects_negatives_and_hex() {
        let r = validate_decimal_amount("-1");
        assert!(r.is_err());
        assert_eq!(cat_of(&r.unwrap_err()), "bad_decimal");

        let r = validate_decimal_amount("0x1");
        assert!(r.is_err());
        assert_eq!(cat_of(&r.unwrap_err()), "bad_decimal");

        let r = validate_decimal_amount("+1");
        assert!(r.is_err());
        assert_eq!(cat_of(&r.unwrap_err()), "bad_decimal");

        let r = validate_decimal_amount("1e18");
        assert!(r.is_err());
        assert_eq!(cat_of(&r.unwrap_err()), "bad_decimal");

        let r = validate_decimal_amount("");
        assert!(r.is_err());
        assert_eq!(cat_of(&r.unwrap_err()), "bad_decimal");

        // Valid
        assert!(validate_decimal_amount("0").is_ok());
        assert!(validate_decimal_amount("1000000000000000000").is_ok());
    }

    #[test]
    fn validate_abi_size_caps_at_64kib() {
        let small = "[]";
        assert!(validate_abi_size(small).is_ok());

        let big = "x".repeat(MAX_ABI_BYTES + 1);
        let r = validate_abi_size(&big);
        assert!(r.is_err());
        assert_eq!(cat_of(&r.unwrap_err()), "abi_oversize");
    }

    #[test]
    fn dry_run_abi_encode_succeeds_then_discards_bytes() {
        let abi = r#"[
            {"type":"function","name":"transfer","inputs":[
                {"name":"to","type":"address"},
                {"name":"amount","type":"uint256"}
            ],"outputs":[{"name":"","type":"bool"}],"stateMutability":"nonpayable"}
        ]"#;
        let args = vec![
            json!("0x0000000000000000000000000000000000000001"),
            json!("1000"),
        ];
        let r = dry_run_abi_encode(abi, "transfer", &args);
        assert!(r.is_ok(), "{r:?}");
    }

    #[test]
    fn dry_run_abi_encode_fails_on_arg_count_mismatch() {
        let abi = r#"[
            {"type":"function","name":"transfer","inputs":[
                {"name":"to","type":"address"},
                {"name":"amount","type":"uint256"}
            ],"outputs":[],"stateMutability":"nonpayable"}
        ]"#;
        let args = vec![json!("0x0000000000000000000000000000000000000001")];
        let r = dry_run_abi_encode(abi, "transfer", &args);
        assert!(r.is_err());
        assert_eq!(cat_of(&r.unwrap_err()), "abi_arg_count");
    }

    #[test]
    fn dry_run_abi_encode_fails_on_function_missing() {
        let abi = r#"[
            {"type":"function","name":"transfer","inputs":[],"outputs":[],"stateMutability":"nonpayable"}
        ]"#;
        let r = dry_run_abi_encode(abi, "approve", &[]);
        assert!(r.is_err());
        assert_eq!(cat_of(&r.unwrap_err()), "abi_function_missing");
    }

    #[test]
    fn dry_run_abi_encode_fails_on_oversize_abi() {
        let big = "x".repeat(MAX_ABI_BYTES + 1);
        let r = dry_run_abi_encode(&big, "f", &[]);
        assert!(r.is_err());
        assert_eq!(cat_of(&r.unwrap_err()), "abi_oversize");
    }

    #[test]
    fn dry_run_abi_encode_fails_on_unparseable_abi() {
        let r = dry_run_abi_encode("not json", "f", &[]);
        assert!(r.is_err());
        assert_eq!(cat_of(&r.unwrap_err()), "abi_parse");
    }
}
