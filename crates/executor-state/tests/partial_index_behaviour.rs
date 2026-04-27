//! SQL-level invariants enforced by the schema (D-01c partial unique index,
//! D-03c FK pragma, D-03b idempotent migration). Uses raw SQL via the
//! `__test_conn` accessor to bypass the typed façade.

mod common;

use common::fresh_memory_store;
use executor_state::StateStore;

#[test]
fn fresh_memory_store_opens() {
    let store = fresh_memory_store();
    let row: i64 = store
        .__test_conn()
        .query_row("SELECT 1", [], |r| r.get(0))
        .expect("SELECT 1");
    assert_eq!(row, 1);
}

#[test]
fn schema_is_idempotent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("s.db");

    // First open: schema created.
    {
        let store = StateStore::open(&path).expect("first open");
        store
            .__test_conn()
            .execute(
                "INSERT INTO strategies(id, name, source, created_at) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params!["a".repeat(64), "name1", "// src", "2026-01-01T00:00:00Z"],
            )
            .expect("insert row");
    }

    // Second open: idempotent — DDL must not error and the row must persist.
    let store = StateStore::open(&path).expect("second open succeeds");
    let n: i64 = store
        .__test_conn()
        .query_row("SELECT COUNT(*) FROM strategies", [], |r| r.get(0))
        .expect("count");
    assert_eq!(n, 1);
}

#[test]
fn partial_unique_index_blocks_duplicate_active_name() {
    let store = fresh_memory_store();
    let conn = store.__test_conn();

    conn.execute(
        "INSERT INTO strategies(id, name, source, created_at) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params!["a".repeat(64), "arb", "src1", "2026-01-01T00:00:00Z"],
    )
    .expect("first insert");

    let err = conn
        .execute(
            "INSERT INTO strategies(id, name, source, created_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["b".repeat(64), "arb", "src2", "2026-01-01T00:00:01Z"],
        )
        .unwrap_err();

    let msg = err.to_string().to_uppercase();
    assert!(
        msg.contains("UNIQUE") || msg.contains("CONSTRAINT"),
        "expected UNIQUE constraint failure, got: {err}"
    );
}

#[test]
fn soft_deleted_name_can_be_reused() {
    let store = fresh_memory_store();
    let conn = store.__test_conn();

    conn.execute(
        "INSERT INTO strategies(id, name, source, created_at) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params!["a".repeat(64), "arb", "src1", "2026-01-01T00:00:00Z"],
    )
    .expect("first insert");

    conn.execute(
        "UPDATE strategies SET deleted_at = ?1 WHERE id = ?2",
        rusqlite::params!["2026-01-02T00:00:00Z", "a".repeat(64)],
    )
    .expect("soft delete");

    // Now a new active row can reuse the name.
    conn.execute(
        "INSERT INTO strategies(id, name, source, created_at) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params!["b".repeat(64), "arb", "src2", "2026-01-03T00:00:00Z"],
    )
    .expect("reuse name after soft-delete");

    // But a third active row with the same name still fails.
    let err = conn
        .execute(
            "INSERT INTO strategies(id, name, source, created_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["c".repeat(64), "arb", "src3", "2026-01-04T00:00:00Z"],
        )
        .unwrap_err();
    assert!(err.to_string().to_uppercase().contains("UNIQUE"));
}

#[test]
fn foreign_keys_enforced() {
    let store = fresh_memory_store();
    let err = store
        .__test_conn()
        .execute(
            "INSERT INTO runs(id, strategy_id, status, started_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                "01HZZZZZZZZZZZZZZZZZZZZZZZ",
                "nonexistent",
                "queued",
                "2026-01-01T00:00:00Z"
            ],
        )
        .unwrap_err();

    assert!(
        err.to_string().to_uppercase().contains("FOREIGN KEY"),
        "expected FK violation, got: {err}"
    );
}
