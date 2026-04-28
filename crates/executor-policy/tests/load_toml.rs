//! Plan 05-03 Task 1 — TOML policy file load + validation suite.
//!
//! Mirrors the per-behaviour matrix in `05-03-PLAN.md` Task 1.

use executor_policy::{
    PolicyError, SelectorPattern, load_policy_from_path, parse_policy_str,
};
use std::path::Path;

#[test]
fn load_permissive_fixture_returns_loaded_policy() {
    let p = Path::new("tests/fixtures/policy.permissive.toml");
    let loaded = load_policy_from_path(p).expect("permissive fixture loads");
    assert!(loaded.chains_allow.contains(&31337));
    assert!(!loaded.contracts_by_chain.is_empty());
    assert!(!loaded.selectors_by_chain_contract.is_empty());
    assert!(loaded.native_value_by_chain.contains_key(&31337));
    assert!(!loaded.erc20_spend_by_chain_token.is_empty());
    assert!(!loaded.raw_call_allow_global);
    assert_eq!(loaded.raw_call_allow.len(), 1);
}

#[test]
fn load_deny_all_fixture_returns_empty_loaded_policy() {
    let loaded = load_policy_from_path(Path::new("tests/fixtures/policy.deny_all.toml"))
        .expect("deny_all parses");
    assert!(loaded.chains_allow.is_empty());
    assert!(loaded.contracts_by_chain.is_empty());
    assert!(loaded.selectors_by_chain_contract.is_empty());
    assert!(loaded.native_value_by_chain.is_empty());
    assert!(loaded.erc20_spend_by_chain_token.is_empty());
    assert!(!loaded.raw_call_allow_global);
    assert!(loaded.raw_call_allow.is_empty());
}

#[test]
fn load_nonexistent_path_returns_file_not_found() {
    let err =
        load_policy_from_path(Path::new("/no/such/__definitely_missing__.toml")).unwrap_err();
    assert!(matches!(err, PolicyError::FileNotFound { .. }));
    assert_eq!(err.data_kind(), "policy_not_loaded");
}

#[test]
fn load_bad_address_fixture_returns_validation_error() {
    let err = load_policy_from_path(Path::new("tests/fixtures/policy.bad_address.toml"))
        .unwrap_err();
    assert!(matches!(err, PolicyError::ValidationError { .. }));
    // Wire-safe Display starts with the stable taxonomy prefix (MR-01).
    assert!(
        err.to_string().starts_with("policy config error"),
        "unexpected display: {err}"
    );
    // Raw input never leaks.
    assert!(!err.to_string().contains("0xnot_an_address"));
}

#[test]
fn load_rejects_unknown_field_in_chains() {
    // deny_unknown_fields cascade — mistyped key inside [chains].
    let toml = "[chains]\nallow = []\nbogus = 1\n";
    let err = parse_policy_str(toml).unwrap_err();
    match err {
        PolicyError::Config { category, .. } => {
            assert_eq!(category.as_ref(), "toml_parse");
        }
        other => panic!("expected Config(toml_parse); got {other:?}"),
    }
}

#[test]
fn load_rejects_chain_in_allow_without_contracts_subtable() {
    let toml = r#"
        [chains]
        allow = [31337, 999]

        [contracts.31337]
        allow = ["0x5fbdb2315678afecb367f032d93f642f64180aa3"]
    "#;
    let err = parse_policy_str(toml).unwrap_err();
    match err {
        PolicyError::ValidationError { category, .. } => {
            assert_eq!(category.as_ref(), "chain_missing_contracts_subtable");
        }
        other => panic!("expected ValidationError(chain_missing_contracts_subtable); got {other:?}"),
    }
}

#[test]
fn load_accepts_lowercase_addresses_in_contracts() {
    let toml = r#"
        [chains]
        allow = [31337]

        [contracts.31337]
        allow = ["0x5fbdb2315678afecb367f032d93f642f64180aa3"]
    "#;
    parse_policy_str(toml).expect("lowercase addresses parse via lenient EIP-55");
}

