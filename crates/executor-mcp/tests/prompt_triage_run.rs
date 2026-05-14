//! v1.11 Track E2 — `triage_run` prompt integration tests.
//!
//! Each test seeds an out-of-band SQLite fixture (strategy + run +
//! journal/decision rows) via `StateStore::open`, then drives the stdio
//! MCP binary pointed at that DB and calls `prompts/get triage_run` to
//! assert the composed markdown report. RPC is intentionally never
//! configured — the receipt-fetch path degrades gracefully (no provider →
//! receipt section omitted), so these tests run hermetically.

mod common;

use anyhow::Result;
use serde_json::{Value, json};

use common::{initialize, recv, send, spawn_server_with_state};

/// Seed a strategy + a run with caller-supplied status + a single
/// `Action` journal row whose payload_json carries `tx_hash`. Returns
/// `(run_id, strategy_id)`.
fn seed_run_with_action(
    db_path: &std::path::Path,
    strategy_name: &str,
    run_status: executor_core::schema::execution::RunStatus,
    tx_hash: Option<&str>,
    action_status: &str,
    error_kind: Option<&str>,
    error_detail: Option<&str>,
) -> Result<(String, String)> {
    use executor_core::schema::execution::JournalActionOutcome;
    use executor_state::{RegisterOutcome, StateStore};
    let mut store = StateStore::open(db_path)?;
    let sid = match store
        .register_strategy(strategy_name, "// noop", None, None)?
    {
        RegisterOutcome::Created(s) | RegisterOutcome::AlreadyExists(s) => s.id,
        RegisterOutcome::ReplacedVersion { created, .. } => created.id,
    };
    let run_id = store.insert_run(&sid, run_status)?;
    // Journal action carrying tx_hash so the prompt's payload_json walk
    // discovers a tx to receipt-fetch (which degrades to NoProvider here).
    let payload = match tx_hash {
        Some(tx) => json!({
            "action": { "kind": "contract_call", "address": "0x1111111111111111111111111111111111111111", "function": "supply" },
            "tx_hash": tx
        }),
        None => json!({
            "action": { "kind": "contract_call", "address": "0x1111111111111111111111111111111111111111", "function": "supply" }
        }),
    };
    let outcome = match action_status {
        "succeeded" => JournalActionOutcome::Actions,
        "failed" => JournalActionOutcome::RuntimeError,
        _ => JournalActionOutcome::Actions,
    };
    store.record_action_outcome(&run_id, outcome, &payload.to_string())?;
    // Mirror onto execution_actions so `execution://{run_id}.actions` has
    // a row with the expected status / receipt / error fields.
    if let Some(tx) = tx_hash {
        store.record_execution_broadcast(&run_id, 0, "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", tx)?;
        if action_status == "succeeded" {
            store.record_execution_receipt_success(&run_id, 0, "success", "21000")?;
        } else {
            store.record_execution_error(
                &run_id,
                0,
                Some("0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
                error_kind.unwrap_or("revert"),
                error_detail,
            )?;
        }
    } else if action_status == "failed" {
        // Pre-broadcast failure (no tx_hash) — still write a failed
        // execution_action so `execution://{run_id}.actions` reflects it.
        store.record_execution_error(
            &run_id,
            0,
            None,
            error_kind.unwrap_or("validation_error"),
            error_detail,
        )?;
    }
    drop(store);
    Ok((run_id, sid))
}

fn seed_noop_run(db_path: &std::path::Path, strategy_name: &str) -> Result<(String, String)> {
    use executor_core::schema::execution::{JournalActionOutcome, RunStatus};
    use executor_state::{RegisterOutcome, StateStore};
    let mut store = StateStore::open(db_path)?;
    let sid = match store
        .register_strategy(strategy_name, "// noop", None, None)?
    {
        RegisterOutcome::Created(s) | RegisterOutcome::AlreadyExists(s) => s.id,
        RegisterOutcome::ReplacedVersion { created, .. } => created.id,
    };
    let run_id = store.insert_run(&sid, RunStatus::Succeeded)?;
    store.record_action_outcome(&run_id, JournalActionOutcome::Noop, "{}")?;
    drop(store);
    Ok((run_id, sid))
}

fn add_policy_deny(
    db_path: &std::path::Path,
    run_id: &str,
    rule: &str,
    detail: &str,
) -> Result<()> {
    use executor_state::{DecisionGate, DecisionVerdict, StateStore};
    let mut store = StateStore::open(db_path)?;
    store.record_decision(
        run_id,
        0,
        DecisionGate::Policy,
        DecisionVerdict::Fail,
        Some(rule),
        Some(detail),
        None,
    )?;
    drop(store);
    Ok(())
}

async fn call_triage(proc: &mut common::ServerProc, id: u64, run_id: &str) -> Result<Value> {
    send(
        proc,
        json!({
            "jsonrpc": "2.0", "id": id, "method": "prompts/get",
            "params": { "name": "triage_run", "arguments": { "run_id": run_id } }
        }),
    )
    .await?;
    recv(proc).await
}

fn report_text(r: &Value) -> String {
    r["result"]["messages"][0]["content"]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("triage_run result missing text: {r}"))
        .to_string()
}

