//! v1.12 Track B1: last-known-good view cache contract tests.
//!
//! Asserts the upsert / get / delete trio on `strategy_view_cache` plus
//! migration idempotency. The cache is keyed by the active strategy row's
//! `id`, so each test seeds a real strategy first to satisfy the FK.

mod common;

use common::fresh_memory_store;
use executor_state::{RegisterOutcome, StateStore};

fn seed_strategy(store: &mut StateStore, name: &str) -> String {
    match store
        .register_strategy(name, &format!("// {name}"), None, None)
        .expect("register seed")
    {
        RegisterOutcome::Created(s) | RegisterOutcome::AlreadyExists(s) => s.id,
        RegisterOutcome::ReplacedVersion { created, .. } => created.id,
    }
}

/// Insert → get returns the row, with `succeeded_at` close to now (RFC3339).
#[test]
fn upsert_then_get_returns_row() {
    let mut store = fresh_memory_store();
    let id = seed_strategy(&mut store, "viewcache-a");

    let body = r#"{"balances":[{"symbol":"USDC","amount":"100.0"}]}"#;
    store
        .upsert_view_cache(&id, body)
        .expect("upsert view cache");

    let row = store
        .get_view_cache(&id)
        .expect("get view cache")
        .expect("present after upsert");
    assert_eq!(row.strategy_id, id);
    assert_eq!(row.body_json, body);

    // succeeded_at is RFC3339 UTC ("Z" suffix from to_rfc3339_opts(Secs, true)).
    assert!(
        row.succeeded_at.ends_with('Z'),
        "succeeded_at must be RFC3339 UTC with Z suffix, got {}",
        row.succeeded_at
    );
    // Should parse cleanly as RFC3339 — a parse failure means the schema
    // contract drifted from `chrono::Utc::now().to_rfc3339_opts(...)`.
    chrono::DateTime::parse_from_rfc3339(&row.succeeded_at)
        .expect("succeeded_at must parse as RFC3339");
}

/// Insert → upsert (same key, different body) → get returns the NEW body
/// AND an equal-or-newer `succeeded_at`. RFC3339 seconds granularity means
/// two upserts in the same wall second can share a timestamp; assert with
/// `>=` rather than strict `>`.
#[test]
fn upsert_overwrites_existing_row() {
    let mut store = fresh_memory_store();
    let id = seed_strategy(&mut store, "viewcache-b");

    store
        .upsert_view_cache(&id, r#"{"v":1}"#)
        .expect("first upsert");
    let first = store
        .get_view_cache(&id)
        .expect("get after first")
        .expect("present");

    store
        .upsert_view_cache(&id, r#"{"v":2}"#)
        .expect("second upsert");
    let second = store
        .get_view_cache(&id)
        .expect("get after second")
        .expect("present");

    assert_eq!(second.body_json, r#"{"v":2}"#);
    assert!(
        second.succeeded_at >= first.succeeded_at,
        "upsert must stamp a non-regressing succeeded_at; first={} second={}",
        first.succeeded_at,
        second.succeeded_at
    );

    // Still exactly one row for this strategy — upsert MUST NOT insert duplicates.
    let count: i64 = store
        .__test_conn()
        .query_row(
            "SELECT COUNT(*) FROM strategy_view_cache WHERE strategy_id = ?1",
            rusqlite::params![&id],
            |r| r.get(0),
        )
        .expect("count rows");
    assert_eq!(count, 1, "upsert must not duplicate rows for the same strategy_id");
}

/// Get on a missing / never-cached strategy_id returns `Ok(None)`.
#[test]
fn get_missing_returns_none() {
    let store = fresh_memory_store();
    let row = store
        .get_view_cache("0000000000000000000000000000000000000000000000000000000000000000")
        .expect("get missing");
    assert!(row.is_none(), "absent strategy must produce Ok(None)");
}

/// Delete → get returns `Ok(None)`. Deleting a non-existent row is a
/// no-op (idempotent) so a second delete after the first still succeeds.
#[test]
fn delete_removes_cached_row() {
    let mut store = fresh_memory_store();
    let id = seed_strategy(&mut store, "viewcache-c");

    store
        .upsert_view_cache(&id, r#"{"balances":[]}"#)
        .expect("upsert");
    assert!(store.get_view_cache(&id).unwrap().is_some());

    store.delete_view_cache(&id).expect("delete");
    assert!(
        store.get_view_cache(&id).unwrap().is_none(),
        "row must be absent after delete"
    );

    // Idempotent: second delete is a no-op, not an error.
    store
        .delete_view_cache(&id)
        .expect("delete on absent row must be a no-op");
}

/// Migration idempotency: opening `:memory:` runs `SCHEMA_SQL` once. We
/// open a second store on `:memory:` to prove a fresh `apply_migrations`
/// pass does not error. (Each `:memory:` open is a private DB, so the
/// stronger "two passes on the SAME DB don't error" guarantee is exercised
/// at the SQL level by `CREATE TABLE IF NOT EXISTS` plus the
/// `INSERT … ON CONFLICT` upsert above.)
#[test]
fn migration_is_idempotent_across_opens() {
    let _s1 = fresh_memory_store();
    let _s2 = fresh_memory_store();
    // Reaching here means both opens succeeded; the second open's
    // `SCHEMA_SQL` + `migrate()` pass would have errored if the new DDL
    // were non-idempotent.
}

/// Stronger idempotency proof: running `open_conn` against an on-disk DB
/// twice (separate process-style sequence) does not error and preserves
/// the cached row. This is the realistic upgrade path — daemon restarts
/// reopen the same file.
#[test]
fn reopening_disk_db_is_idempotent_and_preserves_cache() {
    let tmp = tempfile::NamedTempFile::new().expect("tempfile");
    let path = tmp.path().to_path_buf();

    let id = {
        let mut store = StateStore::open(&path).expect("first open");
        let id = seed_strategy(&mut store, "viewcache-disk");
        store
            .upsert_view_cache(&id, r#"{"persisted":true}"#)
            .expect("upsert");
        id
    };

    // Second open MUST replay the migration cleanly and preserve the row.
    let store2 = StateStore::open(&path).expect("second open");
    let row = store2
        .get_view_cache(&id)
        .expect("get after reopen")
        .expect("row must persist across reopen");
    assert_eq!(row.body_json, r#"{"persisted":true}"#);

    drop(store2);
}
