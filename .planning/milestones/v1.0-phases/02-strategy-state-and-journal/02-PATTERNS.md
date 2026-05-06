# Phase 2: Strategy State and Journal - Pattern Map

**Mapped:** 2026-04-24
**Files analyzed:** 23 (new + modified + response-schema goldens)
**Analogs found:** 20 / 23 (3 genuinely greenfield — flagged in "No Analog" section)

---

## File Classification

### Files to CREATE

| Path | Role | Data Flow | Closest Analog | Match Quality |
|------|------|-----------|----------------|---------------|
| `crates/executor-state/src/error.rs` | error enum (library) | — | `crates/executor-mcp/src/errors.rs` (variant surface) + `executor-core/src/error.rs` (thiserror shape) | role-match |
| `crates/executor-state/src/schema.rs` | storage-init (DDL + pragmas) | batch (one-shot) | *(greenfield)* | none — RESEARCH Example §Pattern 1 |
| `crates/executor-state/src/store.rs` | repository owner (`StateStore::open`) | request-response (sync) | `crates/executor-mcp/src/server.rs` (struct + constructor pattern) | role-match |
| `crates/executor-state/src/strategies.rs` | repository (CRUD + content-address) | CRUD sync | *(greenfield)* | none — RESEARCH Example §Pattern 3 |
| `crates/executor-state/src/runs.rs` | repository (base CRUD) | CRUD sync | `.../strategies.rs` (sibling — written first) | role-match (self-sibling) |
| `crates/executor-state/tests/strategy_roundtrip.rs` | integration test (library) | request-response | `crates/executor-core/tests/schema_snapshots.rs` (crate-scoped integration test shape) | role-match |
| `crates/executor-state/tests/partial_index_behaviour.rs` | integration test (SQL behavior) | batch | same as above | role-match |
| `crates/executor-state/tests/run_base_model.rs` | integration test (roundtrip) | CRUD sync | same as above | role-match |
| `crates/executor-core/tests/schemas/StrategyGetInput.json` | schema golden | — | `.../schemas/StrategyRegisterInput.json` | exact |
| `crates/executor-core/tests/schemas/RunStatus.json` | schema golden | — | same as above | exact |
| `crates/executor-core/tests/schemas/StrategyRegisterResponse.json` | schema golden | — | same as above | exact |
| `crates/executor-core/tests/schemas/StrategyListResponse.json` | schema golden | — | same as above | exact |
| `crates/executor-core/tests/schemas/StrategyGetResponse.json` | schema golden | — | same as above | exact |
| `crates/executor-core/tests/schemas/StrategyDeleteResponse.json` | schema golden | — | same as above | exact |
| `crates/executor-core/tests/schemas/ExecutionGetResponse.json` | schema golden | — | same as above | exact |

### Files to MODIFY

| Path | Role | Data Flow | Closest Analog | Match Quality |
|------|------|-----------|----------------|---------------|
| `crates/executor-state/Cargo.toml` | crate manifest | — | `crates/executor-mcp/Cargo.toml` (workspace-deps style) | exact |
| `crates/executor-state/src/lib.rs` | crate root / re-exports | — | `crates/executor-mcp/src/lib.rs` | exact |
| `crates/executor-core/src/schema/strategy.rs` | schema module (XOR enum + response types) | — | *(self — extend existing file)* | self |
| `crates/executor-core/src/schema/execution.rs` | schema module (response + RunStatus) | — | *(self — extend existing file)* | self |
| `crates/executor-core/tests/schema_snapshots.rs` | test harness | — | *(self — add 7 new `#[test]` fns)* | self |
| `crates/executor-core/tests/schemas/StrategyRegisterInput.json` | schema golden (regenerate) | — | *(self — field split description/tags)* | self |
| `crates/executor-mcp/Cargo.toml` | crate manifest | — | *(self — already depends on executor-state)* | self |
| `crates/executor-mcp/src/config.rs` | config loader | request-response | *(self — Phase 1 `[logging]` pattern)* | exact |
| `crates/executor-mcp/src/errors.rs` | error mapping | — | *(self — extend `unimplemented_err` taxonomy)* | exact |
| `crates/executor-mcp/src/server.rs` | server struct + handler | — | *(self — add `state` field + `.new(cfg)` arg)* | self |
| `crates/executor-mcp/src/tools.rs` | tool dispatch | request-response (async) | *(self — replace 5 placeholder bodies)* | self |
| `crates/executor-mcp/src/resources.rs` | resource read dispatch | request-response (async) | *(self — rewrite `read_resource_impl` branch)* | self |
| `crates/executor-mcp/src/main.rs` | binary entrypoint | — | *(self — pass `cfg.state` to `ExecutorServer::new`)* | self |
| `crates/executor-mcp/tests/stdio_handshake.rs` | integration tests (stdio) | request-response | *(self — add 12 new `#[tokio::test]` fns)* | self |
| `crates/executor-mcp/tests/common/mod.rs` | test helpers | — | *(self — add `seed_strategies` / tempdir spawner)* | self |
| `config.example.toml` | config sample | — | *(self — append `[state]` section)* | self |

---

## Pattern Assignments

### 1. `crates/executor-state/Cargo.toml` (crate manifest)

**Analog:** `crates/executor-mcp/Cargo.toml` (workspace-dep style) + RESEARCH §"Standard Stack → Installation".

**Existing workspace pattern** (`crates/executor-mcp/Cargo.toml:1-14`):
```toml
[package]
name = "executor-mcp"
version.workspace = true
edition.workspace = true
license.workspace = true

[lints]
workspace = true

[dependencies]
rmcp.workspace = true
schemars.workspace = true
# ...
executor-core = { path = "../executor-core" }
```

**What to copy:** `version.workspace = true` / `edition.workspace = true` / `license.workspace = true` / `[lints] workspace = true` + workspace-dep syntax for `serde`/`serde_json`/`thiserror`. New-deps (`rusqlite`, `sha2`, `hex`, `ulid`, `chrono`, `tempfile`) are pinned literally since they are NOT in workspace deps today — planner must decide whether to promote to workspace (cross-phase consistency) or pin per-crate (isolation).