fn tmp_db_path() -> (tempfile::TempPath, std::path::PathBuf) {
    let tmp = tempfile::NamedTempFile::new().expect("tempfile");
    let path = tmp.path().to_path_buf();
    (tmp.into_temp_path(), path)
}

#[tokio::test]
async fn triage_run_happy_path_renders_all_five_sections() -> Result<()> {
    use executor_core::schema::execution::RunStatus;
    let (guard, db_path) = tmp_db_path();
    let (run_id, _sid) = seed_run_with_action(
        &db_path,
        "happy-strat",
        RunStatus::Succeeded,
        Some("0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890"),
        "succeeded",
        None,
        None,
    )?;
    // Keep the temp file alive until end of test.
    let _ = guard.keep()?;

    let mut proc = spawn_server_with_state(&db_path.to_string_lossy()).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_triage(&mut proc, 2, &run_id).await?;
    let text = report_text(&r);
    for required in [
        "# Run ",
        "## What ran",
        "## What succeeded",
        "## What failed",
        "## Likely cause",
        "## Next actions",
    ] {
        assert!(text.contains(required), "missing section `{required}` in: {text}");
    }
    // "What failed" must show the empty marker on a happy run.
    let what_failed = text
        .split("## What failed")
        .nth(1)
        .and_then(|s| s.split("## Likely cause").next())
        .unwrap_or_default();
    assert!(
        what_failed.contains("_(none)_"),
        "happy-path What-failed should be empty marker, got: {what_failed}"
    );
    assert!(text.contains("Outcome: succeeded"));
    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn triage_run_policy_deny_likely_cause_mentions_policy_current() -> Result<()> {
    use executor_core::schema::execution::RunStatus;
    let (guard, db_path) = tmp_db_path();
    let (run_id, _sid) = seed_run_with_action(
        &db_path,
        "deny-strat",
        RunStatus::PolicyDenied,
        None,
        "failed",
        Some("policy_denied"),
        Some("contract not allow-listed"),
    )?;
    add_policy_deny(&db_path, &run_id, "contracts_by_chain", "address 0x1111 not in allow list")?;
    let _ = guard.keep()?;

    let mut proc = spawn_server_with_state(&db_path.to_string_lossy()).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_triage(&mut proc, 2, &run_id).await?;
    let text = report_text(&r);

    let cause = text
        .split("## Likely cause")
        .nth(1)
        .and_then(|s| s.split("## Next actions").next())
        .unwrap_or_default();
    assert!(
        cause.contains("policy://current"),
        "policy-deny likely cause must mention policy://current; got: {cause}"
    );
    assert!(
        cause.contains("contracts_by_chain"),
        "policy-deny likely cause must include the rule; got: {cause}"
    );
    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn triage_run_revert_likely_cause_mentions_revert_reason() -> Result<()> {
    use executor_core::schema::execution::RunStatus;
    let (guard, db_path) = tmp_db_path();
    let (run_id, _sid) = seed_run_with_action(
        &db_path,
        "revert-strat",
        RunStatus::Failed,
        Some("0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef"),
        "failed",
        Some("revert"),
        Some("ERC20: insufficient allowance"),
    )?;
    let _ = guard.keep()?;

    let mut proc = spawn_server_with_state(&db_path.to_string_lossy()).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_triage(&mut proc, 2, &run_id).await?;
    let text = report_text(&r);

    let cause = text
        .split("## Likely cause")
        .nth(1)
        .and_then(|s| s.split("## Next actions").next())
        .unwrap_or_default();
    assert!(
        cause.contains("insufficient") || cause.contains("allowance"),
        "revert likely cause must echo the revert reason; got: {cause}"
    );
    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn triage_run_noop_likely_cause_mentions_strategy_resource() -> Result<()> {
    let (guard, db_path) = tmp_db_path();
    let (run_id, sid) = seed_noop_run(&db_path, "noop-strat")?;
    let _ = guard.keep()?;

    let mut proc = spawn_server_with_state(&db_path.to_string_lossy()).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_triage(&mut proc, 2, &run_id).await?;
    let text = report_text(&r);

    let cause = text
        .split("## Likely cause")
        .nth(1)
        .and_then(|s| s.split("## Next actions").next())
        .unwrap_or_default();
    assert!(
        cause.contains("noop") || cause.contains("entry condition"),
        "noop likely cause must mention noop / entry condition; got: {cause}"
    );
    assert!(
        cause.contains(&format!("strategy://{sid}")) || cause.contains("strategy://"),
        "noop likely cause must reference strategy://{{id}}; got: {cause}"
    );
    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn triage_run_malformed_run_id_returns_invalid_params() -> Result<()> {
    let mut proc = common::spawn_server().await?;
    let _ = initialize(&mut proc).await?;
    let r = call_triage(&mut proc, 2, "not-a-ulid").await?;
    assert_eq!(
        r["error"]["code"], -32602,
        "malformed run_id must surface as invalid_params: {r}"
    );
    let hint = r["error"]["data"]["hint"]
        .as_str()
        .unwrap_or_default();
    assert!(
        hint.contains("execution://list"),
        "invalid_params hint must point at execution://list; got: {hint}"
    );
    proc.child.kill().await?;
    Ok(())
}
