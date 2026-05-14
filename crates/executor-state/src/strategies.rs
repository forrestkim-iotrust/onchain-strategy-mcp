//! Content-addressed strategy CRUD (D-01..D-02, D-07a..c).
//!
//! - `id = hex(sha256(source))` — source-only hash, name/metadata excluded
//!   (D-01a). Same source ⇒ same id, idempotent register (D-01b).
//! - Name uniqueness is enforced **only among non-deleted rows** via the
//!   partial unique index defined in [`crate::schema`] (D-01c).
//! - All SQL parameterised — no `format!`-into-SQL anywhere (T-02-01-01).
//! - `list` projects an explicit column set so `source` is never copied
//!   into list responses (T-02-01-03 / D-07a).

use crate::error::StateError;
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
pub struct Strategy {
    pub id: String,
    pub name: String,
    pub source: String,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
    pub created_at: String,
    pub deleted_at: Option<String>,
    /// v1.4 strategy bundle: canonical JSON for the `records` schema.
    /// NULL for legacy (pre-v1.4) registrations.
    pub records_json: Option<String>,
    /// v1.4 strategy bundle: JS source for the `view` function. NULL for
    /// legacy registrations; consumers fall back to a generic balance view.
    pub view_source: Option<String>,
    /// v1.5 Track 1B: cached static extraction of contracts/selectors the
    /// strategy touches. Canonical JSON; see
    /// `crates/executor-mcp/src/contracts_touched.rs` for the shape. NULL on
    /// rows registered before v1.5.
    pub contracts_touched_json: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StrategySummary {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
    pub created_at: String,
    pub deleted_at: Option<String>,
    /// True when the row carries v1.4 records schema or view source. Used by
    /// callers to know whether `strategy://{id}/view` returns bundle output
    /// vs. generic fallback.
    pub has_bundle: bool,
}

#[derive(Debug, Clone)]
pub enum RegisterOutcome {
    Created(Strategy),
    /// Same-source idempotent path (D-01b). Carries the **existing** row so
    /// the caller can surface `existing_name` / `existing_description` /
    /// `existing_tags` in the agent-facing response.
    AlreadyExists(Strategy),
}

/// Content-addressed strategy id.
///
/// Back-compat invariant: when no bundle fields are present (legacy
/// pre-v1.4 register call), this returns the same hash as the v1.0 form
/// (`sha256(source)`). Existing strategy ids therefore stay stable across
/// the v1.4 upgrade.
///
/// When `records_json` and/or `view_source` is present, the hash mixes
/// them in with explicit length-prefixed framing so that distinct bundle
/// trios collide-free even if one component is empty.
pub fn hash_bundle(
    source: &str,
    records_json: Option<&str>,
    view_source: Option<&str>,
) -> String {
    if records_json.is_none() && view_source.is_none() {
        // Legacy path: source-only hash, byte-for-byte compatible with v1.0..v1.3.
        let mut h = Sha256::new();
        h.update(source.as_bytes());
        return hex::encode(h.finalize());
    }
    // Bundle path: length-prefixed concat of three sections.
    let mut h = Sha256::new();
    h.update(b"osmcp-bundle-v1\n");
    write_section(&mut h, "execute", source);
    write_section(&mut h, "records", records_json.unwrap_or(""));
    write_section(&mut h, "view", view_source.unwrap_or(""));
    hex::encode(h.finalize())
}

fn write_section(h: &mut Sha256, tag: &str, body: &str) {
    h.update(tag.as_bytes());
    h.update(b":");
    h.update((body.len() as u64).to_be_bytes());
    h.update(b"\n");
    h.update(body.as_bytes());
    h.update(b"\n");
}

/// Legacy alias retained so existing callers (and tests) keep compiling.
/// Same byte-for-byte output as `hash_bundle(source, None, None)`.
pub fn hash_source(source: &str) -> String {
    hash_bundle(source, None, None)
}

pub(crate) fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn encode_tags(tags: Option<&[String]>) -> Option<String> {
    tags.map(|t| serde_json::to_string(t).unwrap_or_else(|_| "[]".into()))
}

fn decode_tags(raw: Option<String>) -> Option<Vec<String>> {
    raw.and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
}

#[allow(clippy::too_many_arguments)]
fn map_strategy(
    id: String,
    name: String,
    source: String,
    description: Option<String>,
    tags_raw: Option<String>,
    created_at: String,
    deleted_at: Option<String>,
    records_json: Option<String>,
    view_source: Option<String>,
    contracts_touched_json: Option<String>,
) -> Strategy {
    Strategy {
        id,
        name,
        source,
        description,
        tags: decode_tags(tags_raw),
        created_at,
        deleted_at,
        records_json,
        view_source,
        contracts_touched_json,
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn register(
    conn: &Connection,
    name: &str,
    source: &str,
    description: Option<&str>,
    tags: Option<&[String]>,
    records_json: Option<&str>,
    view_source: Option<&str>,
    contracts_touched_json: Option<&str>,
) -> Result<RegisterOutcome, StateError> {
    // v1.5 Track 1B: `contracts_touched_json` is a DERIVATION from `source`
    // (the regex extractor in `executor-mcp::contracts_touched` computes it)
    // and is INTENTIONALLY excluded from the id hash. Re-deriving never
    // changes the id; only execute/records/view do.
    let id = hash_bundle(source, records_json, view_source);

    // 1. Same id already in DB → idempotent (D-01b same-source, extended to
    //    bundle in v1.4: same trio → same id → idempotent).
    if let Some(existing) = get_by_id(conn, &id)? {
        return Ok(RegisterOutcome::AlreadyExists(existing));
    }

    // 2. Pre-check active name collision → typed NameConflict (D-01b different-source).
    //    The race between this check and the INSERT below is closed by the
    //    Phase 2 single-`Mutex<Connection>` invariant (T-02-01-06 accepted).
    if let Some(active) = get_by_name(conn, name)? {
        return Err(StateError::NameConflict {
            attempted_name: name.to_string(),
            existing_strategy_id: active.id.clone(),
            existing_source_hash: active.id,
            existing_created_at: active.created_at,
        });
    }

    // 3. Insert.
    let now = now_rfc3339();
    let tags_json = encode_tags(tags);
    conn.execute(
        "INSERT INTO strategies(id, name, source, description, tags, created_at, records_json, view_source, contracts_touched_json)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![&id, name, source, description, tags_json, &now, records_json, view_source, contracts_touched_json],
    )?;

    Ok(RegisterOutcome::Created(Strategy {
        id,
        name: name.to_string(),
        source: source.to_string(),
        description: description.map(|s| s.to_string()),
        tags: tags.map(|t| t.to_vec()),
        created_at: now,
        deleted_at: None,
        records_json: records_json.map(|s| s.to_string()),
        view_source: view_source.map(|s| s.to_string()),
        contracts_touched_json: contracts_touched_json.map(|s| s.to_string()),
    }))
}

pub(crate) fn list(
    conn: &Connection,
    include_deleted: bool,
) -> Result<Vec<StrategySummary>, StateError> {
    // Explicit column set — `source` is intentionally absent (T-02-01-03 / D-07a).
    // `records_json` and `view_source` are pulled as boolean presence flags
    // (so list responses can advertise `has_bundle` without dragging the JS
    // source through every listing).
    let sql = if include_deleted {
        "SELECT id, name, description, tags, created_at, deleted_at, \
                records_json IS NOT NULL OR view_source IS NOT NULL \
         FROM strategies ORDER BY created_at DESC"
    } else {
        "SELECT id, name, description, tags, created_at, deleted_at, \
                records_json IS NOT NULL OR view_source IS NOT NULL \
         FROM strategies WHERE deleted_at IS NULL ORDER BY created_at DESC"
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(StrategySummary {
                id: r.get(0)?,
                name: r.get(1)?,
                description: r.get(2)?,
                tags: decode_tags(r.get::<_, Option<String>>(3)?),
                created_at: r.get(4)?,
                deleted_at: r.get(5)?,
                has_bundle: r.get::<_, i64>(6)? != 0,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub(crate) fn get_by_id(conn: &Connection, id: &str) -> Result<Option<Strategy>, StateError> {
    conn.query_row(
        "SELECT id, name, source, description, tags, created_at, deleted_at, \
                records_json, view_source, contracts_touched_json \
         FROM strategies WHERE id = ?1 LIMIT 1",
        params![id],
        |r| {
            Ok(map_strategy(
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
                r.get(9)?,
            ))
        },
    )
    .optional()
    .map_err(StateError::from)
}

pub(crate) fn get_by_name(conn: &Connection, name: &str) -> Result<Option<Strategy>, StateError> {
    conn.query_row(
        "SELECT id, name, source, description, tags, created_at, deleted_at, \
                records_json, view_source, contracts_touched_json \
         FROM strategies WHERE name = ?1 AND deleted_at IS NULL",
        params![name],
        |r| {
            Ok(map_strategy(
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
                r.get(9)?,
            ))
        },
    )
    .optional()
    .map_err(StateError::from)
}

/// Sets `deleted_at` if not yet set; otherwise returns the existing
/// `deleted_at` unchanged (D-07c idempotent semantics).
pub(crate) fn soft_delete(conn: &Connection, id: &str) -> Result<String, StateError> {
    let existing: Option<Option<String>> = conn
        .query_row(
            "SELECT deleted_at FROM strategies WHERE id = ?1",
            params![id],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?;

    match existing {
        None => Err(StateError::NotFound(format!("strategy {id}"))),
        Some(Some(ts)) => Ok(ts),
        Some(None) => {
            let now = now_rfc3339();
            conn.execute(
                "UPDATE strategies SET deleted_at = ?1 WHERE id = ?2",
                params![&now, id],
            )?;
            Ok(now)
        }
    }
}

pub(crate) fn is_deleted(conn: &Connection, id: &str) -> Result<Option<bool>, StateError> {
    let row: Option<Option<String>> = conn
        .query_row(
            "SELECT deleted_at FROM strategies WHERE id = ?1",
            params![id],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?;
    Ok(row.map(|d| d.is_some()))
}
