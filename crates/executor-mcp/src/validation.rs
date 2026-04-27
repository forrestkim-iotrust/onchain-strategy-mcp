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
}
