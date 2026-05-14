//! v1.10 named actions integration tests.
//!
//! Validates:
//!   1. `strategy_run` without `action` is unchanged (execute path).
//!   2. `strategy_run` with `action` invokes `bundle.actions[name]` and
//!      stamps `runs.action` so the audit trail differentiates the call.
//!   3. `action_changed` flips in `ReplacedVersionInfo` on re-register
//!      while lineage_id stays put.
//!   4. Empty / whitespace / reserved / unknown action names are rejected
//!      with `unknown_action` invalid_params, listing the available choices.
//!   5. Register-time guards: reserved keys, empty source bodies, and
//!      whitespace-only names all return -32602 before any DB write.
//!
//! Strategy: drive `StateStore::register_strategy_bundle` directly for the
//! storage assertions (mirrors `lineage.rs`), and drive the JSON-RPC
//! `strategy_run` tool over stdio for the dispatch / error assertions.

mod common;

use anyhow::Result;
use executor_state::{RegisterOutcome, StateStore};
use serde_json::{Value, json};
use std::collections::HashMap;
use tempfile::tempdir;

use common::{initialize, recv, send, spawn_server_with_state};

fn register_with_actions(
    store: &mut StateStore,
    name: &str,
    source: &str,
    actions_json: Option<&str>,
) -> RegisterOutcome {
    store
        .register_strategy_bundle(name, source, None, None, None, None, None, actions_json)
        .expect("register")
}

#[test]
fn action_change_only_bumps_version_and_lineage_holds() {
    let tmp = tempdir().expect("tmp");
    let path = tmp.path().join("a.db");
    let mut store = StateStore::open(&path).expect("open");

    let v1 = register_with_actions(&mut store, "rotator", "// exec", None);
    let v1_active = v1.active_strategy().clone();
    assert!(v1_active.actions_json.is_none(), "v1 has no actions");

    // v2: identical execute, identical records/view (both None), but adds
    // one action. Lineage must persist, version must bump, actions_changed
    // must be true, execute/records/view all unchanged.
    let actions_v2 = r#"{"withdraw":"(ctx) => 'noop'"}"#;
    let v2 = register_with_actions(&mut store, "rotator", "// exec", Some(actions_v2));
    match v2 {
        RegisterOutcome::ReplacedVersion {
            created,
            previous,
            new_version,
            previous_version,
            execute_changed,
            records_changed,
            view_changed,
            actions_changed,
        } => {
            assert_eq!(created.lineage_id, v1_active.lineage_id);
            assert_eq!(previous.id, v1_active.id);
            assert!(previous.deleted_at.is_some());
            assert_eq!(new_version, 2);
            assert_eq!(previous_version, 1);
            assert!(!execute_changed, "execute body unchanged");
            assert!(!records_changed);
            assert!(!view_changed);
            assert!(actions_changed, "actions appeared — must flag the change");
            assert_eq!(created.actions_json.as_deref(), Some(actions_v2));
        }
        other => panic!("expected ReplacedVersion, got {other:?}"),
    }

    // v3: actions map is identical → no version bump (AlreadyExists).
    match register_with_actions(&mut store, "rotator", "// exec", Some(actions_v2)) {
        RegisterOutcome::AlreadyExists(existing) => {
            assert_eq!(existing.lineage_id, v1_active.lineage_id);
            assert_eq!(existing.actions_json.as_deref(), Some(actions_v2));
        }
        other => panic!("expected AlreadyExists, got {other:?}"),
    }
}

#[test]
fn action_names_surface_on_summary_list() {
    let tmp = tempdir().expect("tmp");
    let path = tmp.path().join("b.db");
    let mut store = StateStore::open(&path).expect("open");

    let actions = r#"{"alpha":"(ctx) => 'noop'","beta":"(ctx) => 'noop'"}"#;
    register_with_actions(&mut store, "two-actions", "// exec", Some(actions));

    let list = store.list_strategies(false).expect("list");
    let row = list
        .iter()
        .find(|s| s.name == "two-actions")
        .expect("found");
    assert_eq!(row.action_names, vec!["alpha".to_string(), "beta".to_string()]);
}

// ---- JSON-RPC dispatch tests ----

fn read_response(r: &Value) -> Value {
    r["result"]["content"][0]["text"]
        .as_str()
        .map(|s| serde_json::from_str::<Value>(s).expect("response JSON"))
        .unwrap_or_else(|| r["result"].clone())
}

async fn register_via_rpc(
    proc: &mut common::ServerProc,
    id: i64,
    body: Value,
) -> Result<Value> {
    send(
        proc,
        json!({
            "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": { "name": "strategy_register", "arguments": body }
        }),
    )
    .await?;
    Ok(recv(proc).await?)
}

async fn run_via_rpc(
    proc: &mut common::ServerProc,
    id: i64,
    args: Value,
) -> Result<Value> {
    send(
        proc,
        json!({
            "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": { "name": "strategy_run", "arguments": args }
        }),
    )
    .await?;
    Ok(recv(proc).await?)
}

