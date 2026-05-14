//! Handler-side validation for tool inputs (D-09). Schema-level constraints
//! exist in `executor-core::schema` but serde does not enforce `maxLength`,
//! so the tool entrypoint re-checks every bound and names the violated
//! constraint in the error message (D-09b).

use executor_core::schema::strategy::StrategyRegisterInput;

pub(crate) const MAX_SOURCE_BYTES: usize = 256 * 1024; // 262144
pub(crate) const MAX_NAME_CHARS: usize = 128;
pub(crate) const MAX_DESCRIPTION_CHARS: usize = 4096;
pub(crate) const MAX_TAGS: usize = 16;
pub(crate) const MAX_TAG_CHARS: usize = 64;

/// Phase 5 D-12 / D-18 / BR-02 carry-forward — Action[] length cap enforced
/// at the JSON-output gate (`validate_strategy_output`). Mirrors Phase-4
/// `MAX_ABI_BYTES` cap-at-gate semantics: a strategy returning more than
/// 32 actions is a *shape* problem, surfaced as -32018 STRATEGY_INVALID_OUTPUT.
pub(crate) const MAX_ACTIONS_PER_RUN: usize = 32;

pub fn validate_register(input: &StrategyRegisterInput) -> Result<(), String> {
    // source: byte-length check (D-09 + Pitfall 8 — NOT chars).
    if input.source.is_empty() {
        return Err("source is empty (must be >= 1 byte UTF-8 text)".into());
    }
    if input.source.len() > MAX_SOURCE_BYTES {
        return Err(format!(
            "source size {} bytes exceeds {}",
            input.source.len(),
            MAX_SOURCE_BYTES
        ));
    }

    // name: scalar-count check (D-09 + Pitfall 8).
    if input.name.trim().is_empty() {
        return Err("name is empty or whitespace-only".into());
    }
    let name_chars = input.name.chars().count();
    if name_chars > MAX_NAME_CHARS {
        return Err(format!(
            "name length {name_chars} chars exceeds {MAX_NAME_CHARS}"
        ));
    }

    // description
    if let Some(desc) = input.description.as_deref() {
        let c = desc.chars().count();
        if c > MAX_DESCRIPTION_CHARS {
            return Err(format!(
                "description length {c} chars exceeds {MAX_DESCRIPTION_CHARS}"
            ));
        }
    }

    // tags
    if let Some(tags) = input.tags.as_deref() {
        if tags.len() > MAX_TAGS {
            return Err(format!("tags length {} exceeds {MAX_TAGS}", tags.len()));
        }
        for (i, t) in tags.iter().enumerate() {
            if t.trim().is_empty() {
                return Err(format!("tags[{i}] is empty or whitespace-only"));
            }
            let c = t.chars().count();
            if c > MAX_TAG_CHARS {
                return Err(format!(
                    "tags[{i}] length {c} chars exceeds {MAX_TAG_CHARS}"
                ));
            }
        }
    }

    Ok(())
}

/// Phase-4 D-09: action `kind` allowlist enforced at the JSON-output gate.
///
/// Six allowed kinds: `noop` (Phase 3), plus the five Phase-4 write
/// variants. Non-allowlisted kinds (`multi_call`, `swap`, `bridge`, …)
/// produce -32018 STRATEGY_INVALID_OUTPUT with the stable detail string
/// emitted below.
///
/// Serde alone would also reject (the `Action` enum has only these
/// variants), but having an explicit allowlist gives a CLEARER error
/// message and a future-proof place to add Phase-5 gating.
pub fn validate_action_kind_allowlisted(kind: &str) -> Result<(), String> {
    const ALLOWED: &[&str] = &[
        "noop",
        "contract_call",
        "raw_call",
        "erc20_transfer",
        "erc20_approve",
        "native_transfer",
    ];
    if ALLOWED.contains(&kind) {
        Ok(())
    } else {
        Err(format!(
            "action kind {kind:?} not allowed in Phase 4; expected one of {ALLOWED:?}"
        ))
    }
}

