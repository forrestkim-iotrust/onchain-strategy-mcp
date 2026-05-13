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
-- Phase 4 (D-15d / MR-04 carry-forward): `seq` is a per-run monotonic
-- counter — same-millisecond `ctx.evm.*` calls during a loop need a
-- deterministic tie-break. Mirrors the journal_logs pattern below.
CREATE TABLE IF NOT EXISTS journal_source_reads (
    id           TEXT PRIMARY KEY,
    run_id       TEXT NOT NULL REFERENCES runs(id),
    kind         TEXT NOT NULL,
    target       TEXT NOT NULL,
    payload_json TEXT,
    recorded_at  TEXT NOT NULL,
    seq          INTEGER NOT NULL,
    UNIQUE (run_id, seq)
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

-- Phase 5 D-09: per-action policy/simulation gate verdicts. One row per
-- (action, gate) pair. `seq` is per-run monotonic (MR-04 carry-forward) so
-- list_decisions_for_run produces stable insertion order even when same-ms
-- inserts collide on `recorded_at`. `gate` ∈ {policy, simulation};
-- `verdict` ∈ {pass, fail, skipped}. `rule` and `detail` are NULL when
-- verdict=pass; `payload_json` is the serialized Decision/SimulationOutcome.
CREATE TABLE IF NOT EXISTS journal_decisions (
    id           TEXT PRIMARY KEY,
    run_id       TEXT NOT NULL REFERENCES runs(id),
    action_index INTEGER NOT NULL,
    gate         TEXT NOT NULL,
    verdict      TEXT NOT NULL,
    rule         TEXT,
    detail       TEXT,
    payload_json TEXT,
    recorded_at  TEXT NOT NULL,
    seq          INTEGER NOT NULL,
    UNIQUE (run_id, seq)
);
CREATE INDEX IF NOT EXISTS idx_journal_decisions_run_id
    ON journal_decisions(run_id);

-- Phase 6: per-action local managed execution attempts and receipt status.
CREATE TABLE IF NOT EXISTS execution_actions (
    id             TEXT PRIMARY KEY,
    run_id         TEXT NOT NULL REFERENCES runs(id),
    action_index   INTEGER NOT NULL,
    signer_address TEXT,
    tx_hash        TEXT,
    status         TEXT NOT NULL,
    receipt_status TEXT,
    gas_used       TEXT,
    error_kind     TEXT,
    error_detail   TEXT,
    recorded_at    TEXT NOT NULL,
    updated_at     TEXT NOT NULL,
    UNIQUE (run_id, action_index)
);
CREATE INDEX IF NOT EXISTS idx_execution_actions_run_id ON execution_actions(run_id);

-- v1.2 Trigger Core: triggers + trigger_events tables.
CREATE TABLE IF NOT EXISTS triggers (
    id              TEXT PRIMARY KEY,
    strategy_id     TEXT NOT NULL REFERENCES strategies(id),
    kind            TEXT NOT NULL,
    config_json     TEXT NOT NULL,
    predicate_js    TEXT,
    enabled         INTEGER NOT NULL DEFAULT 1,
    last_fired_at   TEXT,
    created_at      TEXT NOT NULL,
    dedup_window_ms INTEGER
);
CREATE INDEX IF NOT EXISTS idx_triggers_strategy_id ON triggers(strategy_id);
CREATE INDEX IF NOT EXISTS idx_triggers_enabled_kind ON triggers(enabled, kind);

CREATE TABLE IF NOT EXISTS trigger_events (
    id              TEXT PRIMARY KEY,
    trigger_id      TEXT NOT NULL REFERENCES triggers(id),
    event_json      TEXT,
    fired_at        TEXT NOT NULL,
    run_id          TEXT,
    dedup_key       TEXT,
    skipped_reason  TEXT
);
CREATE INDEX IF NOT EXISTS idx_trigger_events_trigger_id ON trigger_events(trigger_id);
CREATE INDEX IF NOT EXISTS idx_trigger_events_dedup
    ON trigger_events(trigger_id, dedup_key, fired_at);
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
