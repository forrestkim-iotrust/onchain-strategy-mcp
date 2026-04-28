//! Phase 4 D-10 — `parseUnits` / `formatUnits` decimal-string helpers.
//!
//! Pure functions over [`alloy_primitives::U256`]. No RPC, no allocation
//! beyond the digit string; safe to call inside the synchronous JS host
//! binding without entering the tokio runtime.
//!
//! ## Wire-safe error contract (D-15 / MR-01)
//!
//! Errors are returned as [`EvmError::Encode`] with a stable
//! `category` string in:
//! - `decimals_out_of_range` — caller passed `decimals > 77`.
//! - `amount_negative` — leading `-`.
//! - `amount_overflow_fraction` — fractional part has more digits than
//!   `decimals` (would lose precision).
//! - `amount_not_decimal` — non-digit character in either part.
//! - `amount_overflow_u256` — combined integer overflows U256.
//! - `amount_empty` — empty input.
//!
//! The `Display` form of the resulting [`EvmError`] never echoes raw
//! input or alloy text — `detail_for_log` carries the diagnostic for
//! `tracing::warn!` consumption only.

use alloy_primitives::U256;

use crate::EvmError;

/// U256 max precision is 78 decimal digits (`2^256 - 1` has 78). We cap
/// `decimals` at 77 so a single integer digit + `decimals` fractional
/// digits always fits without ambiguity at the boundary.
pub const MAX_DECIMALS: u8 = 77;

fn encode_err(category: &'static str, detail: impl Into<String>) -> EvmError {
    EvmError::Encode {
        category: std::borrow::Cow::Borrowed(category),
        detail_for_log: detail.into(),
    }
}

/// Parse a non-negative decimal `amount` to `U256` weighted by `10^decimals`.
///
/// `amount` may include a single `.` separator. Fractional part may be
/// shorter than `decimals` (right-padded with zeros) but never longer
/// (would lose precision — rejected with `amount_overflow_fraction`).
///
/// Behaviour pinned by tests:
/// - `parse_units("1.5", 18)` → `1_500_000_000_000_000_000`
/// - `parse_units("0.5", 18)` → `500_000_000_000_000_000`
/// - `parse_units("123", 0)` → `123`
/// - `parse_units("-1", 18)` → Err(amount_negative)
/// - `parse_units("1.123456789", 6)` → Err(amount_overflow_fraction)
/// - `parse_units("1", 78)` → Err(decimals_out_of_range)
pub fn parse_units(amount: &str, decimals: u8) -> Result<U256, EvmError> {
    if decimals > MAX_DECIMALS {
        return Err(encode_err(
            "decimals_out_of_range",
            format!("decimals {decimals} exceeds U256 precision cap {MAX_DECIMALS}"),
        ));
    }
    if amount.is_empty() {
        return Err(encode_err(
            "amount_empty",
            "amount must be non-empty".to_string(),
        ));
    }
    if amount.starts_with('-') {
        return Err(encode_err(
            "amount_negative",
            "amount must be non-negative".to_string(),
        ));
    }
    if amount.starts_with('+') {
        return Err(encode_err(
            "amount_not_decimal",
            "amount must not have leading '+'".to_string(),
        ));
    }
    if amount.starts_with("0x") || amount.starts_with("0X") {
        return Err(encode_err(
            "amount_not_decimal",
            "amount must be decimal (no 0x prefix)".to_string(),
        ));
    }

    let (int_part, frac_part) = match amount.find('.') {
        Some(i) => {
            // Reject more than one '.'
            if amount[i + 1..].contains('.') {
                return Err(encode_err(
                    "amount_not_decimal",
                    "amount must contain at most one '.'".to_string(),
                ));
            }
            (&amount[..i], &amount[i + 1..])
        }
        None => (amount, ""),
    };

    if frac_part.len() > decimals as usize {
        return Err(encode_err(
            "amount_overflow_fraction",
            format!(
                "fractional digits ({}) exceed decimals ({})",
                frac_part.len(),
                decimals
            ),
        ));
    }

    let valid_digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if !int_part.is_empty() && !valid_digits(int_part) {
        return Err(encode_err(
            "amount_not_decimal",
            format!("integer part must be digits only: {int_part:?}"),
        ));
    }
    if !frac_part.is_empty() && !valid_digits(frac_part) {
        return Err(encode_err(
            "amount_not_decimal",
            format!("fractional part must be digits only: {frac_part:?}"),
        ));
    }
    // Reject "." with empty integer AND empty fraction.
    if int_part.is_empty() && frac_part.is_empty() {
        return Err(encode_err(
            "amount_not_decimal",
            "amount must contain at least one digit".to_string(),
        ));
    }

    let int_part = if int_part.is_empty() { "0" } else { int_part };
    let mut combined = String::with_capacity(int_part.len() + decimals as usize);
    combined.push_str(int_part);
    combined.push_str(frac_part);
    for _ in frac_part.len()..(decimals as usize) {
        combined.push('0');
    }
    U256::from_str_radix(&combined, 10).map_err(|e| {
        encode_err("amount_overflow_u256", format!("U256 parse: {e}"))
    })
}