#[tokio::test]
async fn strategy_run_with_action_routes_to_named_function() -> Result<()> {
    let tmp = tempfile::NamedTempFile::new()?;
    let db_path = tmp.path().to_path_buf();
    let _ = tmp.into_temp_path().keep()?;

    // Pre-seed: bundle whose execute returns "noop" and actions.beta returns
    // the explicit string "beta-was-called" via output validation. Since
    // strategy_run validates output as Action[]|"noop", we make beta return
    // "noop" and prove the routing by inspecting runs.action stamped on
    // the row.
    let sid = {
        let mut store = StateStore::open(&db_path)?;
        let actions = r#"{"beta":"(ctx) => 'noop'"}"#;
        match register_with_actions(&mut store, "with-actions", "(ctx) => 'noop'", Some(actions)) {
            RegisterOutcome::Created(s)
            | RegisterOutcome::AlreadyExists(s)
            | RegisterOutcome::ReplacedVersion { created: s, .. } => s.id,
        }
    };

    let db_str = db_path.to_string_lossy().to_string();
    let mut proc = spawn_server_with_state(&db_str).await?;
    let _ = initialize(&mut proc).await?;

    // Run WITHOUT action → runs.action stays NULL.
    let r1 = run_via_rpc(&mut proc, 10, json!({"strategy_id": sid})).await?;
    assert!(r1.get("error").is_none(), "execute run failed: {r1}");
    let resp1 = read_response(&r1);
    let run_id_1 = resp1["run_id"].as_str().unwrap().to_string();

    // Run WITH action → runs.action == "beta".
    let r2 = run_via_rpc(
        &mut proc,
        11,
        json!({"strategy_id": sid, "action": "beta"}),
    )
    .await?;
    assert!(r2.get("error").is_none(), "action run failed: {r2}");
    let resp2 = read_response(&r2);
    let run_id_2 = resp2["run_id"].as_str().unwrap().to_string();
    assert_ne!(run_id_1, run_id_2);

    // Reach back into the DB and verify the action column.
    let store = StateStore::open(&db_path)?;
    let run1 = store.get_run(&run_id_1)?.expect("run1");
    let run2 = store.get_run(&run_id_2)?.expect("run2");
    assert_eq!(run1.action, None, "execute path stays NULL");
    assert_eq!(run2.action.as_deref(), Some("beta"), "action stamped");

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn strategy_run_rejects_unknown_action() -> Result<()> {
    let tmp = tempfile::NamedTempFile::new()?;
    let db_path = tmp.path().to_path_buf();
    let _ = tmp.into_temp_path().keep()?;

    let sid = {
        let mut store = StateStore::open(&db_path)?;
        let actions = r#"{"alpha":"(ctx) => 'noop'"}"#;
        match register_with_actions(&mut store, "guarded", "(ctx) => 'noop'", Some(actions)) {
            RegisterOutcome::Created(s)
            | RegisterOutcome::AlreadyExists(s)
            | RegisterOutcome::ReplacedVersion { created: s, .. } => s.id,
        }
    };

    let db_str = db_path.to_string_lossy().to_string();
    let mut proc = spawn_server_with_state(&db_str).await?;
    let _ = initialize(&mut proc).await?;

    // 1. Unknown action name → unknown_action error.
    let r = run_via_rpc(
        &mut proc,
        20,
        json!({"strategy_id": sid, "action": "gamma"}),
    )
    .await?;
    let err = r["error"].clone();
    assert!(!err.is_null(), "expected error, got {r}");
    assert_eq!(err["code"], json!(-32602), "invalid_params code");
    let data = err["data"].clone();
    assert_eq!(data["kind"], json!("unknown_action"));
    let avail: Vec<String> = data["available_actions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(avail, vec!["alpha".to_string()]);

    // 2. Empty string → unknown_action.
    let r = run_via_rpc(
        &mut proc,
        21,
        json!({"strategy_id": sid, "action": ""}),
    )
    .await?;
    assert_eq!(r["error"]["code"], json!(-32602));
    assert_eq!(r["error"]["data"]["kind"], json!("unknown_action"));

    // 3. Whitespace-only → unknown_action (trim → empty).
    let r = run_via_rpc(
        &mut proc,
        22,
        json!({"strategy_id": sid, "action": "   "}),
    )
    .await?;
    assert_eq!(r["error"]["code"], json!(-32602));
    assert_eq!(r["error"]["data"]["kind"], json!("unknown_action"));

    // 4. Reserved name → unknown_action (with a different hint).
    let r = run_via_rpc(
        &mut proc,
        23,
        json!({"strategy_id": sid, "action": "execute"}),
    )
    .await?;
    assert_eq!(r["error"]["code"], json!(-32602));
    assert_eq!(r["error"]["data"]["kind"], json!("unknown_action"));

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn strategy_register_rejects_reserved_action_names() -> Result<()> {
    let mut proc = common::spawn_server().await?;
    let _ = initialize(&mut proc).await?;

    let mut actions = HashMap::new();
    actions.insert("execute".to_string(), "(ctx) => 'noop'".to_string());

    let r = register_via_rpc(
        &mut proc,
        30,
        json!({
            "name": "reject-reserved",
            "source": "(ctx) => 'noop'",
            "actions": actions,
        }),
    )
    .await?;
    let err = r["error"].clone();
    assert!(!err.is_null(), "expected error, got {r}");
    assert_eq!(err["code"], json!(-32602));
    assert!(
        err["message"].as_str().unwrap_or("").contains("reserved"),
        "message should mention 'reserved'; got: {}",
        err["message"]
    );

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn strategy_register_rejects_empty_action_body() -> Result<()> {
    let mut proc = common::spawn_server().await?;
    let _ = initialize(&mut proc).await?;

    let mut actions = HashMap::new();
    actions.insert("withdrawAll".to_string(), "   ".to_string());

    let r = register_via_rpc(
        &mut proc,
        31,
        json!({
            "name": "empty-body",
            "source": "(ctx) => 'noop'",
            "actions": actions,
        }),
    )
    .await?;
    assert_eq!(r["error"]["code"], json!(-32602));
    assert!(
        r["error"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("non-empty"),
        "message should mention 'non-empty'; got {}",
        r["error"]["message"]
    );

    proc.child.kill().await?;
    Ok(())
}
