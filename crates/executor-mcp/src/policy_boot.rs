//! v1.5 Track 1A — boot-time policy resolution.
//!
//! Policy storage migrated from `.local/policy.toml` (v1.4) to the SQLite
//! `policies` table. Boot order:
//!
//! 1. If `policies` has an active row → load it. The TOML file is ignored.
//! 2. Else, if `[policy].path` is configured AND that file exists → parse
//!    the TOML, serialize the parsed `PolicyConfig` to canonical JSON, and
//!    write it as the first DB revision with rationale `"initial import
//!    from .local/policy.toml"`. Log a warning suggesting the operator
//!    delete or gitignore the now-redundant TOML.
//! 3. Else → no policy. Server still boots; `strategy_run` returns
//!    -32017 `policy_not_loaded` (D-15 fail-closed).
//!
//! Malformed TOML or address-parse failures during the import path do NOT
//! panic — we log `tracing::error!` and return `None`. The operator's
//! recovery action is to call the `policy_set` MCP tool with a corrected
//! JSON body.

use anyhow::Result;
use executor_policy::{LoadedPolicy, PolicyConfig, resolve_config};
use executor_state::StateStore;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Resolve the boot-time policy. Returns `Ok(None)` for the fail-closed
/// path; `Err(...)` only if a storage error (DB open / SQL execute) makes
/// the store fundamentally unusable — which would already have failed in
/// `StateStore::open` upstream, so this is mostly defensive.
pub fn resolve_boot_policy(
    state: &Arc<Mutex<StateStore>>,
    toml_path: Option<&str>,
) -> Result<Option<LoadedPolicy>> {
    // try_lock is safe here: the server Arc was just constructed in
    // `new_with_full_config` and no other task can hold the mutex yet.
    let mut store = state.try_lock().map_err(|e| {
        anyhow::anyhow!("boot policy resolve: state mutex unexpectedly contended: {e}")
    })?;

    // 1. DB has an active row → that wins.
    match store.get_active_policy() {
        Ok(Some(rev)) => {
            let cfg = match serde_json::from_str::<PolicyConfig>(&rev.body_json) {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!(
                        revision_id = %rev.revision_id,
                        detail = %e,
                        "active policy revision failed to deserialize — fail-closed; \
                         recovery: call policy_set with a corrected JSON body",
                    );
                    return Ok(None);
                }
            };
            return match resolve_config(cfg) {
                Ok(loaded) => {
                    tracing::info!(
                        revision_id = %rev.revision_id,
                        chains = ?loaded.chains_allow,
                        raw_call_global = loaded.raw_call_allow_global,
                        "policy loaded from DB revision",
                    );
                    Ok(Some(loaded))
                }
                Err(e) => {
                    tracing::error!(
                        revision_id = %rev.revision_id,
                        detail = %e.detail_for_log(),
                        kind = %e.data_kind(),
                        "active policy revision failed to resolve — fail-closed; \
                         recovery: call policy_set with a corrected JSON body",
                    );
                    Ok(None)
                }
            };
        }
        Ok(None) => {}
        Err(e) => {
            tracing::error!(
                detail = %e,
                "reading active policy revision failed — fail-closed",
            );
            return Ok(None);
        }
    }

    // 2. DB is empty. Attempt one-shot TOML import if path is configured.
    let Some(toml_path) = toml_path else {
        tracing::warn!(
            "no policy in DB and [policy].path not configured — \
             strategy_run will fail-closed; call policy_set to install one",
        );
        return Ok(None);
    };
    let path = Path::new(toml_path);
    if !path.exists() {
        tracing::warn!(
            path = %toml_path,
            "no policy in DB and [policy].path file does not exist — \
             strategy_run will fail-closed; call policy_set to install one",
        );
        return Ok(None);
    }

    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(
                path = %toml_path,
                detail = %e,
                "TOML import failed at read step — fail-closed; \
                 recovery: call policy_set with a JSON body",
            );
            return Ok(None);
        }
    };
    // First parse to PolicyConfig so we can re-serialize as canonical JSON
    // for storage. Validate by also resolving to LoadedPolicy in one shot.
    let parsed_cfg: PolicyConfig = match toml::from_str(&raw) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(
                path = %toml_path,
                detail = %e,
                "TOML import failed at parse step — fail-closed; \
                 recovery: call policy_set with a corrected JSON body",
            );
            return Ok(None);
        }
    };
    // Re-validate via parse_policy_str's address/U256/selector logic so a
    // malformed TOML import does not silently land in the DB. We use
    // resolve_config (same logic) on the parsed PolicyConfig.
    let loaded = match resolve_config(parsed_cfg.clone()) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(
                path = %toml_path,
                detail = %e.detail_for_log(),
                kind = %e.data_kind(),
                "TOML import failed at validate step — fail-closed; \
                 recovery: call policy_set with a corrected JSON body",
            );
            return Ok(None);
        }
    };
    // Serialize the validated config to canonical JSON. `PolicyConfig` is
    // Serialize and round-trips cleanly with serde_json.
    let body_json = match serde_json::to_string(&parsed_cfg) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(
                detail = %e,
                "TOML import failed at serialize step — fail-closed",
            );
            return Ok(None);
        }
    };
    let rationale = Some("initial import from .local/policy.toml");
    match store.set_active_policy(&body_json, rationale) {
        Ok(rev) => {
            tracing::info!(
                revision_id = %rev.revision_id,
                path = %toml_path,
                "imported policy from TOML — DB is now source of truth. \
                 You can safely delete or gitignore {toml_path}; subsequent \
                 edits MUST go through the `policy_set` MCP tool.",
                toml_path = toml_path,
            );
            Ok(Some(loaded))
        }
        Err(e) => {
            tracing::error!(
                detail = %e,
                "TOML import failed at DB write step — fail-closed",
            );
            Ok(None)
        }
    }
}

