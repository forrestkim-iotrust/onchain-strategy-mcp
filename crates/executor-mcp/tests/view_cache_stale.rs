//! v1.12 Track B2 — `strategy://{id}/view` stale envelope on view failure.
//!
//! Three behaviours under test:
//!
//! 1. **`cache_populated_on_successful_view`** — a clean view evaluation
//!    writes a row into `strategy_view_cache`. A second read continues to
//!    return `confidence: "full"` (the cache is a *fallback*, not a source).
//! 2. **`stale_served_when_view_fails_with_prior_cache`** — pragmatic test:
//!    we directly seed the cache table with a known body, then register a
//!    strategy whose view throws. The read MUST return the cached `data`
//!    wrapped with `confidence: "stale"` + a `staleness` block carrying the
//!    cached `succeeded_at` and the current failure reason.
//! 3. **`partial_envelope_when_view_fails_with_no_cache`** — sanity that
//!    the v1.4 contract still holds: no cache + failing view ⇒ `partial`,
//!    `staleness` field absent.
//!
//! See `crates/executor-state/src/view_cache.rs` for the cache shape, and
//! `crates/executor-mcp/src/resources.rs::read_strategy_view` for the
//! serve-side branch logic.

mod common;

use anyhow::Result;
use serde_json::{Value, json};

use common::{initialize, read_resource, spawn_server_with_state};

/// Pull the JSON body out of a `resources/read` response or panic.
fn read_resource_body(r: &Value) -> Value {
    let text = r["result"]["contents"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("resources/read missing contents[0].text: {r}"));
    serde_json::from_str(text)
        .unwrap_or_else(|e| panic!("resource body is not JSON: {e} — text={text}"))
}

#[tokio::test]
async fn cache_populated_on_successful_view() -> Result<()> {
    let tmp = tempfile::NamedTempFile::new()?;
    let db_path = tmp.path().to_path_buf();
    let _ = tmp.into_temp_path().keep()?;

    // Seed a bundle strategy whose view trivially succeeds — no records, no
    // RPC, deterministic output.
    let sid = {
        use executor_state::{RegisterOutcome, StateStore};
        let mut store = StateStore::open(&db_path)?;
        let outcome = store.register_strategy_bundle(
            "cache-happy",
            "(ctx) => 'noop'",
            None,
            None,
            None,
            Some("(ctx, records) => ({ ok: true, hello: 'world' })"),
            None,
            None,
        )?;
        match outcome {
            RegisterOutcome::Created(s) | RegisterOutcome::AlreadyExists(s) => s.id,
            RegisterOutcome::ReplacedVersion { created, .. } => created.id,
        }
    };

    let db_str = db_path.to_string_lossy().to_string();
    let mut proc = spawn_server_with_state(&db_str).await?;
    let _ = initialize(&mut proc).await?;

    // First read — view runs, body wraps with confidence:"full", cache row gets written.
    let r = read_resource(&mut proc, 2, &format!("strategy://{sid}/view")).await?;
    assert!(r.get("error").is_none(), "first read errored: {r}");
    let body = read_resource_body(&r);
    assert_eq!(body["confidence"], json!("full"), "first read body: {body}");
    assert_eq!(body["data"]["ok"], json!(true));
    // No staleness on a fresh success.
    assert!(
        body.get("staleness").is_none(),
        "fresh success must not carry staleness; body={body}"
    );

    proc.child.kill().await?;

    // Confirm the cache row exists out-of-band.
    {
        use executor_state::StateStore;
        let store = StateStore::open(&db_path)?;
        let row = store
            .get_view_cache(&sid)?
            .expect("cache row must exist after a successful view read");
        assert_eq!(row.strategy_id, sid);
        // body_json must round-trip as JSON with confidence:"full".
        let parsed: Value = serde_json::from_str(&row.body_json)?;
        assert_eq!(parsed["confidence"], json!("full"));
        assert_eq!(parsed["data"]["hello"], json!("world"));
        // succeeded_at parses as RFC3339.
        assert!(
            chrono::DateTime::parse_from_rfc3339(&row.succeeded_at).is_ok(),
            "succeeded_at not RFC3339: {}",
            row.succeeded_at
        );
    }

    // Second read still returns confidence:"full" — the cache is a fallback,
    // not a short-circuit. Spawn a fresh proc to avoid the JS sandbox keeping
    // any in-process state warm.
    let mut proc = spawn_server_with_state(&db_str).await?;
    let _ = initialize(&mut proc).await?;
    let r = read_resource(&mut proc, 2, &format!("strategy://{sid}/view")).await?;
    let body = read_resource_body(&r);
    assert_eq!(body["confidence"], json!("full"), "second read body: {body}");
    proc.child.kill().await?;

    Ok(())
}

