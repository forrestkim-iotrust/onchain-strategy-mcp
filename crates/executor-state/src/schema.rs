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