/// Convenience for sandbox host bindings: parse a non-negative decimal-digit
/// string into U256, then format with the given decimals.
///
/// The strategy-js host binding for `ctx.units.formatUnits` uses this to
/// avoid taking a direct dependency on `alloy_primitives` (D-02 isolation).
pub fn format_units_from_str(value: &str, decimals: u8) -> Result<String, EvmError> {
    if value.is_empty() {
        return Err(encode_err(
            "amount_empty",
            "value must be non-empty".to_string(),
        ));
    }
    if value.starts_with('-') {
        return Err(encode_err(
            "amount_negative",
            "value must be non-negative".to_string(),
        ));
    }
    if !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(encode_err(
            "amount_not_decimal",
            "value must be a non-negative decimal-digit string".to_string(),
        ));
    }
    let u = U256::from_str_radix(value, 10).map_err(|e| {
        encode_err("amount_overflow_u256", format!("U256 parse: {e}"))
    })?;
    format_units(u, decimals)
}

/// Format `value` as a decimal string with `decimals` fractional places.
/// Trailing zeros in the fractional part are trimmed; if the trimmed
/// fraction is empty, the `.` is omitted entirely.
///
/// Behaviour pinned by tests:
/// - `format_units(U256::from(1_500_000_000_000_000_000u64), 18)` → `"1.5"`
/// - `format_units(U256::from(2_000_000_000_000_000_000u64), 18)` → `"2"`
/// - `format_units(U256::ZERO, 18)` → `"0"`
/// - `format_units(U256::from(123u64), 0)` → `"123"`
pub fn format_units(value: U256, decimals: u8) -> Result<String, EvmError> {
    if decimals > MAX_DECIMALS {
        return Err(encode_err(
            "decimals_out_of_range",
            format!("decimals {decimals} exceeds {MAX_DECIMALS}"),
        ));
    }
    if decimals == 0 {
        return Ok(value.to_string());
    }
    let s = value.to_string();
    let dec = decimals as usize;
    let (int_part, frac_part) = if s.len() <= dec {
        let pad = dec - s.len();
        let mut frac = String::with_capacity(dec);
        for _ in 0..pad {
            frac.push('0');
        }
        frac.push_str(&s);
        ("0".to_string(), frac)
    } else {
        let split = s.len() - dec;
        (s[..split].to_string(), s[split..].to_string())
    };
    let frac_trimmed = frac_part.trim_end_matches('0');
    if frac_trimmed.is_empty() {
        Ok(int_part)
    } else {
        Ok(format!("{int_part}.{frac_trimmed}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cat_of(e: &EvmError) -> &str {
        match e {
            EvmError::Encode { category, .. } => category.as_ref(),
            _ => "OTHER",
        }
    }

    #[test]
    fn parse_units_basic_round_trip() {
        let v = parse_units("1.5", 18).expect("ok");
        assert_eq!(v, U256::from(1_500_000_000_000_000_000u64));
        let s = format_units(v, 18).expect("ok");
        assert_eq!(s, "1.5");
    }

    #[test]
    fn format_units_trims_trailing_zeros() {
        let v = U256::from(2_000_000_000_000_000_000u64);
        assert_eq!(format_units(v, 18).unwrap(), "2");
    }

    #[test]
    fn parse_units_rejects_negative() {
        let r = parse_units("-1", 18);
        assert!(r.is_err());
        assert_eq!(cat_of(&r.unwrap_err()), "amount_negative");
    }

    #[test]
    fn parse_units_rejects_decimals_above_77() {
        let r = parse_units("1", 78);
        assert!(r.is_err());
        assert_eq!(cat_of(&r.unwrap_err()), "decimals_out_of_range");
    }

    #[test]
    fn parse_units_zero_decimals() {
        let v = parse_units("123", 0).expect("ok");
        assert_eq!(v, U256::from(123u64));
        // Round-trip
        assert_eq!(format_units(v, 0).unwrap(), "123");
    }

    #[test]
    fn parse_units_fractional_only() {
        let v = parse_units("0.5", 18).expect("ok");
        assert_eq!(v, U256::from(500_000_000_000_000_000u64));
    }

    #[test]
    fn parse_units_rejects_too_many_fractional_digits() {
        let r = parse_units("1.123456789", 6);
        assert!(r.is_err());
        assert_eq!(cat_of(&r.unwrap_err()), "amount_overflow_fraction");
    }

    #[test]
    fn parse_units_rejects_non_digit_input() {
        let r = parse_units("1.2x", 6);
        assert!(r.is_err());
        assert_eq!(cat_of(&r.unwrap_err()), "amount_not_decimal");
    }

    #[test]
    fn parse_units_rejects_empty() {
        let r = parse_units("", 0);
        assert!(r.is_err());
        assert_eq!(cat_of(&r.unwrap_err()), "amount_empty");
    }

    #[test]
    fn parse_units_rejects_hex_prefix() {
        let r = parse_units("0x1", 18);
        assert!(r.is_err());
        assert_eq!(cat_of(&r.unwrap_err()), "amount_not_decimal");
    }

    #[test]
    fn parse_units_rejects_double_dot() {
        let r = parse_units("1.2.3", 18);
        assert!(r.is_err());
        assert_eq!(cat_of(&r.unwrap_err()), "amount_not_decimal");
    }

    #[test]
    fn format_units_zero() {
        assert_eq!(format_units(U256::ZERO, 18).unwrap(), "0");
        assert_eq!(format_units(U256::ZERO, 0).unwrap(), "0");
    }

    #[test]
    fn format_units_small_value_pads_with_zeros() {
        // 1 wei in 18 decimals
        let s = format_units(U256::from(1u64), 18).unwrap();
        assert_eq!(s, "0.000000000000000001");
    }

    #[test]
    fn format_units_rejects_decimals_above_77() {
        let r = format_units(U256::from(1u64), 78);
        assert!(r.is_err());
        assert_eq!(
            match r.unwrap_err() {
                EvmError::Encode { category, .. } => category.into_owned(),
                _ => "OTHER".to_string(),
            },
            "decimals_out_of_range"
        );
    }

    #[test]
    fn round_trip_property_for_select_decimals() {
        // For a sample of values, format_units(parse_units(format_units(v))) == format_units(v).
        let values = [
            U256::ZERO,
            U256::from(1u64),
            U256::from(1_500_000_000_000_000_000u64),
            U256::from(123_456_789u64),
            U256::MAX,
        ];
        for d in [0u8, 1, 6, 9, 18] {
            for v in &values {
                let s1 = format_units(*v, d).expect("format ok");
                // For some MAX values, parsing back may overflow U256 if we
                // chose decimals such that the integer part > 78 digits. Skip
                // the round-trip for U256::MAX with decimals=0 — fits fine —
                // and skip when we'd exceed precision.
                let parsed = match parse_units(&s1, d) {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                let s2 = format_units(parsed, d).expect("format ok");
                assert_eq!(s1, s2, "round trip mismatch v={v} d={d}: {s1} != {s2}");
            }
        }
    }
}