#[tokio::test]
async fn stale_served_when_view_fails_with_prior_cache() -> Result<()> {
    let tmp = tempfile::NamedTempFile::new()?;
    let db_path = tmp.path().to_path_buf();
    let _ = tmp.into_temp_path().keep()?;

    // Register a bundle whose view ALWAYS throws — but pre-seed the cache
    // with a known-good body so the failure path falls back to "stale".
    let sid = {
        use executor_state::{RegisterOutcome, StateStore};
        let mut store = StateStore::open(&db_path)?;
        let outcome = store.register_strategy_bundle(
            "cache-stale",
            "(ctx) => 'noop'",
            None,
            None,
            None,
            // View source that explicitly throws — RuntimeError surfaces.
            Some("(ctx, records) => { throw new Error('boom'); }"),
            None,
            None,
        )?;
        let sid = match outcome {
            RegisterOutcome::Created(s) | RegisterOutcome::AlreadyExists(s) => s.id,
            RegisterOutcome::ReplacedVersion { created, .. } => created.id,
        };

        // Pre-seed the cache directly. The body shape mirrors what the
        // success branch would have written: full wrapped envelope with
        // confidence:"full" + data + logs.
        let cached_body = serde_json::to_string(&json!({
            "data": { "balance": "42", "asset": "USDC" },
            "confidence": "full",
            "logs": [],
        }))?;
        store.upsert_view_cache(&sid, &cached_body)?;
        sid
    };

    let db_str = db_path.to_string_lossy().to_string();
    let mut proc = spawn_server_with_state(&db_str).await?;
    let _ = initialize(&mut proc).await?;

    let r = read_resource(&mut proc, 2, &format!("strategy://{sid}/view")).await?;
    assert!(r.get("error").is_none(), "expected success, got {r}");
    let body = read_resource_body(&r);

    // Stale envelope assertions:
    assert_eq!(
        body["confidence"],
        json!("stale"),
        "view failure + prior cache must surface stale; body={body}"
    );
    // `data` reused verbatim from the cache.
    assert_eq!(body["data"]["balance"], json!("42"));
    assert_eq!(body["data"]["asset"], json!("USDC"));

    // `staleness` block is REQUIRED on stale, with all three fields populated.
    let staleness = body
        .get("staleness")
        .unwrap_or_else(|| panic!("stale body must carry staleness; body={body}"));
    assert!(staleness["succeeded_at"].is_string());
    assert!(
        chrono::DateTime::parse_from_rfc3339(staleness["succeeded_at"].as_str().unwrap()).is_ok(),
        "staleness.succeeded_at not RFC3339: {staleness}"
    );
    // Cache was just written — age must be small.
    let age = staleness["age_seconds"]
        .as_u64()
        .unwrap_or_else(|| panic!("staleness.age_seconds not a u64: {staleness}"));
    assert!(age < 10, "stale age should be < 10s for a fresh seed; got {age}");
    // `current_error` carries the view-failure reason. We don't assert on
    // the specific JS error message (the sandbox may surface OOM / type
    // errors / runtime aborts depending on host configuration); only that
    // the prefix marking it as a view-eval failure is present.
    let err = staleness["current_error"]
        .as_str()
        .unwrap_or_else(|| panic!("staleness.current_error must be string: {staleness}"));
    assert!(
        err.contains("view function failed"),
        "current_error should reference view failure; got {err}"
    );
    assert!(!err.is_empty(), "current_error must be non-empty");

    // `reason` + `remediation` must still be present for human readability.
    assert!(body["reason"].is_string(), "stale must carry reason: {body}");
    assert!(body["remediation"].is_string(), "stale must carry remediation: {body}");

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn partial_envelope_when_view_fails_with_no_cache() -> Result<()> {
    let tmp = tempfile::NamedTempFile::new()?;
    let db_path = tmp.path().to_path_buf();
    let _ = tmp.into_temp_path().keep()?;

    // Bundle whose view always throws, with NO cache row seeded — exercises
    // the "we have nothing to show" path. This is the v1.4 contract that
    // must keep working.
    let sid = {
        use executor_state::{RegisterOutcome, StateStore};
        let mut store = StateStore::open(&db_path)?;
        let outcome = store.register_strategy_bundle(
            "no-cache-partial",
            "(ctx) => 'noop'",
            None,
            None,
            None,
            Some("(ctx, records) => { throw new Error('still broken'); }"),
            None,
            None,
        )?;
        match outcome {
            RegisterOutcome::Created(s) | RegisterOutcome::AlreadyExists(s) => s.id,
            RegisterOutcome::ReplacedVersion { created, .. } => created.id,
        }
    };

    // Sanity: cache truly empty before the read.
    {
        use executor_state::StateStore;
        let store = StateStore::open(&db_path)?;
        assert!(
            store.get_view_cache(&sid)?.is_none(),
            "test precondition: cache must be empty"
        );
    }

    let db_str = db_path.to_string_lossy().to_string();
    let mut proc = spawn_server_with_state(&db_str).await?;
    let _ = initialize(&mut proc).await?;

    let r = read_resource(&mut proc, 2, &format!("strategy://{sid}/view")).await?;
    assert!(r.get("error").is_none(), "expected success, got {r}");
    let body = read_resource_body(&r);

    assert_eq!(
        body["confidence"],
        json!("partial"),
        "view failure + no cache must stay on `partial`; body={body}"
    );
    assert_eq!(body["data"], Value::Null);
    assert!(
        body.get("staleness").is_none(),
        "partial must NOT carry staleness; body={body}"
    );
    let reason = body["reason"]
        .as_str()
        .unwrap_or_else(|| panic!("partial must carry reason; body={body}"));
    assert!(
        reason.contains("view function failed"),
        "reason should be prefixed with view failure marker; got {reason}"
    );

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn delete_view_cache_clears_row() -> Result<()> {
    // v1.12 follow-on: `strategy_delete` drops the cache row alongside the
    // soft-delete so the row doesn't outlive the strategy. Direct façade
    // exercise — the MCP tool path just calls `delete_view_cache` then
    // `soft_delete_strategy` so this is the load-bearing invariant.
    let tmp = tempfile::NamedTempFile::new()?;
    let db_path = tmp.path().to_path_buf();
    let _ = tmp.into_temp_path().keep()?;

    use executor_state::{RegisterOutcome, StateStore};
    let mut store = StateStore::open(&db_path)?;
    let sid = match store.register_strategy_bundle(
        "delete-clears",
        "(ctx) => 'noop'",
        None,
        None,
        None,
        Some("(ctx, records) => ({})"),
        None,
        None,
    )? {
        RegisterOutcome::Created(s) | RegisterOutcome::AlreadyExists(s) => s.id,
        RegisterOutcome::ReplacedVersion { created, .. } => created.id,
    };

    store.upsert_view_cache(&sid, r#"{"data":{},"confidence":"full","logs":[]}"#)?;
    assert!(store.get_view_cache(&sid)?.is_some(), "seed precondition");

    // Idempotent: first delete returns true, second returns false.
    assert!(store.delete_view_cache(&sid)?, "first delete should remove the row");
    assert!(
        !store.delete_view_cache(&sid)?,
        "second delete should be a no-op (idempotent)"
    );
    assert!(store.get_view_cache(&sid)?.is_none(), "row gone after delete");

    Ok(())
}
