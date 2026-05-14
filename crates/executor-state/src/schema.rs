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
    id           TEXT PRIMARY KEY,
    name         TEXT NOT NULL,
    source       TEXT NOT NULL,
    description  TEXT,
    tags         TEXT,
    created_at   TEXT NOT NULL,
    deleted_at   TEXT,
    -- v1.4 strategy bundle: optional records schema + view function.
    -- NULL on rows registered before v1.4; those strategies fall back to
    -- the generic balance-only view.
    records_json TEXT,
    view_source  TEXT,
    -- v1.5 Track 1B: cached static extraction of contracts and selectors the
    -- strategy source touches, computed regex-style at register time. Stored
    -- as canonical JSON shaped like
    --   { "0xCONTRACT": ["selector1", "selector2"],
    --     "_extraction": "complete" | "incomplete",
    --     "_warnings": ["..."] }
    -- NULL on rows registered before v1.5; alignment treats those as
    -- `_extraction: incomplete`.
    contracts_touched_json TEXT,
    -- v1.8 name-anchored lineage: stable identifier preserved across
    -- re-registrations of the SAME name even when content changes. Minted at
    -- first register, copied forward on every version bump within a lineage.
    -- See `strategies.rs` for the register-flow case matrix and how lineage_id
    -- folds into the id hash for fresh lineages (legacy rows are backfilled
    -- with `lineage_id = id` so their pre-v1.8 ids stay byte-identical).
    lineage_id   TEXT
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_strategies_name_active
    ON strategies(name) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_strategies_deleted_at
    ON strategies(deleted_at);
-- NOTE: v1.8 lineage_id indexes live in `migrate()` (NOT here) so the
-- SCHEMA_SQL pass succeeds against a pre-v1.8 strategies table that lacks
-- the `lineage_id` column. migrate() ADDs the column then creates the
-- indexes once the column exists.
CREATE TABLE IF NOT EXISTS runs (
    id           TEXT PRIMARY KEY,
    strategy_id  TEXT NOT NULL REFERENCES strategies(id),
    status       TEXT NOT NULL,
    started_at   TEXT NOT NULL,
    finished_at  TEXT,
    error        TEXT,
    -- v1.8 name-anchored lineage: lineage attached to this run. Survives
    -- view/records re-registrations of the same strategy name so historical
    -- runs aggregate into the lineage's portfolio view. `strategy_id` is
    -- still the EXACT version that executed for forensics.
    strategy_lineage_id TEXT
);
CREATE INDEX IF NOT EXISTS idx_runs_strategy_id ON runs(strategy_id);
-- v1.8 strategy_lineage_id index is created in `migrate()` so pre-v1.8
-- runs tables (without the column) survive the SCHEMA_SQL pass.

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
    dedup_window_ms INTEGER,
    -- v1.8 name-anchored lineage: triggers attach to a LINEAGE rather than
    -- a specific strategy version. Dispatcher resolves lineage_id → latest
    -- active version at fire time so view/records re-registrations don't
    -- orphan the trigger.
    strategy_lineage_id TEXT
);
CREATE INDEX IF NOT EXISTS idx_triggers_strategy_id ON triggers(strategy_id);
-- v1.8 strategy_lineage_id index → `migrate()`.
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

