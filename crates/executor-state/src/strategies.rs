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
}

#[derive(Debug, Clone)]
pub struct StrategySummary {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
    pub created_at: String,
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone)]
pub enum RegisterOutcome {
    Created(Strategy),
    /// Same-source idempotent path (D-01b). Carries the **existing** row so
    /// the caller can surface `existing_name` / `existing_description` /
    /// `existing_tags` in the agent-facing response.
    AlreadyExists(Strategy),
}

pub fn hash_source(source: &str) -> String {
    let mut h = Sha256::new();
    h.update(source.as_bytes());
    hex::encode(h.finalize())
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

fn map_strategy(
    id: String,
    name: String,
    source: String,
    description: Option<String>,
    tags_raw: Option<String>,
    created_at: String,
    deleted_at: Option<String>,
) -> Strategy {
    Strategy {
        id,
        name,
        source,
        description,
        tags: decode_tags(tags_raw),
        created_at,
        deleted_at,
    }
}

pub(crate) fn register(
    conn: &Connection,
    name: &str,
    source: &str,
    description: Option<&str>,
    tags: Option<&[String]>,
) -> Result<RegisterOutcome, StateError> {
    let id = hash_source(source);

    // 1. Same id already in DB → idempotent (D-01b same-source).
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
        "INSERT INTO strategies(id, name, source, description, tags, created_at)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![&id, name, source, description, tags_json, &now],
    )?;

    Ok(RegisterOutcome::Created(Strategy {
        id,
        name: name.to_string(),
        source: source.to_string(),
        description: description.map(|s| s.to_string()),
        tags: tags.map(|t| t.to_vec()),
        created_at: now,
        deleted_at: None,
    }))
}

pub(crate) fn list(
    conn: &Connection,
    include_deleted: bool,
) -> Result<Vec<StrategySummary>, StateError> {
    // Explicit column set — `source` is intentionally absent (T-02-01-03 / D-07a).
    let sql = if include_deleted {
        "SELECT id, name, description, tags, created_at, deleted_at \
         FROM strategies ORDER BY created_at DESC"
    } else {
        "SELECT id, name, description, tags, created_at, deleted_at \
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
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub(crate) fn get_by_id(conn: &Connection, id: &str) -> Result<Option<Strategy>, StateError> {
    conn.query_row(
        "SELECT id, name, source, description, tags, created_at, deleted_at \
         FROM strategies WHERE id = ?1",
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
            ))
        },
    )
    .optional()
    .map_err(StateError::from)
}

pub(crate) fn get_by_name(conn: &Connection, name: &str) -> Result<Option<Strategy>, StateError> {
    conn.query_row(
        "SELECT id, name, source, description, tags, created_at, deleted_at \
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