#[test]
fn load_rejects_mixed_case_bad_checksum_address() {
    // Take the canonical EIP-55 form and flip one alpha char's case.
    let canon = "0x5FbDB2315678afecb367f032d93F642f64180aa3";
    // Flip char at index 3 (the canonical 'b' → 'B') to break the checksum.
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
    assert_ne!(bad, canon, "must have flipped at least one char");

    let toml = format!(
        "[chains]\nallow = [31337]\n[contracts.31337]\nallow = [\"{bad}\"]\n"
    );
    let err = parse_policy_str(&toml).unwrap_err();
    assert!(matches!(err, PolicyError::ValidationError { .. }));
}

#[test]
fn load_parses_selector_hex_and_any_sentinel() {
    let toml = r#"
        [chains]
        allow = [31337]

        [contracts.31337]
        allow = ["0x5fbdb2315678afecb367f032d93f642f64180aa3"]

        [selectors."31337:0x5fbdb2315678afecb367f032d93f642f64180aa3"]
        allow = ["0xa9059cbb", "any"]
    "#;
    let loaded = parse_policy_str(toml).expect("parse ok");
    let addr = alloy_primitives::Address::parse_checksummed(
        "0x5FbDB2315678afecb367f032d93F642f64180aa3",
        None,
    )
    .unwrap();
    let key = executor_policy::ChainContract::new(31337, addr);
    let patterns = loaded
        .selectors_by_chain_contract
        .get(&key)
        .expect("selectors key resolves");
    assert_eq!(patterns.len(), 2);
    assert_eq!(
        patterns[0],
        SelectorPattern::Specific([0xa9, 0x05, 0x9c, 0xbb])
    );
    assert!(matches!(patterns[1], SelectorPattern::Any));
}

#[test]
fn load_rejects_bad_selector_hex_format() {
    let toml = r#"
        [chains]
        allow = [31337]

        [contracts.31337]
        allow = ["0x5fbdb2315678afecb367f032d93f642f64180aa3"]

        [selectors."31337:0x5fbdb2315678afecb367f032d93f642f64180aa3"]
        allow = ["0xZZZZZZZZ"]
    "#;
    let err = parse_policy_str(toml).unwrap_err();
    match err {
        PolicyError::ValidationError { category, .. } => {
            assert_eq!(category.as_ref(), "bad_selector_hex");
        }
        other => panic!("expected ValidationError(bad_selector_hex); got {other:?}"),
    }
}

#[test]
fn load_parses_native_value_decimal_cap() {
    let toml = r#"
        [chains]
        allow = [31337]

        [contracts.31337]
        allow = []

        [native_value.31337]
        max_per_action = "1000000000000000000"
    "#;
    let loaded = parse_policy_str(toml).expect("parse ok");
    assert_eq!(
        loaded
            .native_value_by_chain
            .get(&31337u64)
            .copied()
            .unwrap(),
        alloy_primitives::U256::from(1_000_000_000_000_000_000u64),
    );
}

#[test]
fn load_rejects_negative_u256() {
    let toml = r#"
        [chains]
        allow = [31337]

        [contracts.31337]
        allow = []

        [native_value.31337]
        max_per_action = "-1"
    "#;
    let err = parse_policy_str(toml).unwrap_err();
    match err {
        PolicyError::ValidationError { category, .. } => {
            assert_eq!(category.as_ref(), "bad_u256_negative");
        }
        other => panic!("expected ValidationError(bad_u256_negative); got {other:?}"),
    }
}

#[test]
fn policy_error_data_kind_dispatcher() {
    use std::borrow::Cow;
    assert_eq!(
        PolicyError::FileNotFound {
            detail_for_log: "x".into()
        }
        .data_kind(),
        "policy_not_loaded",
    );
    assert_eq!(
        PolicyError::Config {
            category: Cow::Borrowed("toml_parse"),
            detail_for_log: "y".into()
        }
        .data_kind(),
        "policy_config_error",
    );
    assert_eq!(
        PolicyError::ValidationError {
            category: Cow::Borrowed("bad_address"),
            detail_for_log: "y".into()
        }
        .data_kind(),
        "policy_config_error",
    );
    assert_eq!(
        PolicyError::Denied {
            rule: Cow::Borrowed("contract_not_allowed"),
            detail: "z".into(),
            action_index: 0
        }
        .data_kind(),
        "policy_violation",
    );
    assert_eq!(
        PolicyError::Io {
            detail_for_log: "z".into()
        }
        .data_kind(),
        "policy_config_error",
    );
}