-- v1.4 strategy bundle: per-action records captured at confirm time.
-- One row per (run, strategy_id, record_name) capture event. payload_json
-- holds the evaluated capture map (per the strategy's records[].capture spec).
CREATE TABLE IF NOT EXISTS strategy_records_capture (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id        TEXT NOT NULL REFERENCES runs(id),
    strategy_id   TEXT NOT NULL REFERENCES strategies(id),
    record_name   TEXT NOT NULL,
    captured_at   TEXT NOT NULL,
    payload_json  TEXT NOT NULL,
    -- v1.8 name-anchored lineage: captured records attach to a LINEAGE so
    -- view/records-spec tweaks of the same strategy name preserve history.
    -- `strategy_id` is still the version that produced the capture.
    strategy_lineage_id TEXT
);
CREATE INDEX IF NOT EXISTS idx_records_capture_strategy
    ON strategy_records_capture(strategy_id, captured_at);
CREATE INDEX IF NOT EXISTS idx_records_capture_run
    ON strategy_records_capture(run_id);
-- v1.8 strategy_lineage_id index → `migrate()`.

-- v1.5 Track 1A: policy revisions table. Policy migrates from
-- `.local/policy.toml` to DB; `policy_set` is the only edit path. One row
-- holds the active revision; older rows are preserved for history. The
-- partial unique index enforces the "exactly one active" invariant at the
-- schema level so a regression in `policy_revisions::set_active` would fail
-- at INSERT rather than silently leaving two active rows behind.
CREATE TABLE IF NOT EXISTS policies (
    revision_id   TEXT PRIMARY KEY,
    body_json     TEXT NOT NULL,
    rationale     TEXT,
    set_at        TEXT NOT NULL,
    is_active     INTEGER NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_policies_one_active
    ON policies(is_active) WHERE is_active = 1;
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
    migrate(&conn)?;
    Ok(conn)
}

/// Additive migrations for pre-existing DBs created on older binaries.
/// Idempotent: each step checks current state before applying. The fresh-DB
/// path runs `SCHEMA_SQL` first which already has the v1.4 columns, so these
/// ALTERs only fire on upgrades from v1.3.x.
fn migrate(conn: &Connection) -> Result<(), StateError> {
    if !has_column(conn, "strategies", "records_json")? {
        conn.execute_batch("ALTER TABLE strategies ADD COLUMN records_json TEXT;")?;
    }
    if !has_column(conn, "strategies", "view_source")? {
        conn.execute_batch("ALTER TABLE strategies ADD COLUMN view_source TEXT;")?;
    }
    if !has_column(conn, "strategies", "contracts_touched_json")? {
        conn.execute_batch("ALTER TABLE strategies ADD COLUMN contracts_touched_json TEXT;")?;
    }
    // v1.6.x: free-form natural-language description on each trigger so the
    // operator can recognise "what does this trigger DO" without decoding the
    // address/topic blob. Not part of the content hash — purely descriptive.
    if !has_column(conn, "triggers", "note")? {
        conn.execute_batch("ALTER TABLE triggers ADD COLUMN note TEXT;")?;
    }

    // v1.8 name-anchored lineage: add lineage_id columns + backfill.
    // Idempotent: each column add is gated on `has_column`, and the
    // backfill updates only rows where lineage_id IS NULL.
    if !has_column(conn, "strategies", "lineage_id")? {
        conn.execute_batch("ALTER TABLE strategies ADD COLUMN lineage_id TEXT;")?;
    }
    // Backfill: legacy rows get lineage_id = id so their pre-v1.8 hash
    // semantics stay byte-stable (`hash_bundle_with_lineage(id, ...)` is
    // never invoked for these — the existing `id = hash(execute|records|view)`
    // is preserved as both the row's id AND lineage anchor).
    conn.execute_batch(
        "UPDATE strategies SET lineage_id = id WHERE lineage_id IS NULL;",
    )?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_strategies_lineage_id \
              ON strategies(lineage_id);",
    )?;
    conn.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_strategies_lineage_active \
              ON strategies(lineage_id) WHERE deleted_at IS NULL;",
    )?;

    if !has_column(conn, "triggers", "strategy_lineage_id")? {
        conn.execute_batch(
            "ALTER TABLE triggers ADD COLUMN strategy_lineage_id TEXT;",
        )?;
    }
    conn.execute_batch(
        "UPDATE triggers \
         SET strategy_lineage_id = ( \
           SELECT lineage_id FROM strategies WHERE strategies.id = triggers.strategy_id \
         ) \
         WHERE strategy_lineage_id IS NULL;",
    )?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_triggers_strategy_lineage_id \
              ON triggers(strategy_lineage_id);",
    )?;

    if !has_column(conn, "runs", "strategy_lineage_id")? {
        conn.execute_batch(
            "ALTER TABLE runs ADD COLUMN strategy_lineage_id TEXT;",
        )?;
    }
    conn.execute_batch(
        "UPDATE runs \
         SET strategy_lineage_id = ( \
           SELECT lineage_id FROM strategies WHERE strategies.id = runs.strategy_id \
         ) \
         WHERE strategy_lineage_id IS NULL;",
    )?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_runs_strategy_lineage_id \
              ON runs(strategy_lineage_id);",
    )?;

    if !has_column(
        conn,
        "strategy_records_capture",
        "strategy_lineage_id",
    )? {
        conn.execute_batch(
            "ALTER TABLE strategy_records_capture \
                ADD COLUMN strategy_lineage_id TEXT;",
        )?;
    }
    conn.execute_batch(
        "UPDATE strategy_records_capture \
         SET strategy_lineage_id = ( \
           SELECT lineage_id FROM strategies \
            WHERE strategies.id = strategy_records_capture.strategy_id \
         ) \
         WHERE strategy_lineage_id IS NULL;",
    )?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_records_capture_lineage \
              ON strategy_records_capture(strategy_lineage_id, captured_at);",
    )?;

    Ok(())
}

fn has_column(conn: &Connection, table: &str, column: &str) -> Result<bool, StateError> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({})", table))?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
}
