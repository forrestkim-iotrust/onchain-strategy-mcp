//! Typed [`PolicyError`] (Phase 5 D-13 / Phase 4 MR-01 carry-forward).
//!
//! `Display` is wire-safe: it emits ONLY the stable taxonomy prefixes
//! (`"policy violation: "`, `"policy config error: "`, `"policy io error"`).
//! Raw toml / serde / fs error text lives in the per-variant `detail_for_log`
//! field and is intended for `tracing::warn!` consumption at the MCP boundary
//! — NEVER `format!` it onto the wire.

use std::borrow::Cow;

/// Typed policy errors.
#[derive(Debug, thiserror::Error)]
pub enum PolicyError {
    /// Policy file load / IO failure.
    #[error("policy io error")]
    Io { detail_for_log: String },

    /// Policy file does not exist at the configured path.
    #[error("policy violation: policy file not loaded")]
    FileNotFound { detail_for_log: String },

    /// TOML parse / shape failure or address / U256 / selector parse failure
    /// during load.
    #[error("policy config error: {category}")]
    Config {
        category: Cow<'static, str>,
        detail_for_log: String,
    },

    /// Validation of the parsed `PolicyConfig` (e.g. a chain in
    /// `[chains.allow]` without a corresponding `[contracts.<id>]` sub-table —
    /// Pitfall P-10).
    #[error("policy config error: {category}")]
    ValidationError {
        category: Cow<'static, str>,
        detail_for_log: String,
    },

    /// Policy runtime denial (constructed by `eval` in Plan 05-03).
    #[error("policy violation: {rule}")]
    Denied {
        rule: Cow<'static, str>,
        detail: String,
        action_index: u32,
    },
}

impl PolicyError {
    /// `data.kind` dispatch — Phase 5 mirrors Phase 4 D-12 taxonomy.
    pub fn data_kind(&self) -> &'static str {
        match self {
            Self::FileNotFound { .. } => "policy_not_loaded",
            Self::Io { .. } | Self::Config { .. } | Self::ValidationError { .. } => {
                "policy_config_error"
            }
            Self::Denied { .. } => "policy_violation",
        }
    }

    /// Operator-only diagnostic text. Routed to `tracing::warn!` at the MCP
    /// boundary; NEVER formatted onto the wire.
    pub fn detail_for_log(&self) -> &str {
        match self {
            Self::Io { detail_for_log }
            | Self::FileNotFound { detail_for_log }
            | Self::Config { detail_for_log, .. }
            | Self::ValidationError { detail_for_log, .. } => detail_for_log,
            Self::Denied { detail, .. } => detail,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_strings_are_stable_and_wire_safe() {
        // Variant: Io — raw fs text must not leak.
        let e = PolicyError::Io {
            detail_for_log: "fs::read /etc/passwd: permission denied".into(),
        };
        assert_eq!(e.to_string(), "policy io error");
        assert!(!e.to_string().contains("permission denied"));
        assert!(!e.to_string().contains("/etc/passwd"));

        // Variant: FileNotFound — path must not leak.
        let e = PolicyError::FileNotFound {
            detail_for_log: "/foo/policy.toml".into(),
        };
        assert!(e.to_string().starts_with("policy violation: "));
        assert!(!e.to_string().contains("/foo/policy.toml"));

        // Variant: Config — raw toml::de::Error must not leak.
        let e = PolicyError::Config {
            category: Cow::Borrowed("bad_address"),
            detail_for_log: "raw toml::de::Error message at line 3 col 1".into(),
        };
        assert_eq!(e.to_string(), "policy config error: bad_address");
        assert!(!e.to_string().contains("toml::de"));
        assert!(!e.to_string().contains("line 3"));

        // Variant: ValidationError — same shape as Config.
        let e = PolicyError::ValidationError {
            category: Cow::Borrowed("contract_chain_mismatch"),
            detail_for_log: "chain 31337 in [chains.allow] but [contracts.31337] missing".into(),
        };
        assert_eq!(
            e.to_string(),
            "policy config error: contract_chain_mismatch"
        );

        // Variant: Denied — rule taxonomy reaches the wire; raw detail does not.
        let e = PolicyError::Denied {
            rule: Cow::Borrowed("contract_not_allowed"),
            detail: "contract 0xdead... not allowed on chain 31337".into(),
            action_index: 1,
        };
        assert!(e.to_string().starts_with("policy violation: "));
        assert!(e.to_string().contains("contract_not_allowed"));
        assert!(!e.to_string().contains("0xdead"));
    }

    #[test]
    fn data_kind_groups_variants() {
        assert_eq!(
            PolicyError::Io {
                detail_for_log: "x".into()
            }
            .data_kind(),
            "policy_config_error"
        );
        assert_eq!(
            PolicyError::FileNotFound {
                detail_for_log: "x".into()
            }
            .data_kind(),
            "policy_not_loaded"
        );
        assert_eq!(
            PolicyError::Config {
                category: Cow::Borrowed("c"),
                detail_for_log: "x".into()
            }
            .data_kind(),
            "policy_config_error"
        );
        assert_eq!(
            PolicyError::ValidationError {
                category: Cow::Borrowed("c"),
                detail_for_log: "x".into()
            }
            .data_kind(),
            "policy_config_error"
        );
        assert_eq!(
            PolicyError::Denied {
                rule: Cow::Borrowed("r"),
                detail: "d".into(),
                action_index: 0,
            }
            .data_kind(),
            "policy_violation"
        );
    }

    #[test]
    fn detail_for_log_returns_raw_text() {
        let e = PolicyError::Io {
            detail_for_log: "boom".into(),
        };
        assert_eq!(e.detail_for_log(), "boom");
        let e = PolicyError::Denied {
            rule: Cow::Borrowed("r"),
            detail: "raw detail".into(),
            action_index: 0,
        };
        assert_eq!(e.detail_for_log(), "raw detail");
    }
}
