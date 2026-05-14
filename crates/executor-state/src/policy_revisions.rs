//! v1.5 Track 1A — policy revision history in SQLite.
//!
//! Backs the `policy_set` MCP tool and the `policy://current` /
//! `policy://history` resources. The schema (in [`crate::schema`]) enforces
//! "exactly one active row" via a partial unique index on `is_active = 1`;
//! [`set_active`] additionally wraps the deactivate-old + insert-new pair in
//! a transaction so an interrupted call cannot leave the table in a state
//! with two active rows OR zero active rows after a successful prior write.
//!
//! Revision ids are fresh ULIDs (monotonic within a millisecond). `set_at`
//! is RFC3339 (UTC, milliseconds — see [`now_rfc3339_millis`]).

use crate::error::StateError;
use rusqlite::{Connection, OptionalExtension, params};

/// Full revision row — `body_json` is the canonical serialized policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyRevision {
    pub revision_id: String,
    pub body_json: String,
    pub rationale: Option<String>,
    pub set_at: String,
}

/// Summary row for `policy://history` — omits the (potentially large)
/// `body_json` blob to keep listings cheap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyRevisionSummary {
    pub revision_id: String,
    pub rationale: Option<String>,
    pub set_at: String,
    pub is_active: bool,
}

fn now_rfc3339_millis() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Atomic deactivate-old + insert-new. The partial unique index
/// `idx_policies_one_active` is the schema-level guard; the explicit
/// transaction here is the application-level guard. A regression in either
/// would surface as a constraint failure at INSERT — never as a silently
/// corrupted "two-active" or "zero-active" row count.
pub(crate) fn set_active(
    conn: &mut Connection,
    body_json: &str,
    rationale: Option<&str>,
) -> Result<PolicyRevision, StateError> {
    let tx = conn.transaction()?;
    tx.execute(
        "UPDATE policies SET is_active = 0 WHERE is_active = 1",
        params![],
    )?;
    let revision_id = ulid::Ulid::new().to_string();
    let set_at = now_rfc3339_millis();
    tx.execute(
        "INSERT INTO policies(revision_id, body_json, rationale, set_at, is_active) \
         VALUES (?1, ?2, ?3, ?4, 1)",
        params![&revision_id, body_json, rationale, &set_at],
    )?;
    tx.commit()?;
    Ok(PolicyRevision {
        revision_id,
        body_json: body_json.to_string(),
        rationale: rationale.map(|s| s.to_string()),
        set_at,
    })
}

/// Returns the single active revision, if any. Boot uses this to decide
/// whether the one-shot `.local/policy.toml` import path should fire.
pub(crate) fn get_active(conn: &Connection) -> Result<Option<PolicyRevision>, StateError> {
    conn.query_row(
        "SELECT revision_id, body_json, rationale, set_at \
         FROM policies WHERE is_active = 1 LIMIT 1",
        params![],
        |r| {
            Ok(PolicyRevision {
                revision_id: r.get(0)?,
                body_json: r.get(1)?,
                rationale: r.get(2)?,
                set_at: r.get(3)?,
            })
        },
    )
    .optional()
    .map_err(StateError::from)
}

/// Newest-first revision summary listing. `limit` is hard-capped at 200 to
/// keep `policy://history` payloads bounded.
pub(crate) fn list_revisions(
    conn: &Connection,
    limit: u64,
) -> Result<Vec<PolicyRevisionSummary>, StateError> {
    let capped = limit.min(200) as i64;
    let mut stmt = conn.prepare(
        "SELECT revision_id, rationale, set_at, is_active \
         FROM policies \
         ORDER BY set_at DESC, revision_id DESC \
         LIMIT ?1",
    )?;
    let rows = stmt
        .query_map(params![capped], |r| {
            Ok(PolicyRevisionSummary {
                revision_id: r.get(0)?,
                rationale: r.get(1)?,
                set_at: r.get(2)?,
                is_active: r.get::<_, i64>(3)? != 0,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Count rows for invariant assertions (test-only). Production code paths
/// shouldn't need this; it exists so the transactional invariant test in
/// `tests/policy_revisions.rs` can assert exactly one active row.
#[doc(hidden)]
pub fn __test_count_active(conn: &Connection) -> Result<i64, StateError> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM policies WHERE is_active = 1",
        params![],
        |r| r.get(0),
    )?;
    Ok(n)
}