---

### 2. `crates/executor-state/src/lib.rs` (crate root)

**Analog:** `crates/executor-mcp/src/lib.rs:1-25` (current file).

**Imports/module pattern** (lines 1-24):
```rust
#![deny(clippy::print_stdout, clippy::print_stderr, clippy::dbg_macro)]
//! `executor-mcp` — stdio MCP server.
//!
//! - `config` loads `config.toml` (Phase 1: `logging.level` only).
//! - `logging` sets up a stderr-only tracing subscriber ...
//! [...]

pub mod config;
pub mod errors;
pub mod logging;
pub mod prompts;
pub mod resources;
pub mod server;
pub mod tools;

pub use server::ExecutorServer;
```

**What to copy:**
- Top-level `#![deny(...)]` for stdout/stderr/dbg (workspace lint tripwire is already there; belt-and-suspenders is the Phase 1 pattern).
- Module-docstring with per-module one-liner.
- `pub mod ...` per file + `pub use` re-export of the main public type (`StateStore`).

**Current placeholder to replace** (`crates/executor-state/src/lib.rs:1-2`):
```rust
#![deny(clippy::print_stdout, clippy::print_stderr, clippy::dbg_macro)]
//! Strategy/execution/journal persistence boundary — Phase 2에서 SQLite 구현.
```

---

### 3. `crates/executor-state/src/error.rs` (NEW — thiserror enum)

**Analog:** RESEARCH §"Code Examples → Example 2" (lines 704-722). No in-repo `thiserror` enum exists yet (Phase 1 crates use `anyhow` in the binary and nothing in libraries).

**Core pattern** (RESEARCH Example 2):
```rust
#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("storage error: {0}")]
    Storage(String),

    #[error("strategy not found: {0}")]
    NotFound(String),

    #[error("strategy name conflict: {attempted_name}")]
    NameConflict {
        attempted_name: String,
        existing_strategy_id: String,
        existing_source_hash: String,
        existing_created_at: String,
    },

    #[error("input validation failed: {0}")]
    InvalidInput(String),
}

impl From<rusqlite::Error> for StateError {
    fn from(e: rusqlite::Error) -> Self { StateError::Storage(e.to_string()) }
}
```

**What to copy:** four-variant enum verbatim; `From<rusqlite::Error>` impl for `?` ergonomics. Message text should match the conflict message in CONTEXT specifics §"name conflict error message" (`"strategy name '{}' already used by strategy_id={} (created {}); soft-delete that strategy to reuse the name, or choose a different name"`).

---

### 4. `crates/executor-state/src/schema.rs` (NEW — DDL + pragmas)

**Analog:** None in repo. RESEARCH §"Pattern 1: Open + Pragma + Idempotent Migration" is the canonical reference (§RESEARCH line 339-392).

**Core pattern** (RESEARCH Pattern 1):
```rust
pub const SCHEMA_SQL: &str = include_str!("schema.sql");

pub fn open_store(path: &Path) -> Result<Connection, StateError> {
    let conn = Connection::open(path)
        .map_err(|e| StateError::Storage(format!("open {}: {e}", path.display())))?;

    // Order matters: pragmas BEFORE any DDL.
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;",
    )?;

    conn.execute_batch(SCHEMA_SQL)?;
    Ok(conn)
}
```

**DDL to include** (inline or via `include_str!("schema.sql")` — planner decides):
```sql
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
```

**Pitfall call-outs** (from RESEARCH §Pitfalls 1, 2, 3):
- `PRAGMA foreign_keys = ON` MUST be set per-connection — bundled defaults ON but system SQLite defaults OFF.
- `:memory:` silently rejects WAL — do NOT assert `journal_mode == 'wal'`. Log via `tracing` and move on.
- Partial unique index with `WHERE deleted_at IS NULL` allows many NULL-name deleted rows to coexist.

---

### 5. `crates/executor-state/src/store.rs` (NEW — `StateStore` owner)

**Analog:** `crates/executor-mcp/src/server.rs:26-47` (struct definition + `new()` constructor pattern).

**Struct pattern to copy** (server.rs:26-47):
```rust
#[derive(Clone)]
pub struct ExecutorServer {
    pub(crate) tool_router: ToolRouter<Self>,
    pub(crate) prompt_router: PromptRouter<Self>,
}

impl ExecutorServer {
    pub fn new() -> Self {
        Self { tool_router: Self::tool_router(), prompt_router: Self::prompt_router() }
    }
}
```

**Adaptation for `StateStore`** (signature from CONTEXT D-06 + RESEARCH Pattern 1 + Pattern 2):
```rust
pub struct StateStore {
    conn: Mutex<rusqlite::Connection>,  // std::sync::Mutex inside the store; Arc<tokio::sync::Mutex<StateStore>> lives in ExecutorServer (see Pattern 6 below).
}

impl StateStore {
    pub fn open(path: &std::path::Path) -> Result<Self, StateError> {
        let conn = crate::schema::open_store(path)?;
        Ok(Self { conn: Mutex::new(conn) })
    }
}
```

