//! v1.4 Track A4 — `strategy://{id}/view` + `strategy://{id}/records` integration.
//!
//! - `view_template_registered` + `records_template_registered` assert the
//!   MCP resource-templates handshake advertises both v1.4 URIs.
//! - `view_fallback_for_legacy_strategy` asserts a strategy registered
//!   without `view_source` returns the honesty-contract fallback shape.
//! - `records_endpoint_lists_rows` exercises the raw browse with + without
//!   `since` filtering against directly-seeded `strategy_records_capture`
//!   rows.
//! - `view_endpoint_runs_user_view_against_aggregated_records` is the
//!   dogfood: register a bundle strategy with a `records[].name = "supply"`
//!   spec + a view that returns `{ principal: records.supply.sums.amount }`,
//!   seed one synthetic supply capture row, and assert the read returns the
//!   captured principal under `data`.
//!
//! All tests run against a temp-file SQLite DB so we can pre-seed strategy
//! + capture rows out-of-band before the MCP binary starts.

mod common;

use anyhow::Result;
use serde_json::{Value, json};

use common::{initialize, recv, send, spawn_server, spawn_server_with_state};

fn read_resource_body(r: &Value) -> Value {
    let text = r["result"]["contents"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("resources/read missing contents[0].text: {r}"));
    serde_json::from_str(text)
        .unwrap_or_else(|e| panic!("resource body is not JSON: {e} — text={text}"))
}

