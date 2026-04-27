//! Schema + pragmas for the SQLite state store (D-03b/c, D-04a-c).
//!
//! - Pragmas BEFORE DDL so FK enforcement applies (Pitfall 1).
//! - `:memory:` silently rejects WAL — do NOT assert journal_mode=wal (Pitfall 3).
//! - `CREATE ... IF NOT EXISTS` ⇒ idempotent reboot (D-03b).

use crate::error::StateError;
use rusqlite::Connection;
use std::path::Path;

pub(crate) const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS strategies (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    source      TEXT NOT NULL,
    description TEXT,
    tags        TEXT,
    created_at  TEXT NOT NULL,
    deleted_at  TEXT
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_strategies_name_active
    ON strategies(name) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_strategies_deleted_at
    ON strategies(deleted_at);
CREATE TABLE IF NOT EXISTS runs (
    id           TEXT PRIMARY KEY,
    strategy_id  TEXT NOT NULL REFERENCES strategies(id),
    status       TEXT NOT NULL,
    started_at   TEXT NOT NULL,
    finished_at  TEXT,
    error        TEXT
);
CREATE INDEX IF NOT EXISTS idx_runs_strategy_id ON runs(strategy_id);

-- Phase 3 (D-06): three append-only journal tables.
CREATE TABLE IF NOT EXISTS journal_source_reads (
    id           TEXT PRIMARY KEY,
    run_id       TEXT NOT NULL REFERENCES runs(id),
    kind         TEXT NOT NULL,
    target       TEXT NOT NULL,
    payload_json TEXT,
    recorded_at  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_journal_source_reads_run_id
    ON journal_source_reads(run_id);

CREATE TABLE IF NOT EXISTS journal_actions (
    id           TEXT PRIMARY KEY,
    run_id       TEXT NOT NULL REFERENCES runs(id),
    outcome      TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    recorded_at  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_journal_actions_run_id
    ON journal_actions(run_id);

-- MR-04: `seq` is a per-run monotonic counter assigned at INSERT
-- (see `journal::record_log`). It is the primary tie-break for
-- ORDER BY (recorded_at, seq) — same-second / same-millisecond
-- log inserts are common (RFC3339 second granularity, ULID random
-- suffix is not insertion-ordered within a millisecond bucket).
-- UNIQUE (run_id, seq) makes the monotonic invariant a schema-level
-- contract: a regression in `record_log` would fail at INSERT.
CREATE TABLE IF NOT EXISTS journal_logs (
    id           TEXT PRIMARY KEY,
    run_id       TEXT NOT NULL REFERENCES runs(id),
    message      TEXT NOT NULL,
    recorded_at  TEXT NOT NULL,
    seq          INTEGER NOT NULL,
    UNIQUE (run_id, seq)
);
CREATE INDEX IF NOT EXISTS idx_journal_logs_run_id
    ON journal_logs(run_id);
"#;

pub(crate) fn open_conn(path: &Path) -> Result<Connection, StateError> {
    let conn = Connection::open(path)
        .map_err(|e| StateError::Storage(format!("open {}: {e}", path.display())))?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;\n\
         PRAGMA synchronous = NORMAL;\n\
         PRAGMA foreign_keys = ON;",
    )?;
    conn.execute_batch(SCHEMA_SQL)?;
    Ok(conn)
}