**Key decision for the planner (from Claude's Discretion):** The CONTEXT says `Mutex<Connection>` (D-03d); RESEARCH Pattern 2 uses `Arc<tokio::sync::Mutex<StateStore>>` on the server side and `store.blocking_lock()` inside `spawn_blocking`. Two valid interpretations:
- (A) `StateStore` holds `std::sync::Mutex<Connection>`; `ExecutorServer` holds `Arc<StateStore>`; tools call `store.register(..)` which locks internally. Simpler API but `StateStore` must be `Send + Sync`.
- (B) `StateStore` holds `Connection` directly (no inner mutex); `ExecutorServer` holds `Arc<tokio::sync::Mutex<StateStore>>`; tools lock the outer mutex inside `spawn_blocking`. Matches RESEARCH Pattern 2 exactly.
Pick one; RESEARCH tilts toward (B).

---

### 6. `crates/executor-state/src/strategies.rs` (NEW — CRUD + content-address)

**Analog:** None in repo. RESEARCH §"Pattern 3: Content-Addressed Register" (lines 454-506) and §"Code Examples → Example 1: hash_source" (lines 670-695).

**Hash helper** (RESEARCH Example 1):
```rust
use sha2::{Digest, Sha256};

pub fn hash_source(source: &str) -> String {
    let mut h = Sha256::new();
    h.update(source.as_bytes());
    hex::encode(h.finalize())
}
```

**Register pattern** (RESEARCH Pattern 3 — the three-step decision tree is load-bearing):
```rust
pub fn register(
    conn: &Connection,
    name: &str, source: &str,
    description: Option<&str>, tags: Option<&[String]>,
) -> Result<RegisterOutcome, StateError> {
    let id = hash_source(source);
    // 1. Fast path: same id already exists → idempotent (D-01b same-source).
    if let Some(existing) = fetch_by_id(conn, &id)? {
        return Ok(RegisterOutcome::AlreadyExists(existing));
    }
    // 2. Pre-check active name collision → typed NameConflict (D-01b different-source).
    if let Some(active) = fetch_active_by_name(conn, name)? {
        return Err(StateError::NameConflict {
            attempted_name: name.to_string(),
            existing_strategy_id: active.id.clone(),
            existing_source_hash: active.id,
            existing_created_at: active.created_at,
        });
    }
    // 3. Insert.
    let now = now_rfc3339();
    let tags_json = tags.map(|t| serde_json::to_string(t).unwrap_or("[]".into()));
    conn.execute(
        "INSERT INTO strategies(id, name, source, description, tags, created_at)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![&id, name, source, description, tags_json, &now],
    )?;
    Ok(RegisterOutcome::Created { id, name: name.into(), created_at: now })
}
```

**Other methods to implement** (from CONTEXT D-06 / §code_context line 243):
- `list(conn, include_deleted: bool) -> Vec<StrategySummary>` — SELECT **without** `source` column (D-07a).
- `get_by_id(conn, id: &str) -> Option<Strategy>` — returns regardless of `deleted_at`.
- `get_by_name(conn, name: &str) -> Option<Strategy>` — active rows only.
- `soft_delete(conn, id: &str) -> Result<String>` — idempotent; returns existing `deleted_at` if already set (D-07c).
- `is_deleted(conn, id: &str) -> Option<bool>`.

**Pitfall call-outs:**
- SQL injection: always use `rusqlite::params![..]` — never string-format (RESEARCH §Security Domain row 1).
- Pitfall 9: `hash_source` matches even for soft-deleted rows; idempotent path returns them with `deleted_at` set (CONTEXT D-01b literal reading).

---

### 7. `crates/executor-state/src/runs.rs` (NEW — base CRUD)

**Analog:** `strategies.rs` (sibling) + RESEARCH §"Code Examples → Example 6: RunStatus" (lines 917-942) + §"Pitfall 6: ULID monotonicity".

**ULID generation pattern** (RESEARCH Pitfall 6):
```rust
// Phase 2 inserts runs from one code path (Phase 3 future), single Mutex<Connection> → no concurrency.
// For deterministic tests, seed a ulid::Generator with a fixed datetime:
let mut gen = ulid::Generator::new();
let id = gen.generate_from_datetime(now).unwrap();  // strictly-increasing within ms
```

**Methods to implement** (CONTEXT D-05a / D-06):
```rust
pub fn insert_run(conn: &Connection, strategy_id: &str, status: RunStatus) -> Result<String, StateError>;
pub fn update_run_status(conn: &Connection, run_id: &str, status: RunStatus) -> Result<(), StateError>;
pub fn get_run(conn: &Connection, run_id: &str) -> Result<Option<Run>, StateError>;
```

**Validation guard** (CONTEXT D-05c + RESEARCH Example 6 `phase2_emittable()`):
```rust
// At the StateStore::insert_run / update_run_status entry: reject future-reserved values
// with StateError::InvalidInput(format!("status {status:?} is reserved for Phase 5/6"))
if !status.phase2_emittable() { return Err(...); }
```

**FK constraint** (CONTEXT §specifics line 264): `runs.strategy_id REFERENCES strategies(id)` is `NO ACTION` (default) — cascade is forbidden; soft-delete keeps FK valid.

---

### 8. `crates/executor-state/tests/strategy_roundtrip.rs`, `partial_index_behaviour.rs`, `run_base_model.rs` (NEW — repository tests)

**Analog:** `crates/executor-core/tests/schema_snapshots.rs:1-80` (crate-scoped `tests/*.rs` harness with per-file `#[test]` fns).

**Test-file structure to copy** (schema_snapshots.rs):
```rust
//! Module docstring (2-line purpose)
use executor_core::schema::{ ... };        // absolute crate path
use schemars::schema_for;

fn assert_schema_matches_golden<S: serde::Serialize>(name: &str, schema: S) { ... }  // helper

#[test]
fn strategy_register_input_schema_stable() {
    assert_schema_matches_golden("StrategyRegisterInput", schema_for!(StrategyRegisterInput));
}
```

**What to copy:**
- One `tests/<topic>.rs` file per conceptual unit (not one big `tests.rs`).
- Module docstring explaining the slice being tested.
- A small private helper (`fresh_memory_store()` — RESEARCH Example 5) used by every `#[test]` fn.
- `#[test]` naming echoes the assertion verbatim (`schema_is_idempotent`, `partial_unique_index_blocks_duplicate_active_name`, `run_roundtrip_insert_get_update_status`).

**Seed helper** (RESEARCH Example 5):
```rust
pub fn fresh_memory_store() -> Result<StateStore, StateError> {
    StateStore::open(Path::new(":memory:"))
}
pub fn seed_strategies(store: &StateStore, n: usize) -> Vec<String> {
    (0..n).map(|i| {
        let source = format!("// strategy {i}\n");
        let name = format!("s{i}");
        store.register(&name, &source, None).unwrap().strategy_id()
    }).collect()
}
```

**Pitfall call-out:** for WAL-specific tests use `tempfile::tempdir()` (RESEARCH Pitfall 5 — `NamedTempFile` leaks `-wal`/`-shm` sidecars). For in-process logic, stick with `:memory:`.

---

### 9. `crates/executor-core/src/schema/strategy.rs` (MODIFY — add XOR enum + response types)

**Analog:** self — extend existing file. Phase 1 shape (`crates/executor-core/src/schema/strategy.rs:1-31`):
```rust
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "Register a JavaScript strategy (Phase 2 implements persistence).")]
pub struct StrategyRegisterInput {
    #[schemars(description = "Human-readable strategy name; does not need to be unique globally.")]
    pub name: String,
    #[schemars(description = "JavaScript source — executed in a sandbox starting Phase 3.")]
    pub source: String,
    #[schemars(description = "Optional metadata blob persisted alongside the strategy.")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}
```

**What to add** (RESEARCH §Pattern 4 + RESEARCH Open Question 4):

1. **`StrategyRegisterInput` split** — choose between "keep `metadata: Value`" (A) and "split into top-level `description: Option<String>` + `tags: Option<Vec<String>>`" (B). RESEARCH Open Q4 recommends (B). Decision triggers golden regeneration — reflect in `schema_contract_round_trip` test payload.

2. **`StrategyGetInput` XOR enum** (RESEARCH §Pattern 4):
```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
pub enum StrategyGetInput {
    ById { strategy_id: String },
    ByName { name: String },
}
```
Note: schemars 1.2 emits `anyOf` not `oneOf` (RESEARCH Pitfall 7) — accept per RESEARCH recommendation.

3. **Response types** (CONTEXT D-07 / D-07a / D-07b / D-07c — shape verbatim). Derive `Debug, Clone, Serialize, Deserialize, JsonSchema` on every response struct so schema goldens can lock the shape:
```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StrategyRegisterResponse {
    pub strategy_id: String,
    pub name: String,
    pub created_at: String,
    pub already_exists: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub existing_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub existing_metadata: Option<serde_json::Value>,
}
// + StrategyListResponse { strategies: Vec<StrategySummary> } (no source)
// + StrategyGetResponse { strategy_id, name, source, description, tags, created_at, deleted_at }
// + StrategyDeleteResponse { strategy_id, deleted_at }
```

**Pattern to copy from existing file:**
- `#[schemars(description = "...")]` attribute on every field for agent-facing tooltips.
- `#[serde(default, skip_serializing_if = "Option::is_none")]` for optional response fields.

---

### 10. `crates/executor-core/src/schema/execution.rs` (MODIFY — add RunStatus + ExecutionGetResponse)

**Analog:** self — extend existing file. Current shape (`crates/executor-core/src/schema/execution.rs:1-17`):
```rust
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "Input for execution_get (Phase 2 implements persistence).")]
pub struct ExecutionIdInput {
    /// Opaque execution identifier returned from a previous `strategy_run_once`.
    #[schemars(description = "...")]
    pub execution_id: String,
}
```

**What to add** (RESEARCH Example 6 — verbatim RunStatus):
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued, Running, Succeeded, Failed,
    Canceled, SimulationDenied, PolicyDenied,  // Phase 5/6 future-reserved (D-05)
}

