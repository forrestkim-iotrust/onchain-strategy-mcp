//! Phase 4 D-11 — `isAddress`, `checksum`, `ZERO_ADDRESS`.
//!
//! Pure validation/canonicalisation backed by [`alloy_primitives::Address`].
//! No RPC. No state. EIP-55 strict via `Address::parse_checksummed(_, None)`;
//! all-lowercase / all-uppercase fall back to `Address::from_str`.
//! Mixed-case-with-bad-checksum is rejected.
//!
//! `ZERO_ADDRESS` is exposed as a `&'static str`; the JS host binding
//! (`ctx.address.zeroAddress`) installs it as a frozen string property
//! (D-11 — agents shouldn't need to call it; reassignment in the strategy
//! only affects the strategy's local view, not host-side reads).

use alloy_primitives::Address;
use std::str::FromStr;

use crate::EvmError;

/// Canonical zero address. Used by `ctx.address.zeroAddress`.
pub const ZERO_ADDRESS: &str = "0x0000000000000000000000000000000000000000";

fn encode_err(category: &'static str, detail: impl Into<String>) -> EvmError {
    EvmError::Encode {
        category,
        detail_for_log: detail.into(),
    }
}

/// Total predicate — never throws. Matches the JS-side semantic:
/// non-string callers get `false` at the JS boundary.
///
/// Returns `true` for:
/// - All-lowercase 40-hex (`0x` + 40 lowercase-or-digit hex).
/// - All-uppercase 40-hex.
/// - Strict EIP-55 mixed-case (canonical checksum).
///
/// Returns `false` for:
/// - Wrong length (≠ 42).
/// - Missing `0x` prefix.
/// - Non-hex characters.
/// - Mixed-case-with-bad-checksum.
pub fn is_address(s: &str) -> bool {
    if s.len() != 42 {
        return false;
    }
    if !(s.starts_with("0x") || s.starts_with("0X")) {
        return false;
    }
    // Strict EIP-55 path
    if Address::parse_checksummed(s, None).is_ok() {
        return true;
    }
    // All-lower / all-upper fallback
    let body = &s[2..];
    if !body.bytes().all(|b| b.is_ascii_hexdigit()) {
        return false;
    }
    let all_lower = body
        .chars()
        .all(|c| !c.is_ascii_alphabetic() || c.is_ascii_lowercase());
    let all_upper = body
        .chars()
        .all(|c| !c.is_ascii_alphabetic() || c.is_ascii_uppercase());
    if all_lower || all_upper {
        return Address::from_str(s).is_ok();
    }
    false
}

/// Strict checksum — produces EIP-55 mixed-case canonical form.
///
/// - All-lowercase / all-uppercase / canonical EIP-55 input → canonical EIP-55 out.
/// - Mixed-case with bad checksum → `Err(EvmError::Encode { category: "bad_address" })`.
/// - Wrong length / non-hex / missing 0x → `Err(EvmError::Encode { category: "bad_address" })`.
pub fn checksum(s: &str) -> Result<String, EvmError> {
    if s.len() != 42 || !(s.starts_with("0x") || s.starts_with("0X")) {
        return Err(encode_err(
            "bad_address",
            format!("address must be 0x + 40 hex digits: {s}"),
        ));
    }
    let body = &s[2..];
    if !body.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(encode_err(
            "bad_address",
            "address contains non-hex characters".to_string(),
        ));
    }
    // Strict EIP-55 first.
    if let Ok(a) = Address::parse_checksummed(s, None) {
        return Ok(a.to_checksum(None));
    }
    // Fallback only for all-lower or all-upper bodies.
    let all_lower = body
        .chars()
        .all(|c| !c.is_ascii_alphabetic() || c.is_ascii_lowercase());
    let all_upper = body
        .chars()
        .all(|c| !c.is_ascii_alphabetic() || c.is_ascii_uppercase());
    if !(all_lower || all_upper) {
        return Err(encode_err(
            "bad_address",
            format!("address looks checksummed but checksum is invalid: {s}"),
        ));
    }
    let a = Address::from_str(s)
        .map_err(|e| encode_err("bad_address", format!("address parse: {e}")))?;
    Ok(a.to_checksum(None))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cat_of(e: &EvmError) -> &'static str {
        match e {
            EvmError::Encode { category, .. } => category,
            _ => "OTHER",
        }
    }

    #[test]
    fn zero_address_constant_is_canonical() {
        assert_eq!(ZERO_ADDRESS, "0x0000000000000000000000000000000000000000");
        assert!(is_address(ZERO_ADDRESS));
    }

    #[test]
    fn is_address_accepts_lowercase_and_eip55_rejects_bad_checksum() {
        let lower = "0xdeadbeefcafebabedeadbeefcafebabedeadbeef";
        assert!(is_address(lower));
        let strict = Address::from_str(lower).unwrap().to_checksum(None);
        assert!(is_address(&strict));
        // Flip case of one alpha char in the canonical form -> bad checksum.
        let mut chars: Vec<char> = strict.chars().collect();
        for (i, c) in strict.chars().enumerate().skip(2) {
            if c.is_ascii_alphabetic() {
                chars[i] = if c.is_ascii_uppercase() {
                    c.to_ascii_lowercase()
                } else {
                    c.to_ascii_uppercase()
                };
                break;
            }
        }
        let bad: String = chars.into_iter().collect();
        assert_ne!(bad, strict, "must have flipped at least one char");
        assert!(
            !is_address(&bad),
            "expected rejection for mixed-case bad checksum: {bad}"
        );
    }

    #[test]
    fn is_address_rejects_wrong_length() {
        assert!(!is_address("0xdead"));
        assert!(!is_address(
            "0xdeadbeefcafebabedeadbeefcafebabedeadbeef00"
        )); // too long
    }

    #[test]
    fn is_address_rejects_missing_0x() {
        assert!(!is_address(
            "deadbeefcafebabedeadbeefcafebabedeadbeefcafe"
        ));
    }

    #[test]
    fn is_address_rejects_non_hex() {
        assert!(!is_address(
            "0xZZZdbeefcafebabedeadbeefcafebabedeadbeef"
        ));
    }

    #[test]
    fn checksum_produces_eip55_mixed_case() {
        // Canonical example from EIP-55 spec.
        let lower = "0x52908400098527886e0f7030069857d2e4169ee7";
        let canon = "0x52908400098527886E0F7030069857D2E4169EE7";
        assert_eq!(checksum(lower).unwrap(), canon);
        assert_eq!(checksum(canon).unwrap(), canon);
    }

    #[test]
    fn checksum_rejects_mixed_case_bad() {
        // Take the canonical form and flip one bit.
        let canon = "0x52908400098527886E0F7030069857D2E4169EE7";
        // Flip the 'E' to 'e' at position 21 (index into the string).
        let mut chars: Vec<char> = canon.chars().collect();
        for (i, c) in canon.chars().enumerate().skip(2) {
            if c.is_ascii_alphabetic() {
                chars[i] = if c.is_ascii_uppercase() {
                    c.to_ascii_lowercase()
                } else {
                    c.to_ascii_uppercase()
                };
                break;
            }
        }
        let bad: String = chars.into_iter().collect();
        let r = checksum(&bad);
        assert!(r.is_err());
        assert_eq!(cat_of(&r.unwrap_err()), "bad_address");
    }

    #[test]
    fn checksum_rejects_wrong_length() {
        let r = checksum("0xdead");
        assert!(r.is_err());
        assert_eq!(cat_of(&r.unwrap_err()), "bad_address");
    }
}