// Also expose the parse path used inside the tool layer so we don't
// duplicate logic. Tool callers want { LoadedPolicy, canonical_json } pair.
pub fn parse_policy_json(
    body: &serde_json::Value,
) -> Result<(LoadedPolicy, String), executor_policy::PolicyError> {
    // PolicyConfig: Deserialize + Serialize → round-trip safe canonical JSON.
    let cfg: PolicyConfig =
        serde_json::from_value(body.clone()).map_err(|e| executor_policy::PolicyError::Config {
            category: std::borrow::Cow::Borrowed("bad_json_shape"),
            detail_for_log: format!("serde_json::from_value: {e}"),
        })?;
    let loaded = resolve_config(cfg.clone())?;
    let canonical = serde_json::to_string(&cfg).map_err(|e| {
        executor_policy::PolicyError::Config {
            category: std::borrow::Cow::Borrowed("canonicalize_failed"),
            detail_for_log: format!("serde_json::to_string: {e}"),
        }
    })?;
    Ok((loaded, canonical))
}

#[cfg(test)]
mod tests {
    use super::*;
    use executor_state::StateStore;
    use std::path::PathBuf;
    use tempfile::TempDir;
    use tokio::sync::Mutex;

    fn open_store(tmp: &TempDir) -> Arc<Mutex<StateStore>> {
        let path: PathBuf = tmp.path().join("state.db");
        Arc::new(Mutex::new(StateStore::open(&path).expect("open store")))
    }

    #[test]
    fn no_db_no_toml_returns_none() {
        let tmp = TempDir::new().unwrap();
        let store = open_store(&tmp);
        let loaded = resolve_boot_policy(&store, None).expect("resolve");
        assert!(loaded.is_none());
    }

    #[test]
    fn missing_toml_path_returns_none() {
        let tmp = TempDir::new().unwrap();
        let store = open_store(&tmp);
        let loaded = resolve_boot_policy(&store, Some("/no/such/__missing__.toml"))
            .expect("resolve");
        assert!(loaded.is_none());
    }

    #[test]
    fn toml_import_writes_first_revision() {
        let tmp = TempDir::new().unwrap();
        let store = open_store(&tmp);

        // Write a valid TOML.
        let toml = r#"
            [chains]
            allow = [31337]

            [contracts.31337]
            allow = ["0x5fbdb2315678afecb367f032d93f642f64180aa3"]
        "#;
        let toml_path = tmp.path().join("policy.toml");
        std::fs::write(&toml_path, toml).unwrap();

        let loaded =
            resolve_boot_policy(&store, Some(toml_path.to_str().unwrap())).expect("resolve");
        assert!(loaded.is_some(), "policy should load via import path");
        assert!(loaded.unwrap().chains_allow.contains(&31337));

        // Verify the DB now has one active row.
        let store_guard = store.try_lock().expect("lock");
        let active = store_guard.get_active_policy().unwrap();
        assert!(active.is_some());
        assert_eq!(
            active.unwrap().rationale.as_deref(),
            Some("initial import from .local/policy.toml"),
        );
    }

    #[test]
    fn malformed_toml_returns_none_no_panic() {
        let tmp = TempDir::new().unwrap();
        let store = open_store(&tmp);

        // Invalid TOML — missing the [contracts.31337] subtable that
        // Pitfall P-10 requires when [chains.allow] lists 31337.
        let toml = r#"
            [chains]
            allow = [31337]
        "#;
        let toml_path = tmp.path().join("policy.toml");
        std::fs::write(&toml_path, toml).unwrap();

        let loaded =
            resolve_boot_policy(&store, Some(toml_path.to_str().unwrap())).expect("resolve");
        assert!(
            loaded.is_none(),
            "malformed TOML must fail-closed without panic"
        );

        // Verify the DB has NO row (we never write an invalid policy).
        let store_guard = store.try_lock().expect("lock");
        assert!(store_guard.get_active_policy().unwrap().is_none());
    }

    #[test]
    fn db_revision_wins_over_toml_path() {
        let tmp = TempDir::new().unwrap();
        let store = open_store(&tmp);

        // Pre-seed a DB revision.
        let body = r#"{"chains":{"allow":[31337]},"contracts":{"31337":{"allow":["0x5fbdb2315678afecb367f032d93f642f64180aa3"]}},"selectors":{},"native_value":{},"erc20_spend":{},"raw_call":{"allow_global":false,"allow":[]}}"#;
        {
            let mut s = store.try_lock().expect("lock");
            s.set_active_policy(body, Some("seed")).unwrap();
        }

        // Pre-create an unrelated TOML at a different path — must be ignored.
        let toml_path = tmp.path().join("policy.toml");
        std::fs::write(&toml_path, "[chains]\nallow = [1]\n").unwrap();

        let loaded =
            resolve_boot_policy(&store, Some(toml_path.to_str().unwrap())).expect("resolve");
        let loaded = loaded.expect("active revision should load");
        assert_eq!(loaded.chains_allow, vec![31337]);
    }
}