#[tokio::test]
async fn view_and_records_templates_registered() -> Result<()> {
    let mut proc = spawn_server().await?;
    let _ = initialize(&mut proc).await?;

    send(
        &mut proc,
        json!({ "jsonrpc": "2.0", "id": 2, "method": "resources/templates/list" }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    let templates = r["result"]["resourceTemplates"]
        .as_array()
        .expect("resourceTemplates array");
    let uris: Vec<&str> = templates
        .iter()
        .map(|t| t["uriTemplate"].as_str().unwrap_or_default())
        .collect();
    assert!(
        uris.contains(&"strategy://{strategy_id}/view"),
        "strategy://{{strategy_id}}/view template must be registered; got {uris:?}"
    );
    assert!(
        uris.contains(&"strategy://{strategy_id}/records"),
        "strategy://{{strategy_id}}/records template must be registered; got {uris:?}"
    );
    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn view_fallback_for_legacy_strategy() -> Result<()> {
    let tmp = tempfile::NamedTempFile::new()?;
    let db_path = tmp.path().to_path_buf();
    let _ = tmp.into_temp_path().keep()?;

    // Seed a legacy (no records/view) strategy.
    let sid = {
        use executor_state::{RegisterOutcome, StateStore};
        let mut store = StateStore::open(&db_path)?;
        match store.register_strategy("legacy", "(ctx) => 'noop'", None, None)? {
            RegisterOutcome::Created(s) | RegisterOutcome::AlreadyExists(s) => s.id,
        }
    };

    let db_str = db_path.to_string_lossy().to_string();
    let mut proc = spawn_server_with_state(&db_str).await?;
    let _ = initialize(&mut proc).await?;

    send(
        &mut proc,
        json!({
            "jsonrpc": "2.0", "id": 2, "method": "resources/read",
            "params": { "uri": format!("strategy://{sid}/view") }
        }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    assert!(r.get("error").is_none(), "expected success, got {r}");
    let body = read_resource_body(&r);
    assert_eq!(body["confidence"], json!("missing"));
    assert!(body.get("reason").is_some(), "fallback must carry reason");
    assert!(
        body.get("remediation").is_some(),
        "fallback must carry remediation"
    );
    assert_eq!(body["data"], Value::Null);

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn records_endpoint_lists_rows_with_since_filter() -> Result<()> {
    let tmp = tempfile::NamedTempFile::new()?;
    let db_path = tmp.path().to_path_buf();
    let _ = tmp.into_temp_path().keep()?;

    let sid = {
        use executor_core::schema::execution::RunStatus;
        use executor_state::{RegisterOutcome, StateStore};
        let mut store = StateStore::open(&db_path)?;
        let sid = match store.register_strategy_bundle(
            "bundle-records",
            "(ctx) => 'noop'",
            None,
            None,
            Some(r#"[{"name":"supply","on":{"kind":"contractCall"},"capture":{"amount":"args[1]"}}]"#),
            Some("(ctx, records) => ({ count: records.supply ? records.supply.count : 0 })"),
            None,
        )? {
            RegisterOutcome::Created(s) | RegisterOutcome::AlreadyExists(s) => s.id,
        };
        // Insert a run + capture rows directly (the capture hook would do
        // this from the run pipeline, but tests stay hermetic by seeding).
        let rid = store.insert_run(&sid, RunStatus::Queued)?;
        // Three captures with different ts via the public façade. The
        // captured_at uses now_rfc3339; we can't override it here, so we
        // sleep slightly between inserts (millisecond precision) to keep
        // them in distinct buckets.
        for i in 0..3 {
            store.record_strategy_capture(&rid, &sid, "supply", &format!(r#"{{"amount":"{i}00"}}"#))?;
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        sid
    };

    let db_str = db_path.to_string_lossy().to_string();
    let mut proc = spawn_server_with_state(&db_str).await?;
    let _ = initialize(&mut proc).await?;

    // Unfiltered.
    send(
        &mut proc,
        json!({
            "jsonrpc": "2.0", "id": 2, "method": "resources/read",
            "params": { "uri": format!("strategy://{sid}/records") }
        }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    assert!(r.get("error").is_none(), "expected success, got {r}");
    let body = read_resource_body(&r);
    let rows = body["records"].as_array().expect("records array");
    assert_eq!(rows.len(), 3, "three seeded captures");
    assert_eq!(body["count"], json!(3));
    // Newest-first ordering — the last seeded row (amount=200) must lead.
    assert_eq!(rows[0]["payload"]["amount"], json!("200"));

    // `since` filter that excludes everything.
    send(
        &mut proc,
        json!({
            "jsonrpc": "2.0", "id": 3, "method": "resources/read",
            "params": { "uri": format!("strategy://{sid}/records?since=2999-01-01T00:00:00Z") }
        }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    let body = read_resource_body(&r);
    assert_eq!(body["count"], json!(0));

    // Bad `since`.
    send(
        &mut proc,
        json!({
            "jsonrpc": "2.0", "id": 4, "method": "resources/read",
            "params": { "uri": format!("strategy://{sid}/records?since=not-a-date") }
        }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    assert!(r.get("error").is_some(), "malformed since must error");
    assert_eq!(r["error"]["code"], -32602);

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn view_endpoint_runs_user_view_against_aggregated_records() -> Result<()> {
    // Dogfood: register a bundle with a `supply` records spec capturing
    // `args[1]`, plus a view function that returns
    // `{ principal: records.supply.sums.amount }`. Seed one capture row
    // (amount="1000000"), then fetch `strategy://{id}/view` and assert.
    let tmp = tempfile::NamedTempFile::new()?;
    let db_path = tmp.path().to_path_buf();
    let _ = tmp.into_temp_path().keep()?;

    let sid = {
        use executor_core::schema::execution::RunStatus;
        use executor_state::{RegisterOutcome, StateStore};
        let mut store = StateStore::open(&db_path)?;
        let records_json = r#"[{
            "name": "supply",
            "on": {"kind":"contractCall","target":"0xA238Dd80C259a72e81d7e4664a9801593F98d1c5","selector":"supply"},
            "capture": {"amount":"args[1]"}
        }]"#;
        let view_source = r#"(ctx, records) => ({
            principal: records.supply ? records.supply.sums.amount : null,
            count: records.supply ? records.supply.count : 0
        })"#;
        let sid = match store.register_strategy_bundle(
            "aave-funnel",
            "(ctx) => 'noop'",
            None,
            None,
            Some(records_json),
            Some(view_source),
            None,
        )? {
            RegisterOutcome::Created(s) | RegisterOutcome::AlreadyExists(s) => s.id,
        };
        let rid = store.insert_run(&sid, RunStatus::Queued)?;
        // Two supply captures: amounts 1_000_000 and 500_000 → sum 1_500_000.
        store.record_strategy_capture(&rid, &sid, "supply", r#"{"amount":"1000000"}"#)?;
        std::thread::sleep(std::time::Duration::from_millis(3));
        store.record_strategy_capture(&rid, &sid, "supply", r#"{"amount":"500000"}"#)?;
        sid
    };

    let db_str = db_path.to_string_lossy().to_string();
    let mut proc = spawn_server_with_state(&db_str).await?;
    let _ = initialize(&mut proc).await?;

    send(
        &mut proc,
        json!({
            "jsonrpc": "2.0", "id": 2, "method": "resources/read",
            "params": { "uri": format!("strategy://{sid}/view") }
        }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    assert!(r.get("error").is_none(), "expected success, got {r}");
    let body = read_resource_body(&r);
    assert_eq!(body["confidence"], json!("full"), "body={body}");
    let data = &body["data"];
    assert_eq!(data["count"], json!(2), "two supply rows seeded; body={body}");
    assert_eq!(
        data["principal"],
        json!("1500000"),
        "principal must be host-aggregated sum; body={body}"
    );

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn view_on_unknown_strategy_is_not_found() -> Result<()> {
    let mut proc = spawn_server().await?;
    let _ = initialize(&mut proc).await?;

    // 64 zeros — valid format, but not in the DB.
    let zero_id = "0".repeat(64);
    send(
        &mut proc,
        json!({
            "jsonrpc": "2.0", "id": 2, "method": "resources/read",
            "params": { "uri": format!("strategy://{zero_id}/view") }
        }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    assert!(r.get("error").is_some(), "expected error, got {r}");
    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn view_on_malformed_id_is_resource_not_found() -> Result<()> {
    let mut proc = spawn_server().await?;
    let _ = initialize(&mut proc).await?;

    send(
        &mut proc,
        json!({
            "jsonrpc": "2.0", "id": 2, "method": "resources/read",
            "params": { "uri": "strategy://not-a-real-id/view" }
        }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    assert!(r.get("error").is_some(), "expected error, got {r}");
    proc.child.kill().await?;
    Ok(())
}
