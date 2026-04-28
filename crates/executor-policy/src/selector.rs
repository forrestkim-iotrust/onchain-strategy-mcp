//! 4-byte selector helpers (Phase 5 POL-03).

/// Extract the 4-byte selector prefix from raw calldata. Returns `None` when
/// calldata is shorter than 4 bytes (e.g. zero-data NativeTransfer or
/// sub-4-byte RawCall — Pitfall P-4).
pub fn extract_selector(data: &[u8]) -> Option<[u8; 4]> {
    if data.len() < 4 {
        return None;
    }
    Some([data[0], data[1], data[2], data[3]])
}

/// Format a 4-byte selector as `0x` + 8 lowercase hex chars.
pub fn selector_to_hex(s: &[u8; 4]) -> String {
    format!("0x{:02x}{:02x}{:02x}{:02x}", s[0], s[1], s[2], s[3])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_none_for_short_calldata() {
        assert_eq!(extract_selector(&[]), None);
        assert_eq!(extract_selector(&[0x12]), None);
        assert_eq!(extract_selector(&[0x12, 0x34]), None);
        assert_eq!(extract_selector(&[0x12, 0x34, 0x56]), None);
    }

    #[test]
    fn extracts_first_four_bytes() {
        let calldata = [0xa9, 0x05, 0x9c, 0xbb, 0x00, 0x01, 0x02];
        assert_eq!(extract_selector(&calldata), Some([0xa9, 0x05, 0x9c, 0xbb]));
    }

    #[test]
    fn extracts_exactly_four_byte_input() {
        assert_eq!(
            extract_selector(&[0x09, 0x5e, 0xa7, 0xb3]),
            Some([0x09, 0x5e, 0xa7, 0xb3])
        );
    }

    #[test]
    fn selector_to_hex_pads_two_hex_per_byte() {
        assert_eq!(
            selector_to_hex(&[0xa9, 0x05, 0x9c, 0xbb]),
            "0xa9059cbb"
        );
        assert_eq!(
            selector_to_hex(&[0x00, 0x01, 0x02, 0x03]),
            "0x00010203"
        );
    }
}