impl RunStatus {
    pub fn phase2_emittable(self) -> bool {
        matches!(self, Self::Queued | Self::Running | Self::Succeeded | Self::Failed)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExecutionGetResponse {
    pub run_id: String,
    pub strategy_id: String,
    pub status: RunStatus,
    pub started_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
```

**Critical** (CONTEXT D-05): **all seven** enum values must appear in the `RunStatus.json` golden even though Phase 2 only emits the first four. The `run_status_schema_includes_future_variants` test (D-08a) asserts this.

---

### 11. `crates/executor-core/tests/schema_snapshots.rs` (MODIFY — add 7 new tests)

**Analog:** self — identical pattern repeated. Existing (lines 46-80):
```rust
#[test]
fn strategy_register_input_schema_stable() {
    assert_schema_matches_golden("StrategyRegisterInput", schema_for!(StrategyRegisterInput));
}
```

**What to copy:** literally one `#[test] fn ..._schema_stable()` per new struct. No helper changes.

**New tests to add:**
- `strategy_get_input_schema_stable` → `StrategyGetInput`
- `run_status_schema_stable` → `RunStatus`
- `strategy_register_response_schema_stable` → `StrategyRegisterResponse`
- `strategy_list_response_schema_stable` → `StrategyListResponse`
- `strategy_get_response_schema_stable` → `StrategyGetResponse`
- `strategy_delete_response_schema_stable` → `StrategyDeleteResponse`
- `execution_get_response_schema_stable` → `ExecutionGetResponse`

---

### 12. `crates/executor-core/tests/schemas/*.json` (7 NEW + 1 REGENERATE)

**Analog:** `crates/executor-core/tests/schemas/StrategyRegisterInput.json` (existing; 24 lines).

**Existing golden pattern** (StrategyRegisterInput.json):
```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "StrategyRegisterInput",
  "description": "Register a JavaScript strategy (Phase 2 implements persistence).",
  "type": "object",
  "properties": { ... },
  "required": ["name", "source"]
}
```

**What to copy:** nothing by hand — all goldens are regenerated by `UPDATE_SCHEMAS=1 cargo test -p executor-core --test schema_snapshots`. Just verify the diff, commit the file.

**Regeneration:** If the planner chooses RESEARCH Open Q4 option (B) (split `description`/`tags` out of `metadata`), `StrategyRegisterInput.json` MUST be regenerated. Call this out explicitly in the plan so reviewers see the intentional diff.

---

### 13. `crates/executor-mcp/Cargo.toml` (MODIFY — already depends on executor-state)

**Analog:** self (current file shown in Cargo.toml above).

**What to check:** `executor-state = { path = "../executor-state" }` is already present (line 19 of current Cargo.toml). No change required unless executor-state's `tempfile` test-dep leaks. Workspace-level `tempfile` dep may need adding for integration tests `strategies_persist_across_restart` (per-restart file DB).

---

### 14. `crates/executor-mcp/src/config.rs` (MODIFY — add `[state]` section)

**Analog:** self — apply the existing `[logging]` pattern. Current shape (config.rs:15-40):
```rust
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub logging: LoggingConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoggingConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
}

fn default_log_level() -> String { "info".into() }

impl Default for LoggingConfig {
    fn default() -> Self { Self { level: default_log_level() } }
}
```

**Adaptation for `[state]`** (CONTEXT D-03a / D-03e):
```rust
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub logging: LoggingConfig,
    #[serde(default)]
    pub state: StateConfig,          // Option<StateConfig> ≈ StateConfig::default() via #[serde(default)]
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateConfig {
    #[serde(default = "default_state_path")]
    pub path: String,                // "./state.db"; ":memory:" allowed
}
fn default_state_path() -> String { "./state.db".into() }
impl Default for StateConfig { fn default() -> Self { Self { path: default_state_path() } } }
```

**What to copy from existing file:**
- Exact module-docstring style (config.rs:1-11) — update priority list to mention `[state]`.
- `#[serde(deny_unknown_fields)]` on both structs (D-03a-alignment with Phase 1 principle).
- Unit test style (config.rs:78-106):
```rust
#[test]
fn rejects_unknown_top_level_fields() {
    let err = toml::from_str::<Config>("[state]\nsomething = 1\n").unwrap_err();
    assert!(err.to_string().to_lowercase().contains("state"));
}
```
**⚠ This existing test must be updated** — `[state]` will no longer be unknown. Replace with a genuinely-unknown section like `[foo]` or a new test asserting the `[state].something` unknown-field path.

**Known issue** (CONTEXT §canonical_refs line 218 + RESEARCH Open Q2): `--config=PATH` (with `=`) is NOT parsed today. Planner decides whether to fix alongside this config extension.

---

### 15. `crates/executor-mcp/src/errors.rs` (MODIFY — add state-error mapping)

**Analog:** self — extend existing. Current (errors.rs:1-32):
```rust
use rmcp::{ErrorData as McpError, model::ErrorCode};
use serde_json::json;

const UNIMPLEMENTED_CODE: ErrorCode = ErrorCode(-32010);

pub fn unimplemented_err(tool_name: &'static str, phase: u8) -> McpError {
    McpError::new(
        UNIMPLEMENTED_CODE,
        format!("{tool_name} is not implemented yet (lands in Phase {phase})"),
        Some(json!({
            "code": "unimplemented", "tool": tool_name,
            "phase": phase, "hint": format!("will be implemented when Phase {phase} lands"),
        })),
    )
}
```

**What to add** (RESEARCH Example 2 — `map_state_error` + `invalid_params`):
```rust
use executor_state::StateError;

pub const STORAGE_NOT_FOUND:    ErrorCode = ErrorCode(-32014);
pub const STORAGE_NAME_CONFLICT:ErrorCode = ErrorCode(-32015);
pub const STORAGE_ERROR:        ErrorCode = ErrorCode(-32016);
pub const INVALID_PARAMS:       ErrorCode = ErrorCode(-32602);   // JSON-RPC 2.0 standard

pub fn map_state_error(e: StateError) -> McpError {
    match e {
        StateError::NotFound(what) => McpError::new(
            STORAGE_NOT_FOUND,
            format!("not found: {what}"),
            Some(json!({ "code": "not_found", "resource": what })),
        ),
        StateError::NameConflict { attempted_name, existing_strategy_id, existing_created_at, .. } =>
            McpError::new(STORAGE_NAME_CONFLICT, /* full message as in CONTEXT specifics */, Some(json!({ ... }))),
        StateError::InvalidInput(msg) => invalid_params(msg),
        StateError::Storage(msg) => McpError::new(STORAGE_ERROR, /* ... */, Some(json!({ "code": "storage_error" }))),
    }
}

pub fn invalid_params(msg: impl Into<String>) -> McpError {
    McpError::new(INVALID_PARAMS, msg.into(), Some(json!({ "code": "invalid_params" })))
}
```

**What to copy from existing file:**
- Exact shape of `McpError::new(code, msg, Some(json!({...})))` with `code` as kebab-case string inside `data`.
- A `#[cfg(test)] mod tests` with a single unit test per new code asserting the `data` payload shape (mirrors `carries_structured_data` test at errors.rs:37-46).

**Decision** (RESEARCH Open Q3): verify `-32014 / -32015 / -32016` don't collide with any rmcp internal code via `grep -r 'ErrorCode(-3201' rmcp-*/`. RESEARCH Assumption A5 says clean.

---

### 16. `crates/executor-mcp/src/server.rs` (MODIFY — add `state` field)

**Analog:** self — extend existing. Current struct (server.rs:26-47):
```rust
#[derive(Clone)]
pub struct ExecutorServer {
    pub(crate) tool_router: ToolRouter<Self>,
    pub(crate) prompt_router: PromptRouter<Self>,
}

impl ExecutorServer {
    pub fn new() -> Self { Self { tool_router: Self::tool_router(), prompt_router: Self::prompt_router() } }
}

impl Default for ExecutorServer {
    fn default() -> Self { Self::new() }
}
```

**What to add** (RESEARCH Pattern 2 lines 410-415):
```rust
use std::sync::Arc;
use executor_state::StateStore;

#[derive(Clone)]
pub struct ExecutorServer {
    pub(crate) tool_router: ToolRouter<Self>,
    pub(crate) prompt_router: PromptRouter<Self>,
    pub(crate) state: Arc<tokio::sync::Mutex<StateStore>>,   // NEW
}

impl ExecutorServer {
    pub fn new(state_cfg: &crate::config::StateConfig) -> anyhow::Result<Self> {  // signature changes
        let store = StateStore::open(std::path::Path::new(&state_cfg.path))?;
        Ok(Self {
            tool_router: Self::tool_router(),
            prompt_router: Self::prompt_router(),
            state: Arc::new(tokio::sync::Mutex::new(store)),
        })
    }
}

// Default impl: delete OR make fallible with ::open(":memory:") — planner decides.
```

**What to copy from existing file:**
- Preserve the **single `impl ServerHandler for ExecutorServer`** block with both `#[tool_handler]` + `#[prompt_handler]` (server.rs:53-96; Pitfall 6 from Plan 01-03 summary).
- Keep `get_info` instructions block; update the "Phase 1 placeholder" language to reflect which tools are now live vs still phase-gated (Phase 3/5/6).
- The `list_resources` / `read_resource` delegations stay — only the implementations in `resources.rs` change.

**Breaking change:** `Default for ExecutorServer` must either (a) be removed (callers must provide config) or (b) default to `:memory:` — `main.rs` + every integration test currently calls `ExecutorServer::new()` with no args.

---

### 17. `crates/executor-mcp/src/tools.rs` (MODIFY — replace 5 placeholder bodies)

**Analog:** self — 5 methods to transition. Current example (tools.rs:34-40):
```rust
#[tool(name = "strategy_register", description = "...")]
async fn strategy_register(
    &self,
    Parameters(_input): Parameters<StrategyRegisterInput>,
) -> Result<CallToolResult, McpError> {
    Err(unimplemented_err("strategy_register", 2))
}
```

**Target shape** (RESEARCH §Pattern 2 lines 417-440):
```rust
#[tool(name = "strategy_register", description = "Register a JavaScript strategy (content-addressed; idempotent on same source).")]
async fn strategy_register(
    &self,
    Parameters(input): Parameters<StrategyRegisterInput>,
) -> Result<CallToolResult, McpError> {
    // 1. Handler-side validation (D-09) — cheap, synchronous, fail fast.
    validate_register(&input).map_err(invalid_params)?;

    // 2. Hand off blocking DB work.
    let state = self.state.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        let store = state.blocking_lock();
        store.register(&input.name, &input.source, input.description.as_deref(), input.tags.as_deref())
    })
    .await
    .map_err(|e| map_state_error(StateError::Storage(format!("spawn_blocking join: {e}"))))?
    .map_err(map_state_error)?;

    // 3. Serialize into CallToolResult.
    let body = serde_json::to_string(&StrategyRegisterResponse::from(outcome))
        .map_err(|e| map_state_error(StateError::Storage(e.to_string())))?;
    Ok(CallToolResult::success(vec![Content::text(body)]))
}
```

**Validation helper** (RESEARCH Example 3 — paste verbatim into tools.rs or a sibling `validation.rs`):
- `MAX_SOURCE_BYTES = 256 * 1024` (bytes check — `input.source.len()`)
- `MAX_NAME_CHARS = 128` (char check — `input.name.chars().count()`)
- `MAX_DESCRIPTION_CHARS = 4096`
- `MAX_TAGS = 16`, `MAX_TAG_CHARS = 64`
- Specific error messages naming the violated constraint (D-09b).

**Methods to transition** (5 of 8):
| Tool | Before | After pattern |
|------|--------|---------------|
| `strategy_register` | `Err(unimplemented_err("strategy_register", 2))` | validate → spawn_blocking → register → serialize |
| `strategy_delete` | `Err(unimplemented_err("strategy_delete", 2))` | spawn_blocking → soft_delete → `StrategyDeleteResponse` |
| `strategy_list` | `Ok(CallToolResult::success(vec![Content::text("[]")]))` | accept `include_deleted`, spawn_blocking → list → serialize |
| `strategy_get` | `Err(McpError::resource_not_found(...))` | match `StrategyGetInput` variant → get_by_id or get_by_name → serialize |
| `execution_get` | `Err(McpError::resource_not_found(...))` | spawn_blocking → get_run → `ExecutionGetResponse` or `NotFound` |

**Untouched** (stay Phase-N placeholders):
- `strategy_run_once` (Phase 6)
- `policy_get` (Phase 5 — returns placeholder object)
- `policy_update` (Phase 5)

**Pitfall call-outs** (RESEARCH §Pattern 2 notes):
- `tokio::sync::Mutex::blocking_lock()` panics if called outside `spawn_blocking` — always wrap.
- Do NOT hold the lock across an `await` — acquire-query-release inside the closure.

---

### 18. `crates/executor-mcp/src/resources.rs` (MODIFY — branch `read_resource_impl` on `strategy://`)

**Analog:** self — rewrite branch logic. Current (resources.rs:97-109):
```rust
pub(crate) async fn read_resource_impl(
    request: ReadResourceRequestParams,
    _ctx: RequestContext<RoleServer>,
) -> Result<ReadResourceResult, McpError> {
    Err(McpError::resource_not_found(
        "resource not found (Phase 1 placeholder surface — no objects stored yet)",
        Some(json!({ "uri": request.uri, "phase": 1 })),
    ))
}
```

**Target shape** (RESEARCH §Code Examples → Example 4 lines 847-892):
```rust
pub(crate) async fn read_resource_impl(
    request: ReadResourceRequestParams,
    _ctx: RequestContext<RoleServer>,
    state: Arc<tokio::sync::Mutex<StateStore>>,    // NEW param; call site in server.rs passes self.state.clone()
) -> Result<ReadResourceResult, McpError> {
    let uri = &request.uri;
    let Some(id) = uri.strip_prefix("strategy://") else {
        return Err(McpError::resource_not_found(
            format!("unsupported resource URI: {uri}"),
            Some(json!({ "uri": uri, "phase": 2 })),
        ));
    };
    // D-09a shape check: 64 hex chars.
    if id.len() != 64 || !id.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)) {
        return Err(McpError::resource_not_found(
            format!("malformed strategy id: {id}"),
            Some(json!({ "uri": uri, "phase": 2 })),
        ));
    }

    let id_owned = id.to_string();
    let row = tokio::task::spawn_blocking(move || state.blocking_lock().get_by_id(&id_owned))
        .await
        .map_err(|e| map_state_error(StateError::Storage(format!("spawn_blocking join: {e}"))))??;

    match row {
        None => Err(McpError::resource_not_found(format!("strategy {uri} not found"), Some(json!({ "uri": uri })))),
        Some(s) => {
            let body = serde_json::to_string(&StrategyGetResponse::from(s))
                .map_err(|e| map_state_error(StateError::Storage(e.to_string())))?;
            Ok(ReadResourceResult {
                contents: vec![ResourceContents::text(body, uri).with_mime_type("application/json")],
            })
        }
    }
}
```

**What to copy from existing file:**
- Keep `list_resources_impl` / `list_resource_templates_impl` intact — only `read_resource_impl` changes.
- Keep the `make_template` helper (resources.rs:42-52) — Phase 2 may want to extend `list_resources_impl` to page through `StateStore::list` and emit real `Resource` objects (optional; not in RESEARCH §Code Examples).
- `execution://` and `journal://` URIs still return `resource_not_found` with `data.phase = 3` / `data.phase = 6` — don't touch.

**Security call-outs** (RESEARCH §Security Domain row 2): strict regex on id prevents `strategy://../../../etc/passwd` traversal — fail-closed.

---

### 19. `crates/executor-mcp/src/main.rs` (MODIFY — pass config to server)

**Analog:** self (main.rs:1-16):
```rust
#[tokio::main]
async fn main() -> Result<()> {
    let cfg = config::load()?;
    logging::init(&cfg)?;
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "executor-mcp starting");
    let service = ExecutorServer::new().serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
```

**One-line change:**
```rust
let service = ExecutorServer::new(&cfg.state)?.serve(stdio()).await?;
```

---

### 20. `crates/executor-mcp/tests/stdio_handshake.rs` (MODIFY — add 12 new tests)

**Analog:** self — add tests in the style already established (lines 212-282 `resources_surface_matches_contract`, lines 140-210 `readonly_tools_return_placeholder`).

**Test shape to copy** (stdio_handshake.rs:213-281):
```rust
#[tokio::test]
async fn resources_surface_matches_contract() -> Result<()> {
    let mut proc = spawn_server().await?;
    let _ = initialize(&mut proc).await?;

    send(&mut proc, json!({ "jsonrpc": "2.0", "id": 2, "method": "resources/list" })).await?;
    let r = recv(&mut proc).await?;
    // ... assertions ...
    proc.child.kill().await?;
    Ok(())
}
```

**Key reused patterns:**
- `let mut proc = spawn_server().await?; let _ = initialize(&mut proc).await?;` boilerplate (every test).
- `send()` + `recv()` JSON-RPC roundtrip (common::recv already asserts `jsonrpc: "2.0"`).
- Explicit `proc.child.kill().await?` at end (so dropped-process doesn't leak pipes).
- JSON assertion on `r["error"]["code"]` + `r["error"]["data"]["phase"]` for phase-gated errors.

**New tests to add** (CONTEXT D-08a + RESEARCH §"Phase Requirements → Test Map" line 1017-1037):
1. `strategy_register_creates_row` — first register returns `already_exists: false`
2. `strategy_register_idempotent_same_source` — second call with same source returns `already_exists: true`
3. `strategy_register_conflict_same_name_different_source` — returns `-32015` with `existing_strategy_id`
4. `strategy_register_rejects_oversized_source` — `-32602` with byte count in message
5. `strategy_register_rejects_empty_name` — `-32602`
6. `strategy_list_excludes_source_payload` — response items lack `source` key
7. `strategy_list_filters_deleted_by_default` — soft-deleted row absent; `include_deleted=true` returns it
8. `strategy_get_by_id_returns_source` — response includes `source`
9. `strategy_get_by_name_only_returns_active` — soft-deleted name → `not_found`
10. `strategy_delete_is_soft_and_idempotent` — second call returns same `deleted_at`
11. `soft_deleted_name_can_be_reused` — register with reused name succeeds after soft-delete
12. `run_roundtrip_insert_get_update_status` — if exposed via test-only helper, otherwise this lives in `crates/executor-state/tests/`
13. `execution_get_returns_not_found_when_empty` — `-32014`
14. `resource_read_strategy_uri_returns_body` — `resources/read` with real id returns strategy JSON
15. `strategies_persist_across_restart` — spawn twice against tempdir DB; second call sees first's strategies

**Test-DB isolation**: use `EXECUTOR_CONFIG=<tempdir>/config.toml` via a helper in `tests/common/mod.rs` that writes a `[state] path = "<tempdir>/state.db"` config before calling `spawn_server`. For most tests `:memory:` via a dedicated config suffices.

**Update existing test** (stdio_handshake.rs:93-136 `unimplemented_tools_return_phase_hint`):
- Remove `strategy_register` and `strategy_delete` from the `cases` array (they are no longer `-32010`).
- Leave `strategy_run_once` (Phase 6) and `policy_update` (Phase 5).
- Similarly, `readonly_tools_return_placeholder` (stdio_handshake.rs:140-210) must update the `strategy_get` / `execution_get` assertions — they now return typed storage errors when the store is empty, not always `resource_not_found`.

---

### 21. `crates/executor-mcp/tests/common/mod.rs` (MODIFY — add test-DB helpers)

**Analog:** self — add helpers alongside existing. Current (common/mod.rs:19-46 `spawn_server`, 48-68 `send`/`recv`, 70-95 `initialize`).

**What to add:**
- `spawn_server_with_state_db(path: &Path)` — sets `EXECUTOR_CONFIG` env var pointing at a temp config that declares `[state].path = path`.
- `fresh_tempdir_state_db()` — returns a `tempfile::TempDir` + path, keeps tempdir alive for the test.
- Test helper to call `tools/call` for `strategy_register` and parse `StrategyRegisterResponse` — reduces boilerplate across the 12 new tests.

**Pattern to copy** (common/mod.rs:19-27 `spawn_server`):
```rust
pub async fn spawn_server() -> Result<ServerProc> {
    let bin = env!("CARGO_BIN_EXE_executor-mcp");
    let mut child = Command::new(bin)
        .env("RUST_LOG", "error")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    // ...
}
```

**Key:** add env var via `.env("EXECUTOR_CONFIG", config_path_str)` before `.spawn()`.

---

### 22. `config.example.toml` (MODIFY — append `[state]`)

**Analog:** self (the Phase 1 sample).

**What to add:**
```toml
[state]
# Path to the SQLite database file. Use ":memory:" for ephemeral testing.
path = "./state.db"
```

Keep the existing `[logging]` section intact; add a trailing comment mentioning Phase 4+ extensions will add `[evm]` / `[policy]` / `[signer]` sections.

---

## Shared Patterns

### A. Module docstring convention

**Source:** every `crates/executor-mcp/src/*.rs` (config.rs:1-11, errors.rs:1-13, server.rs:1-9, resources.rs:1-30).

**Apply to:** every new `crates/executor-state/src/*.rs` file.

**Shape:**
```rust
//! One-line module purpose.
//!
//! Extended paragraph: which CONTEXT decision drives this file (e.g., "D-03c
//! pragma sequence"), which RESEARCH §Pattern N is the source, and any pitfall
//! callouts the future reader needs (e.g., "PRAGMA foreign_keys is per-connection").
```

### B. `#[deny(...)]` tripwire

**Source:** `crates/executor-mcp/src/lib.rs:1` + `crates/executor-mcp/src/main.rs:1` + `crates/executor-state/src/lib.rs:1`.

**Apply to:** every new lib/bin crate root in `executor-state`. Belt-and-suspenders against the workspace-level lint denylist.

```rust
#![deny(clippy::print_stdout, clippy::print_stderr, clippy::dbg_macro)]
```

### C. Parameterized SQL — zero string-concat

**Source:** RESEARCH §Security Domain + Pattern 3 line 499-503.

**Apply to:** every `StateStore` method.

```rust
conn.execute(
    "INSERT INTO strategies(id, name, source, description, tags, created_at)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    rusqlite::params![&id, name, source, description, tags_json, &now],
)?;
```

Never `format!` into SQL. Use `rusqlite::params![..]` or `named_params!{..}`.

### D. `spawn_blocking` + `blocking_lock` bridge

**Source:** RESEARCH §"Pattern 2: Async Handler → spawn_blocking → sync rusqlite" lines 398-445.

**Apply to:** every `tools.rs` method that touches `self.state` AND `resources.rs::read_resource_impl`.

```rust
let state = self.state.clone();
let result = tokio::task::spawn_blocking(move || {
    let store = state.blocking_lock();
    store.some_method(&args)
})
.await
.map_err(|e| map_state_error(StateError::Storage(format!("spawn_blocking join: {e}"))))?
.map_err(map_state_error)?;
```

Never `.lock().await` then call rusqlite — blocks tokio schedulers.
Never hold the lock across an `await`.

### E. Structured MCP error payload

**Source:** `crates/executor-mcp/src/errors.rs:22-31` (existing `unimplemented_err`).

**Apply to:** every new error constructor (`map_state_error`, `invalid_params`).

```rust
McpError::new(
    <ErrorCode>,
    <human message naming the specific constraint violated>,
    Some(json!({ "code": "<kebab-case>", /* extra structured fields */ })),
)
```

Agents key off `error.data.code` string + typed fields. Never rely on message-text parsing (D-09b).

### F. Schema golden harness (regenerate on intentional change)

**Source:** `crates/executor-core/tests/schema_snapshots.rs:23-44` (`assert_schema_matches_golden`).

**Apply to:** every new input/response type. Run `UPDATE_SCHEMAS=1 cargo test -p executor-core --test schema_snapshots` to create/refresh; commit both the test addition and the `.json` in the same commit.

```rust
#[test]
fn <struct_name>_schema_stable() {
    assert_schema_matches_golden("<StructName>", schema_for!(<StructName>));
}
```

### G. `#[serde(deny_unknown_fields)]` on all config/input structs

**Source:** `crates/executor-mcp/src/config.rs:16` and `17:24`; RESEARCH §Pattern 4 on `StrategyGetInput`.

**Apply to:** `StateConfig`, `StrategyGetInput`. Intentionally NOT applied to `StrategyRegisterInput`'s optional metadata object (D-09 "unknown metadata fields ignored for forward-compat").

### H. Integration test teardown

**Source:** every test in `crates/executor-mcp/tests/stdio_handshake.rs` (e.g., line 87 `proc.child.kill().await?;`).

**Apply to:** every new `#[tokio::test]` in Phase 2. Pair `kill_on_drop(true)` + explicit `.kill().await?` to prevent pipe leaks under flaky CI.

---

## No Analog Found

| File | Role | Data Flow | Reason | Substitute |
|------|------|-----------|--------|------------|
| `crates/executor-state/src/schema.rs` | storage-init (DDL + pragmas) | batch | No SQL in the repo yet; this is the first persistence code. | RESEARCH §Pattern 1 (verified via `/tmp/rusqlite_probe`) |
| `crates/executor-state/src/strategies.rs` (CRUD core) | repository | CRUD sync | No repository pattern exists pre-Phase 2 — `executor-state` was an empty stub. | RESEARCH §Pattern 3 + §Pitfalls 2, 8, 9 |
| `crates/executor-state/src/runs.rs` | repository | CRUD sync | Same as strategies.rs — use sibling file as analog once written; RESEARCH Example 6 covers the RunStatus shape. | RESEARCH §Pitfall 6 (ULID monotonicity) + Example 6 |

For these three files, the planner should cite RESEARCH section numbers (not file paths) in plan tasks. Every other file has a concrete in-repo analog.

---

## Metadata

**Analog search scope:**
- `crates/executor-mcp/src/` (9 files)
- `crates/executor-mcp/tests/` (2 files)
- `crates/executor-core/src/schema/` (6 files)
- `crates/executor-core/tests/` (1 test harness + 7 goldens)
- `crates/executor-state/` (2 files — stub)
- `crates/executor-signer/` (1 file — stub)

**Files scanned:** 25 (entire workspace — small enough to fully index).

**Key insight:** Phase 2 is heavily pattern-driven — 20/23 files have strong in-repo analogs from Phase 1's structural choices (module docstrings, `#[deny]` tripwires, `#[serde(deny_unknown_fields)]`, schema goldens, stdio integration tests). The three genuinely new patterns (pragma+DDL, CRUD repo, ULID generation) are all covered by verified RESEARCH examples. The planner's leverage is high: most plan tasks become "extend file X in the style of file Y lines N-M" rather than green-field design.

**Pattern extraction date:** 2026-04-24