/// D-09a: `strategy_delete.strategy_id` must match `^[0-9a-f]{64}$`.
pub fn validate_strategy_id_format(id: &str) -> Result<(), String> {
    if id.len() != 64 {
        return Err(format!(
            "strategy_id length {} does not match expected 64 hex characters",
            id.len()
        ));
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
    {
        return Err("strategy_id must be lowercase hexadecimal".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(name: &str, source: &str) -> StrategyRegisterInput {
        StrategyRegisterInput {
            name: name.into(),
            source: source.into(),
            description: None,
            tags: None,
            // v1.4 bundle fields — not exercised by validation tests.
            records: None,
            view: None,
            dry_run: None,
        }
    }

    #[test]
    fn validate_register_accepts_minimal_input() {
        assert!(validate_register(&input("x", "// ok")).is_ok());
    }

    #[test]
    fn validate_register_rejects_empty_source() {
        let err = validate_register(&input("x", "")).unwrap_err();
        assert!(err.contains("source is empty"), "got: {err}");
    }

    #[test]
    fn validate_register_rejects_oversized_source() {
        let big = "x".repeat(MAX_SOURCE_BYTES + 1);
        let err = validate_register(&input("x", &big)).unwrap_err();
        assert!(err.contains("262145"), "got: {err}");
        assert!(err.contains("262144"), "got: {err}");
    }

    #[test]
    fn validate_register_rejects_whitespace_name() {
        let err = validate_register(&input("   ", "// ok")).unwrap_err();
        assert!(err.contains("whitespace-only"), "got: {err}");
    }

    #[test]
    fn validate_register_rejects_overlong_name() {
        let long = "x".repeat(MAX_NAME_CHARS + 1);
        let err = validate_register(&input(&long, "// ok")).unwrap_err();
        assert!(err.contains("128"), "got: {err}");
    }

    #[test]
    fn validate_register_rejects_oversize_description() {
        let mut i = input("x", "// ok");
        i.description = Some("y".repeat(MAX_DESCRIPTION_CHARS + 1));
        let err = validate_register(&i).unwrap_err();
        assert!(err.contains("4096"), "got: {err}");
    }

    #[test]
    fn validate_register_rejects_too_many_tags() {
        let mut i = input("x", "// ok");
        i.tags = Some(vec!["t".to_string(); MAX_TAGS + 1]);
        let err = validate_register(&i).unwrap_err();
        assert!(err.contains("16"), "got: {err}");
    }

    #[test]
    fn validate_register_rejects_oversize_tag() {
        let mut i = input("x", "// ok");
        i.tags = Some(vec!["x".repeat(MAX_TAG_CHARS + 1)]);
        let err = validate_register(&i).unwrap_err();
        assert!(err.contains("64"), "got: {err}");
    }

    #[test]
    fn validate_register_rejects_whitespace_tag() {
        let mut i = input("x", "// ok");
        i.tags = Some(vec![" ".to_string()]);
        let err = validate_register(&i).unwrap_err();
        assert!(err.contains("whitespace-only"), "got: {err}");
    }

    #[test]
    fn validate_strategy_id_format_accepts_64_hex() {
        let id = "a".repeat(64);
        assert!(validate_strategy_id_format(&id).is_ok());
        let mixed = "0123456789abcdef".repeat(4);
        assert_eq!(mixed.len(), 64);
        assert!(validate_strategy_id_format(&mixed).is_ok());
    }

    #[test]
    fn validate_strategy_id_format_rejects_uppercase() {
        let id = "A".repeat(64);
        let err = validate_strategy_id_format(&id).unwrap_err();
        assert!(err.contains("lowercase"), "got: {err}");
    }

    #[test]
    fn validate_strategy_id_format_rejects_wrong_length() {
        let id = "a".repeat(63);
        let err = validate_strategy_id_format(&id).unwrap_err();
        assert!(err.contains("63"), "got: {err}");
    }

    #[test]
    fn validate_action_kind_allowlisted_accepts_phase4_variants() {
        for k in [
            "noop",
            "contract_call",
            "raw_call",
            "erc20_transfer",
            "erc20_approve",
            "native_transfer",
        ] {
            assert!(
                validate_action_kind_allowlisted(k).is_ok(),
                "kind {k:?} should be allowed"
            );
        }
    }

    #[test]
    fn max_actions_per_run_constant_is_32() {
        assert_eq!(MAX_ACTIONS_PER_RUN, 32);
    }

    #[test]
    fn validate_action_kind_allowlisted_rejects_phase5_variants() {
        for k in ["multi_call", "swap", "bridge", "deploy"] {
            let err = validate_action_kind_allowlisted(k).unwrap_err();
            assert!(err.contains(k), "error should name the kind: {err}");
            assert!(
                err.contains("not allowed in Phase 4"),
                "stable detail string: {err}"
            );
        }
    }
}
