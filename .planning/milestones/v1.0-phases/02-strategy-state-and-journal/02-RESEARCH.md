# Phase 2: Strategy State and Journal - Research

**Researched:** 2026-04-24
**Domain:** Local SQLite persistence (schema + repository layer) wired into rmcp 1.5 async tool handlers
**Confidence:** HIGH (most claims verified by compiling+running probes against `rusqlite 0.39 + bundled SQLite 3.51.3`, `ulid 1.2.1`, `sha2 0.11`, `schemars 1.2`; rmcp 1.5 patterns cross-checked against the official SDK's `counter_stdio` example).

## Summary

Phase 2 turns the Phase 1 placeholder surface into a real persistence layer. `executor-state` becomes a concrete crate that opens a `rusqlite::Connection`, runs idempotent `CREATE TABLE IF NOT EXISTS` migrations, applies `WAL + synchronous=NORMAL + foreign_keys=ON` pragmas, and exposes a single `StateStore` type with content-addressed strategy CRUD plus a base-model run repository. The MCP layer changes from an 8-tool dispatch that calls `unimplemented_err` for write-capable tools into one that routes 5 tools (`strategy_register`, `strategy_list`, `strategy_get`, `strategy_delete`, `execution_get`) through `StateStore`, plus populates `resources/read` for `strategy://{id}`.

The research confirms every risky corner empirically:
- `rusqlite 0.39` with `bundled` feature ships **SQLite 3.51.3** (verified). Partial unique indexes with `WHERE deleted_at IS NULL` behave exactly as D-01c requires (multiple deleted rows with the same name coexist; one active row wins). `Connection` is `Send` but **not `Sync`**.
- The rmcp 1.5 canonical pattern (verified against `examples/servers/common/counter.rs`) is `Arc<tokio::sync::Mutex<State>>` on a `#[derive(Clone)]` server struct. For `!Sync` + blocking operations, the refinement is `tokio::task::spawn_blocking` around the connection-locked block.
- `#[serde(untagged)]` enums produce `anyOf` (not `oneOf`) in schemars 1.2, but serde rejects payloads that match both variants at runtime, so D-01d's XOR semantics hold behaviorally. If strict `oneOf` wire shape is required, the planner must override via `#[schemars(schema_with = "...")]`.

**Primary recommendation:** adopt `rusqlite 0.39 + bundled + serde_json` + `sha2 0.11` + `ulid 1.2` + `chrono 0.4.44` (RFC3339 UTC). Wrap a single `Arc<tokio::sync::Mutex<Connection>>` on `ExecutorServer`, run repository calls inside `tokio::task::spawn_blocking`. Use an untagged enum with two variants for `strategy_get`'s XOR input.

<user_constraints>
## User Constraints (from 02-CONTEXT.md)

### Locked Decisions

**Strategy identity & mutability:**
- **D-01:** Strategy is content-addressed, immutable. `strategy_id = hex(sha256(source))` — lowercase hex of the source text bytes' SHA-256 hash.
- **D-01a:** Hash covers **source only** — `name`/`metadata` do not affect id. Metadata-only difference ⇒ same strategy (idempotent).
- **D-01b:** `strategy_register(name, source, metadata)` re-call semantics:
  - Same source ⇒ idempotent. Return existing row with `already_exists: true` + `existing_name` + `existing_metadata`. Do **not** overwrite name/metadata.
  - Different source colliding with existing active name ⇒ **conflict error** (`-32015` storage_conflict) carrying `existing_strategy_id`, `existing_source_hash`, `existing_created_at`.
  - Metadata-only mutation is unsupported in v1.
- **D-01c:** `name` is UNIQUE among **non-deleted** strategies. Enforced by `CREATE UNIQUE INDEX idx_strategies_name_active ON strategies(name) WHERE deleted_at IS NULL;`.
- **D-01d:** Agent can query strategy by id OR name:
  - `strategy_get(strategy_id=...)` — exact id; deleted rows included.
  - `strategy_get(name=...)` — active only; unique; not_found if absent.
  - Resource URI `strategy://{id}` takes id only.
  - Input schema is `oneOf` (id XOR name).

**Delete semantics:**
- **D-02:** Soft delete via `strategies.deleted_at TIMESTAMP NULL`.
- **D-02a:** `strategy_list(include_deleted?: bool = false)`. Default hides deleted.
- **D-02b:** `strategy_get(id)` returns regardless of `deleted_at`, with the column in the response. `strategy_get(name)` active-only.
- **D-02c:** `strategy_run_once` on a deleted strategy ⇒ rejected (Phase 3).

**DB file & migrations:**
- **D-03:** `rusqlite 0.39` sync API (locked by AGENTS.md + STACK.md). No sqlx, no sea-orm.
- **D-03a:** Config extension — new `[state]` section:
  ```toml
  [state]
  path = "./state.db"   # default; cwd-relative; absolute allowed; `:memory:` allowed
  ```
- **D-03b:** Migrations via `CREATE TABLE IF NOT EXISTS` (no migration crate). `schema_version` table deferred to v2.
- **D-03c:** On boot: open connection → `PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = ON;` → `CREATE TABLE IF NOT EXISTS` + partial unique index. Failure ⇒ startup error. WAL adopted for crash durability even though `Mutex<Connection>` negates concurrent-read benefit.
- **D-03d:** Connection wrapping = single `Mutex<Connection>`. No pool in v1.
- **D-03e:** `Config::state` is `Option<StateConfig>`; absent ⇒ `StateStore::open("./state.db")`.

**Schema (v1 minimal):**
- **D-04:** Only two tables in Phase 2 — `strategies`, `runs`. Journal detail tables land in Phase 3/5/6.
- **D-04a:** `strategies` columns — see decisions table verbatim.
- **D-04b:** `runs` columns (base): `id TEXT PRIMARY KEY` (ULID), `strategy_id TEXT NOT NULL REFERENCES strategies(id)`, `status TEXT NOT NULL`, `started_at TEXT NOT NULL`, `finished_at TEXT NULL`, `error TEXT NULL`.
- **D-04c:** Indexes — partial unique on `strategies.name` WHERE deleted_at IS NULL; regular on `runs.strategy_id`; regular on `strategies.deleted_at`.
- **D-04d:** Metadata schema deliberately small (description + tags).

**Run status & lifecycle:**
- **D-05:** Declare **all seven** `RunStatus` values in Phase 2 (Queued, Running, Succeeded, Failed, Canceled, SimulationDenied, PolicyDenied). Phase 2 code only emits the first four.
- **D-05a:** Phase 2 does not insert runs from any tool path; but the `RunRepo::{insert, update_status, get}` methods are complete and tested via integration roundtrip.
- **D-05b:** `run_id` = ULID (26 chars, Crockford Base32). `ulid` crate.
- **D-05c:** Phase 2 must not emit `Canceled / SimulationDenied / PolicyDenied`.

**Repository layer:**
- **D-06:** One pub `StateStore` struct. Internal separation into Strategy* / Run* sections. No trait abstraction in v1.
- **D-06a:** `StateError` (thiserror) → MCP error code mapping: `not_found(-32014)`, `name_conflict(-32015)`, `storage_error(-32016)`.

**MCP tool transitions:**
- **D-07:** `strategy_register` response shape as specified.
- **D-07a:** `strategy_list` response excludes `source` (no large JS blobs per-call).
- **D-07b:** `strategy_get` includes `source`. `strategy://{id}` resource read returns full strategy.
- **D-07c:** `strategy_delete` idempotent — repeated calls return the same `deleted_at`.
- **D-07d:** `execution_get` hits real DB; returns not_found when no run exists.

**Input validation:**
- **D-09:** Enforce both in JSON Schema and in handler code.
  - `source`: non-empty, UTF-8, max 256 KiB (262144 bytes).
  - `name`: non-empty (no whitespace-only), max 128 scalars.
  - `description`: optional, max 4096 scalars.
  - `tags`: optional, max 16 items, each max 64 scalars, no whitespace-only.
  - Unknown metadata fields ignored (forward compat); wrong types rejected.
- **D-09a:** `strategy_delete.strategy_id` must match `^[0-9a-f]{64}$` at schema level.
- **D-09b:** Validation errors name the violated constraint (no generic "invalid input").

**Testing:**
- **D-08:** Integration tests use `:memory:` SQLite + real `StateStore`. No mocking.
- **D-08a:** Integration tests (12) listed verbatim in CONTEXT §Testing.
- **D-08b:** Repository-level unit tests live under `crates/executor-state/tests/`.

### Claude's Discretion

- `executor-state` internal module split (`schema.rs` / `strategies.rs` / `runs.rs` / `error.rs`).
- `rusqlite` feature flag choice (bundled vs system). Binary-size trade-off.
- SHA-256 crate (`sha2` is the incumbent from STACK.md).
- ULID crate (`ulid` is the incumbent from STACK.md).
- Datetime serialization (`chrono` vs `time`).
- Config loader extension pattern for `[state]`.
- Exact MCP error code numbers (D-06a gives the guide).
- Representation of the `strategy_get` XOR input schema.

### Deferred Ideas (OUT OF SCOPE)

- `strategy_metadata_update` tool.
- Strategy versioning by name / "latest" alias.
- Connection pool (r2d2_sqlite).
- `schema_version` + versioned migrations.
- XDG/OS-specific data dir.
- Binary blob strategies (WASM, bytecode).
- Strategy import/export (JSON/tar.gz bundles).
- Making `source` size cap tunable beyond 256 KiB.

</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| **STR-01** | Agent can register a JavaScript strategy with name, source, and metadata. | §Standard Stack (`rusqlite` + `sha2` + `schemars` validation); §MCP Tool Transition → `strategy_register` flow; §Input Validation; §Code Examples → `StateStore::register`. |
| **STR-02** | Agent can list, inspect, and delete registered strategies. | §MCP Tool Transition → `list` / `get` / `delete`; §Schema Shape (partial unique index for soft-delete-then-reuse); §Resource URI parsing for `strategy://{id}`. |
| **STJ-01** | Runtime persists strategies and strategy metadata locally. | §DB File & Migrations (WAL + FK + IF NOT EXISTS); §Schema Shape `strategies` columns; §Testing (`:memory:` + tempfile integration patterns). |
| **STJ-02** | Runtime persists each strategy run with run ID, strategy ID, started time, and status. | §Schema Shape `runs` columns; §Run Status Enum (7 values declared, 4 emitted); §ULID choice; §Testing (`run_roundtrip_insert_get_update_status`). |

</phase_requirements>

## Project Constraints (from CLAUDE.md + AGENTS.md)

- **Git commits must not mention Claude** (global user rule; also enforced in every phase summary: commits are `feat/fix/docs(02):` without AI attribution).
- **Destructive worktree git commands forbidden** (no `git reset --hard`, `git clean -fd`, etc., without explicit user request).
- **No stdout writes from the MCP server** (AGENTS.md hard boundary + workspace clippy denylist on `print_stdout`/`print_stderr`/`dbg_macro`). All DB error surfaces must route through `tracing` on stderr, never stdout.
- **No sqlx / sea-orm / tokio-rusqlite** (AGENTS.md stack lock: `rusqlite` only).
- **Strategy code must not access FS / network / keys** (this is Phase 3 concern; Phase 2 just stores source text — no execution yet).
- **Simulation and policy run before signing** (future phases — but the `runs` table today must accept status values from the full enum so downstream phases don't ALTER TABLE).

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Hashing source → strategy_id | `executor-state::strategies::hash_source` | — | Content addressing is part of storage contract; keeping it beside the repo ensures the hash rule travels with the schema. |
| SQLite connection + migrations | `executor-state::StateStore::open` | `executor-mcp::server::ExecutorServer::new` | Repo owns the lifecycle; server owns the `Arc<Mutex<>>` wrapping and passes it into tool handlers. |
| Input validation (size/length limits) | `executor-mcp::tools` (handler-side re-check) | `schemars` derive on `executor-core::schema::strategy` (agent-facing contract) | D-09 requires both layers. Schema broadcasts contract; handler enforces at runtime. |
| Error code mapping (StateError → McpError) | `executor-mcp::errors` | `executor-state::error::StateError` (thiserror) | Domain crate stays rmcp-free (per Phase 1 decision: executor-core/state don't depend on rmcp). |
| Response types (StrategyRegisterResponse etc.) | `executor-core::schema::strategy` (new) | `executor-mcp::tools` (serialization) | Keep response shapes adjacent to input shapes so golden tests live in one place. |
| Resource read `strategy://{id}` | `executor-mcp::resources::read_resource_impl` | `StateStore::get_by_id` | Phase 1 returned -32002 unconditionally; Phase 2 parses URI, delegates to store. |
| `oneOf` XOR input | `executor-core::schema::strategy::StrategyGetInput` | — | `#[serde(untagged)]` enum sits with other schema types. |
| Config extension `[state]` | `executor-mcp::config::StateConfig` | — | Follows Phase 1 `[logging]` precedent. |

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `rusqlite` | `0.39.0` | SQLite wrapper | [VERIFIED: `cargo info rusqlite` → 0.39.0; AGENTS.md lock; STACK.md]. MSRV listed as "unknown" on crates.io; no concerns — workspace is Rust 2024 edition. |
| `sha2` | `0.11.0` | SHA-256 hashing for `strategy_id` | [VERIFIED: `cargo info sha2` → 0.11.0, MSRV 1.85, RustCrypto canonical]. Probe confirmed `Sha256::new().update(..).finalize()` → `hex::encode(..)` yields the RFC 6234 vectors. |
| `hex` | `0.4` | Lowercase hex encoding | [VERIFIED: standard companion to sha2; `hex::encode(&[u8])` outputs lowercase by default]. Alternatives: `base16`, `subtle-encoding` — none widely adopted. |
| `ulid` | `1.2.1` | ULID-based `run_id` | [VERIFIED: `cargo info ulid` → 1.2.1, `std` feature default, `serde` feature available]. Probe confirmed 26-char Crockford Base32 output, `Ulid::new()` + `Generator::generate_from_datetime` for monotonic-within-ms test determinism. |
| `chrono` | `0.4.44` | RFC3339 UTC timestamps | [VERIFIED: `cargo info chrono` → 0.4.44]. Default features (`clock`, `std`, `oldtime`, `wasmbind`) — we only need `std` + `serde`. Consider `default-features = false, features = ["std", "clock", "serde"]` to keep binary lean. |
| `tempfile` | `3.27.0` | Test DB isolation | [VERIFIED: `cargo info tempfile` → 3.27.0]. Use `tempfile::NamedTempFile` for file-backed WAL tests; use `:memory:` for in-process tests that don't care about WAL behavior. |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `anyhow` | workspace | Error propagation in binary crate | `executor-mcp::config::load` already uses this; keep pattern. |
| `thiserror` | workspace | Typed errors in library crate | `executor-state::StateError` (D-06a). |
| `serde_json` | workspace | Tag array serialization | `tags` column is JSON-encoded text (no SQLite native JSON use in v1). |

### Alternatives Considered

| Instead of | Could Use | Tradeoff | Verdict |
|------------|-----------|----------|---------|
| `rusqlite` sync | `tokio-rusqlite` | Hides `spawn_blocking` behind async API | **Rejected** — AGENTS.md locks rusqlite; tokio-rusqlite internally runs a dedicated thread, which conflicts with D-03d single `Mutex<Connection>` model. |
| `rusqlite` sync | `sqlx` | Native async, compile-time checked queries | **Rejected** — AGENTS.md lock; sqlx also forces a runtime migration system and removes the "simple sync call" story. |
| `sha2` | `blake3` | 2-4x faster on modern CPUs | **Rejected** — FIPS 180-4 SHA-256 is what "content-addressed" means in most ecosystems. No performance need (256 KiB ceiling, one hash per register). Agents expect `hex(sha256(source))`. [CITED: CONTEXT D-01 explicitly names SHA-256]. |
| `sha2` | `ring` | FIPS-validated, bundled assembly | **Rejected** — `ring`'s API and build system are heavier. `sha2` is pure Rust, single-purpose. |
| `ulid` | `rusty_ulid` | Alternative; `2.0.0` on crates.io | **Rejected** — `ulid 1.2.1` is the more widely used fork (originally `dylanhart/ulid-rs`); monotonic API is `ulid::Generator`. `rusty_ulid` has similar API but lower ecosystem adoption. |
| `ulid` | `uuid` v7 | Time-ordered UUID alternative | **Rejected** — D-05b locks ULID. UUID v7 also valid but 36-char hyphenated form is less agent-friendly than 26-char Crockford. |
| `chrono` | `time` | Both valid; `time` is more modern, fewer CVEs historically | **Consideration** — `time` 0.3.47 is also fine. `chrono` chosen because (a) wider ecosystem familiarity, (b) rusqlite has first-class `chrono` feature, (c) RFC3339 via `DateTime::<Utc>::to_rfc3339_opts(SecondsFormat::Secs, true)` is one line. **Either is acceptable; defaulting to chrono for familiarity.** |
| `rusqlite`'s `chrono` feature | Manual RFC3339 string columns | Native `DateTime` binding vs TEXT roundtrip | **Recommended: manual TEXT columns.** Reason: D-04 stores RFC3339 strings already (the `deleted_at TEXT NULL` shape is the contract). Activating `rusqlite = { features = ["chrono"] }` adds a `ToSql/FromSql` for `DateTime` but we still store TEXT — the feature adds a dep (`chrono`) to rusqlite without simplifying our code. Keep chrono in the app layer; rusqlite sees TEXT. |

**Installation:**

```toml
# crates/executor-state/Cargo.toml
[dependencies]
executor-core = { path = "../executor-core" }
rusqlite = { version = "0.39", features = ["bundled"] }
sha2 = "0.11"
hex = "0.4"
ulid = { version = "1.2", features = ["std"] }
chrono = { version = "0.4", default-features = false, features = ["std", "clock", "serde"] }
thiserror = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }

[dev-dependencies]
tempfile = "3"
```

**Version verification:**

```
cargo info rusqlite  → 0.39.0 [VERIFIED 2026-04-24]
cargo info sha2      → 0.11.0 [VERIFIED 2026-04-24]
cargo info ulid      → 1.2.1  [VERIFIED 2026-04-24]
cargo info chrono    → 0.4.44 [VERIFIED 2026-04-24]
cargo info tempfile  → 3.27.0 [VERIFIED 2026-04-24]
```

**Bundled SQLite version** (probed locally with `rusqlite = { version = "0.39", features = ["bundled"] }`):

```
SELECT sqlite_version() → 3.51.3   [VERIFIED: /tmp/rusqlite_probe]
```

Bundled build defaults observed: `PRAGMA foreign_keys = 1` (ON), `PRAGMA journal_mode = memory`, `PRAGMA synchronous = 2 (FULL)`. **Do not rely on the bundled ON default**; system-linked SQLite defaults FK OFF, and portability requires explicit `PRAGMA foreign_keys = ON` on every connection open.

### Bundled vs System SQLite (Claude's Discretion)

**Recommendation: `bundled`.**

| Axis | `bundled` | System-linked |
|------|-----------|---------------|
| Binary size | +~1.5 MB (includes sqlite3.c compiled-in) | No overhead |
| Portability | Same SQLite version everywhere | Depends on OS; macOS ships 3.x, Linux distros vary |
| CI/dev machine setup | `cc` required (already present on all dev OSes) | Requires `libsqlite3-dev` or equivalent |
| Feature coverage | Modern SQLite (partial indexes, WAL, `json1` available) | Depends on OS package version |
| Build time | +3-5s first build, cached thereafter | Negligible |

The `bundled` feature implies `modern_sqlite` which enables the full bindings set, so partial indexes and WAL work regardless of host. Binary-size cost is negligible for a local developer runtime (we are not shipping a 50 MB+ EVM RPC client; the runtime binary is already ~30 MB debug).

## Architecture Patterns

### System Architecture Diagram

```
┌──────────────────────────────────────────────────────────────────────┐
│                     MCP Client (agent)                               │
└───────────────────────┬──────────────────────────────────────────────┘
                        │ JSON-RPC 2.0 over stdio
                        ▼
┌──────────────────────────────────────────────────────────────────────┐
│ executor-mcp::main.rs                                                │
│   config::load() → Config (now includes [state])                     │
│   logging::init(&cfg) → stderr-only tracing                          │
│   ExecutorServer::new(cfg.state)?        ─── opens StateStore here   │
│   .serve(stdio()).await                                              │
└───────────────────────┬──────────────────────────────────────────────┘
                        │
                        ▼
┌──────────────────────────────────────────────────────────────────────┐
│ ExecutorServer                                                       │
│   { tool_router, prompt_router, state: Arc<Mutex<StateStore>> }      │
│                                                                      │
│   ┌─ #[tool_handler] ──┬─ #[prompt_handler] ─┬─ hand-written resources ┐
│   │ tools.rs           │ prompts.rs          │ resources.rs           │
│   │  strategy_register │  write_evm_strategy │  list_resources        │
│   │  strategy_list     │  review_evm_strategy│  list_resource_templates │
│   │  strategy_get  ◄───┼─── now routes       │  read_resource ◄──────  │
│   │  strategy_delete   │    through state    │    parses strategy://   │
│   │  strategy_run_once │                     │    delegates to store   │
│   │  execution_get ◄───┘                     │                         │
│   │  policy_get        (unchanged from Phase 1)                        │
│   │  policy_update                                                     │
│   └────────────────────┴─────────────────────┴─────────────────────────┘
│                        │                                              │
│                        │  state.lock().await → spawn_blocking(...)    │
│                        ▼                                              │
└──────────────────────────────────────────────────────────────────────┘
                        │
                        ▼
┌──────────────────────────────────────────────────────────────────────┐
│ executor-state                                                       │
│   StateStore { conn: Mutex<Connection> }                             │
│   ├─ schema.rs        → init_schema(&mut conn) (pragmas + DDL)       │
│   ├─ strategies.rs    → register / list / get_by_id / get_by_name    │
│   │                      soft_delete / is_deleted                    │
│   ├─ runs.rs          → insert / update_status / get /               │
│   │                      list_for_strategy                           │
│   └─ error.rs         → StateError (thiserror)                       │
└───────────────────────┬──────────────────────────────────────────────┘
                        │
                        ▼
┌──────────────────────────────────────────────────────────────────────┐
│ SQLite file (default ./state.db, or :memory: in tests)               │
│   strategies(id PK, name UNIQUE partial, source, description, tags,  │
│              created_at, deleted_at)                                 │
│   runs(id PK (ULID), strategy_id FK, status, started_at,             │
│        finished_at, error)                                           │
│   idx_strategies_name_active (partial WHERE deleted_at IS NULL)      │
│   idx_runs_strategy_id                                               │
│   idx_strategies_deleted_at                                          │
└──────────────────────────────────────────────────────────────────────┘
```

### Recommended Project Structure

```
crates/executor-state/
├── Cargo.toml
├── src/
│   ├── lib.rs          # re-exports StateStore, StateError, StateConfig-adjacent types
│   ├── error.rs        # thiserror enum
│   ├── schema.rs       # SQL constants + init_schema() pragma + DDL
│   ├── store.rs        # StateStore::open; holds Mutex<Connection>
│   ├── strategies.rs   # impl StrategyRepo for StateStore
│   └── runs.rs         # impl RunRepo for StateStore (Phase 2 = base methods only)
└── tests/
    ├── strategy_roundtrip.rs
    ├── partial_index_behaviour.rs
    └── run_base_model.rs

crates/executor-mcp/src/
├── config.rs           # +StateConfig (Option<path>)
├── server.rs           # ExecutorServer { state: Arc<Mutex<StateStore>>, ... }
├── tools.rs            # 5 tools route to StateStore; 3 stay Phase-N placeholders
├── resources.rs        # read_resource parses strategy://{id}; delegates to store
└── errors.rs           # +storage_not_found(-32014), +name_conflict(-32015), +storage_error(-32016)

crates/executor-core/src/schema/
├── strategy.rs         # +StrategyGetInput (untagged enum, XOR)
│                        # +StrategyRegisterResponse, StrategyListResponse,
│                        #  StrategyGetResponse, StrategyDeleteResponse
└── execution.rs        # +ExecutionGetResponse, RunStatus enum
```

### Pattern 1: Open + Pragma + Idempotent Migration

**What:** `StateStore::open(path)` runs pragmas and `CREATE TABLE IF NOT EXISTS` in a single `execute_batch` call before returning.

**When to use:** Every boot. Pragmas are connection-scoped in SQLite — missing `PRAGMA foreign_keys = ON` per open means FK constraints are silently ignored.

**Example:**
```rust
// Source: verified pattern, extrapolated from rusqlite 0.39 probe + SQLite docs.
use rusqlite::Connection;
use std::path::Path;

pub const SCHEMA_SQL: &str = include_str!("schema.sql");

pub fn open_store(path: &Path) -> Result<Connection, StateError> {
    let conn = Connection::open(path)
        .map_err(|e| StateError::Storage(format!("open {}: {e}", path.display())))?;

    // Order matters: pragmas BEFORE any DDL so FK enforcement applies to CREATE TABLE.
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;",
    )?;

    // Idempotent migration — safe on every startup.
    conn.execute_batch(SCHEMA_SQL)?;
    Ok(conn)
}
```

Where `schema.sql` is:
```sql
-- Source: derived from CONTEXT D-04 verbatim.
CREATE TABLE IF NOT EXISTS strategies (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    source      TEXT NOT NULL,
    description TEXT,
    tags        TEXT,              -- JSON-encoded array; nullable
    created_at  TEXT NOT NULL,     -- RFC3339 UTC
    deleted_at  TEXT               -- RFC3339 UTC; NULL = active
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_strategies_name_active
    ON strategies(name) WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_strategies_deleted_at
    ON strategies(deleted_at);

CREATE TABLE IF NOT EXISTS runs (
    id           TEXT PRIMARY KEY,                   -- ULID
    strategy_id  TEXT NOT NULL REFERENCES strategies(id),
    status       TEXT NOT NULL,                      -- snake_case RunStatus
    started_at   TEXT NOT NULL,
    finished_at  TEXT,
    error        TEXT
);

CREATE INDEX IF NOT EXISTS idx_runs_strategy_id ON runs(strategy_id);
```

**Verified:** `/tmp/rusqlite_probe` ran `execute_batch` with exactly this multi-statement shape (CREATE TABLE + CREATE INDEX + INSERTS) successfully.

### Pattern 2: Async Handler → spawn_blocking → sync rusqlite

**What:** Tool methods are `async`. rusqlite `Connection` is sync + `!Sync`. Use `tokio::task::spawn_blocking` around the lock+query block so we don't block tokio's worker threads on disk I/O.

**When to use:** Every tool handler that touches `StateStore`.

**Example:**
```rust
// Source: pattern extrapolated from rmcp 1.5 counter_stdio example
// (https://github.com/modelcontextprotocol/rust-sdk/blob/main/examples/servers/src/common/counter.rs)
// combined with tokio bridging-with-sync-code guidance
// (https://tokio.rs/tokio/topics/bridging).

// ExecutorServer struct gains one field:
#[derive(Clone)]
pub struct ExecutorServer {
    pub(crate) tool_router: ToolRouter<Self>,
    pub(crate) prompt_router: PromptRouter<Self>,
    pub(crate) state: Arc<tokio::sync::Mutex<StateStore>>,
}

// Inside tools.rs:
async fn strategy_register(
    &self,
    Parameters(input): Parameters<StrategyRegisterInput>,
) -> Result<CallToolResult, McpError> {
    // 1. Handler-side validation (D-09) — cheap, synchronous, fail fast.
    validate_register(&input).map_err(invalid_params)?;

    // 2. Hand off blocking DB work to a blocking pool thread.
    let state = self.state.clone();
    let result = tokio::task::spawn_blocking(move || {
        // blocking_lock() inside spawn_blocking is the sanctioned tokio pattern
        // because spawn_blocking runs on a thread allowed to block.
        let store = state.blocking_lock();
        store.register(&input.name, &input.source, input.metadata.as_ref())
    })
    .await
    .map_err(|e| storage_error(format!("spawn_blocking join: {e}")))??;

    // 3. Serialize into CallToolResult.
    let body = serde_json::to_string(&result).unwrap_or_default();
    Ok(CallToolResult::success(vec![Content::text(body)]))
}
```

**Notes:**
- `tokio::sync::Mutex::blocking_lock()` is intended for use **inside** `spawn_blocking`; it panics if called from async context. [CITED: tokio Mutex docs](https://docs.rs/tokio/latest/tokio/sync/struct.Mutex.html).
- An alternative — using `.lock().await` then calling sync rusqlite inside — also works but blocks the tokio scheduler thread for the duration of the query. For Phase 2 small queries (<1 ms) it is *probably fine*, but `spawn_blocking` is the correct pattern and future-proof for larger strategies/list calls.
- Do **not** hold the lock across `await` points. Acquire, query, release — all inside the blocking closure.

### Pattern 3: Content-Addressed Register (idempotent)

**What:** Compute `strategy_id = hex(sha256(source))` deterministically; `INSERT ... ON CONFLICT(id) DO NOTHING` then check whether a row existed.

**When to use:** Every `strategy_register` call.

**Example:**
```rust
// Source: adapted from SQLite upsert docs (https://www.sqlite.org/lang_upsert.html)
// and CONTEXT D-01 content-addressing rule.
use sha2::{Digest, Sha256};

pub fn hash_source(source: &str) -> String {
    let mut h = Sha256::new();
    h.update(source.as_bytes());
    hex::encode(h.finalize())
}

pub fn register(
    conn: &Connection,
    name: &str,
    source: &str,
    description: Option<&str>,
    tags: Option<&[String]>,
) -> Result<RegisterOutcome, StateError> {
    let id = hash_source(source);

    // 1. Fast path: row with same id already exists (same source) → idempotent.
    if let Some(existing) = fetch_by_id(conn, &id)? {
        return Ok(RegisterOutcome::AlreadyExists(existing));
    }

    // 2. Pre-check: does an ACTIVE row with this name exist for a different id?
    //    If so, we'd hit the partial unique index — turn it into a typed conflict
    //    with richer fields (existing_strategy_id etc.) rather than a raw FK error.
    if let Some(active) = fetch_active_by_name(conn, name)? {
        // Defensive: this can only happen when the existing row's id != our id,
        // since id match already returned above.
        return Err(StateError::NameConflict {
            attempted_name: name.to_string(),
            existing_strategy_id: active.id,
            existing_source_hash: active.id_clone_or_copy,
            existing_created_at: active.created_at,
        });
    }

    // 3. Insert. Because partial unique index depends on deleted_at IS NULL,
    //    another connection COULD race and insert same name between step 2 and step 3.
    //    With D-03d single Mutex<Connection>, that race cannot happen in v1 —
    //    but document this assumption (Pitfall: future connection pool introduces race).
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

### Pattern 4: Untagged Enum for XOR Input

**What:** `strategy_get(strategy_id | name)` via `#[serde(untagged)]` enum with two variants.

**When to use:** Any tool input where exactly one of N fields must be present.

**Example:**
```rust
// Source: verified pattern (/tmp/schemars_probe).
// Note: schemars 1.2 emits anyOf, not oneOf, but serde runtime rejects
// payloads matching both variants, so XOR holds behaviorally.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
pub enum StrategyGetInput {
    ById { strategy_id: String },
    ByName { name: String },
}
```

Generated schema (probed):
```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "StrategyGetInput",
  "anyOf": [
    { "type": "object", "properties": { "strategy_id": {"type":"string"} }, "additionalProperties": false, "required": ["strategy_id"] },
    { "type": "object", "properties": { "name": {"type":"string"} },        "additionalProperties": false, "required": ["name"] }
  ]
}
```

**If strict `oneOf` is required** (some MCP clients' schema validators differ): override with `#[schemars(schema_with = "strategy_get_input_schema")]` and hand-write a schema function emitting `"oneOf"`. Decision for the planner: **accept `anyOf`** unless downstream tooling complains — the server-side serde behavior already enforces XOR.

### Anti-Patterns to Avoid

- **Skipping `PRAGMA foreign_keys = ON` on open** — bundled build has it ON by default, but system SQLite does NOT. Without it, `runs.strategy_id REFERENCES strategies(id)` is silently ignored. Always set it explicitly.
- **Calling `:memory:` + `PRAGMA journal_mode = WAL`** — `:memory:` databases reject WAL silently; the pragma query returns `memory` instead. Don't assert `journal_mode == 'wal'` in `:memory:` tests.
- **Holding the `tokio::sync::Mutex` guard across an `await`** — use `spawn_blocking` OR acquire-query-release synchronously. Holding across await stalls other tasks.
- **Mocking `StateStore` for tests** — D-08 explicitly forbids this. `:memory:` is fast enough and exercises real SQL.
- **Deriving `serde::Deserialize` on `StrategyRegisterInput` without size validation** — schema-level `maxLength` is not enforced by serde; the handler MUST re-check.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| SQL migration runner | Hand-parsed `.sql` file iteration | `rusqlite::Connection::execute_batch(SCHEMA_SQL)` | Built-in; supports multi-statement DDL in one call. Probe-verified. |
| SHA-256 | Roll-your-own Merkle-Damgård | `sha2::Sha256` | Pure Rust, FIPS 180-4 compliant, zero unsafe. |
| ULID | Custom timestamp + random generation | `ulid::Ulid::new()` / `ulid::Generator` | Monotonic-within-millisecond, 26-char Crockford alphabet, serde/ord/display all implemented. |
| RFC3339 formatting | `format!("{:04}-{:02}-...", ...)` | `chrono::DateTime::<Utc>::to_rfc3339_opts(SecondsFormat::Secs, true)` | Handles leap seconds, timezone Z suffix, fractional seconds. |
| Hex encoding | Byte→char table | `hex::encode(&[u8])` | Default lowercase. |
| Connection pooling (v1) | r2d2 setup | **Nothing — single `Mutex<Connection>`** (D-03d) | Defer until there's measurable contention. |
| Temp DB for tests | Roll your own `/tmp/xxx.db` | `tempfile::NamedTempFile` | Auto-cleanup, unique path, handles concurrent test runs. |
| JSON Schema `oneOf` for XOR input | Hand-write `impl JsonSchema` | `#[serde(untagged)]` enum | serde enforces XOR; schema comes free (as `anyOf`). |
| CLI `--config=PATH` parsing | Split by `=` yourself | Either `clap` or the `strip_prefix("--config=")` one-liner in `/01-REVIEW.md` IN-01 | Known Phase 1 bug; planner may opt to fix as part of `[state]` config extension. |

**Key insight:** Phase 2 is almost entirely glue — every primitive (hash, ID, timestamp, storage) has a well-maintained crate. The one place custom code matters is the **pragma+migration sequence at open time** and the **handler-side validation re-check**; everything else should be a three-line call into a library.

## Runtime State Inventory

| Category | Items Found | Action Required |
|----------|-------------|------------------|
| Stored data | **None** — Phase 2 **creates** the state.db for the first time. No pre-existing rows anywhere. | N/A (fresh schema) |
| Live service config | **None** — no running services depend on this yet. (Phase 1 server writes no state.) | N/A |
| OS-registered state | **None** — no scheduled tasks, launchd plists, systemd units. Project is a stdio binary invoked by MCP clients. | N/A |
| Secrets/env vars | `EXECUTOR_CONFIG` env var (Phase 1) — no rename. Phase 2 adds no new env vars. | N/A |
| Build artifacts | Phase 1 binary `target/debug/executor-mcp` — Phase 2 rebuild picks up the new `executor-state` dep automatically. No stale egg-info / reinstall concern. | N/A — standard `cargo build` |

**This is a greenfield storage phase.** Everything Phase 2 creates is net-new; there is no old data/config/binary to migrate. The section is intentionally short.

## Common Pitfalls

### Pitfall 1: `PRAGMA foreign_keys` is per-connection, not per-database

**What goes wrong:** Open a fresh connection, skip `PRAGMA foreign_keys = ON`, insert a `run` row with a bogus `strategy_id` — SQLite accepts it silently. Later, `SELECT` joining strategies returns no rows and the agent can't explain the dangling reference.

**Why it happens:** SQLite stores the pragma state in the `sqlite3*` struct, not in the file. System-linked SQLite defaults OFF.

**How to avoid:** Put `PRAGMA foreign_keys = ON` in `StateStore::open` unconditionally, execute before any schema DDL. Probe confirmed bundled SQLite 3.51.3 **already defaults to ON**, but we do not rely on this.

**Warning signs:** Integration test "insert run with nonexistent strategy_id" does NOT fail. If that test doesn't exist, add it.

### Pitfall 2: Partial unique index + multiple NULLs

**What goes wrong:** Assuming `CREATE UNIQUE INDEX ... ON (name) WHERE deleted_at IS NULL` treats all NULLs as distinct. For **partial** indexes, SQLite only applies the uniqueness constraint to rows satisfying the WHERE clause — so multiple rows where `deleted_at IS NOT NULL` can share any name, and among rows where `deleted_at IS NULL` exactly one can have each name.

**Why it happens:** This is actually the desired behavior for D-01c, but the wording is confusing and developers often expect standard unique-index-with-NULLs semantics (where unique indexes allow multiple NULLs too). [CITED: https://www.sqlite.org/partialindex.html]

**How to avoid:** Write a test (listed in D-08a: `soft_deleted_name_can_be_reused`) that inserts "arb" active, soft-deletes it, inserts "arb" with new source — must succeed; then inserts a second active "arb" — must fail.

**Verified:** `/tmp/rusqlite_probe` ran this exact sequence and observed: (1) second active insert fails, (2) after UPDATE to mark deleted, re-insert active succeeds, (3) additional "deleted" rows with same name can coexist.

### Pitfall 3: `:memory:` rejects WAL silently

**What goes wrong:** Test uses `StateStore::open(Path::new(":memory:"))`, which works — but `PRAGMA journal_mode = WAL` returns `memory` instead of `wal`. If your open() asserts the returned mode, tests fail. If it doesn't, you get false confidence that WAL is active.

**Why it happens:** WAL requires disk-based journaling.

**How to avoid:** Have `init_schema()` accept the result of the pragma and log a warning (via tracing) if the actual mode differs from requested — but **do not fail**. In tests, use `tempfile::NamedTempFile` when you want real WAL behavior; use `:memory:` when you don't.

**Verified:** `/tmp/rusqlite_probe` tests: `:memory:` → `journal_mode=memory` after requesting WAL; `NamedTempFile` → `journal_mode=wal` after requesting WAL.

### Pitfall 4: `rusqlite::Connection` is `Send` but NOT `Sync`

**What goes wrong:** Wrapping `Connection` in `Arc<Connection>` directly and passing across threads compiles initially but `Connection`'s methods take `&self` while mutating internal state, so concurrent reference from two tokio tasks UB (in practice panics or worse).

**Why it happens:** SQLite connections have mutable state per-connection (prepared statements, transaction context). rusqlite models this as `!Sync`.

**How to avoid:** Always wrap in `Mutex<Connection>`. Never take `&Connection` from multiple tasks. Probed: `assert_send::<Connection>()` compiles; `assert_sync::<Connection>()` does not.

**Warning signs:** "`Connection` cannot be shared between threads safely" compiler error, or (worse) it compiles and `cargo test` random-fails only with `--test-threads=4`.

### Pitfall 5: WAL sidecar files leak in tempdir cleanup

**What goes wrong:** `tempfile::NamedTempFile::new()` gives `/tmp/xxxxx` and its `Drop` removes that file — but WAL mode creates `/tmp/xxxxx-wal` and `/tmp/xxxxx-shm` sidecars that aren't tracked. When the test process exits, they remain.

**Why it happens:** `NamedTempFile` tracks one inode; SQLite creates siblings.

**How to avoid:** Use `tempfile::tempdir()` for tests that open WAL DBs — it removes the whole directory including sidecars. Or, for tests that don't care about crash durability, prefer `:memory:` + `synchronous=OFF`.

### Pitfall 6: ULID monotonicity breaks across threads

**What goes wrong:** Two runs inserted within the same millisecond from different threads each call `Ulid::new()` — no shared state, so the ULIDs are not guaranteed to sort in insertion order.

**Why it happens:** `Ulid::new()` uses a fresh random suffix each call. Monotonicity within a millisecond requires a **shared `Generator` instance** that tracks the last-emitted random component and increments on collision.

**How to avoid:** Phase 2 inserts runs from exactly one code path (Phase 3's future `strategy_run_once`) and v1 holds a single `Mutex<Connection>` so there's no concurrency. But: for the deterministic run-ordering test (`run_roundtrip_insert_get_update_status`), seed a `ulid::Generator` with a fixed datetime and call `generate_from_datetime` twice — this gives deterministic, strictly-increasing ULIDs.

**Verified:** Probe — `Generator::generate_from_datetime(t)` called twice with same `t` produced strictly increasing ULIDs (`01KPZJQ2EATYPY7CS9X7PYS0D4` < `...D5`).

### Pitfall 7: `schemars` `#[serde(untagged)]` → `anyOf` not `oneOf`

**What goes wrong:** D-01d / CONTEXT D-07b says "Input 스키마는 `oneOf`로". If the planner expects strict `oneOf` on the wire, schemars 1.2 won't give it — the output is `anyOf`. Some MCP clients' schema validators are strict about the distinction.

**Why it happens:** schemars emits `anyOf` for untagged enums because two variants *could* both validate structurally if they had no `additionalProperties: false`. With `deny_unknown_fields` the variants become disjoint and `anyOf` behaves like `oneOf` in practice — but the keyword is still `anyOf`.

**How to avoid:** Either (a) accept `anyOf` as good enough (serde runtime enforces XOR; see probe), or (b) override with `#[schemars(schema_with = "fn that emits oneOf")]`. **Recommendation: (a)** unless the planner has evidence of a client rejecting `anyOf`.

**Verified:** `/tmp/schemars_probe` — untagged enum → `anyOf`, but `{"strategy_id":"x","name":"y"}` rejected at deserialize time with "data did not match any variant of untagged enum".

### Pitfall 8: 256 KiB byte limit vs UTF-8 character count

**What goes wrong:** D-09 says `source` max is "256 KiB". Counting `source.chars().count()` or `source.len()` gives different numbers: `.len()` is bytes (correct for KiB), `.chars().count()` is scalar count. Mix them up and you either accept too-big sources (agent sends 500 KiB of ASCII) or reject fine ones (agent sends 50 KiB of CJK characters stored as 3-byte UTF-8 sequences).

**Why it happens:** Rust string API has both.

**How to avoid:** Use `source.len() > 262144` for the byte check. For `name` (max 128 characters), use `name.chars().count() > 128` to match the spec. Document the difference in the error message.

### Pitfall 9: Registering a deleted strategy's source re-activates conflict logic

**What goes wrong:** Strategy "arb" soft-deleted. Agent re-registers identical source. `hash_source(source)` produces the same id — so the existing (deleted!) row's id matches. The idempotent fast path returns the deleted row as if it were live.

**Why it happens:** D-01a hash covers source only. The fast-path check (fetch by id) doesn't look at `deleted_at`.

**How to avoid:** Phase 2 register semantics per D-01b are narrow — **the same source is idempotent regardless of `deleted_at`**. This is correct: the strategy's content identity is the hash. But the response should include `deleted_at` in the "already_exists" payload so the agent notices. If agents want to "undelete", they should explicitly call a future `strategy_undelete` tool (deferred). For Phase 2: return the existing row as-is with `already_exists: true` and `deleted_at` set — this is unambiguous.

Alternative interpretation (also defensible): reject register-of-deleted with a typed error suggesting "soft-delete the deleted row's name slot is free; use a new name or call strategy_undelete". But this conflicts with "same source is always idempotent". **Decision: follow D-01b literally — return the existing row including `deleted_at`.**

## Code Examples

### Example 1: `hash_source` — content-addressed id

```rust
// Source: verified (/tmp/sha2_probe).
use sha2::{Digest, Sha256};

pub fn hash_source(source: &str) -> String {
    let mut h = Sha256::new();
    h.update(source.as_bytes());
    hex::encode(h.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn known_vectors() {
        assert_eq!(
            hash_source(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hash_source("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(hash_source("// noop").len(), 64);
    }
}
```

### Example 2: `StateError` + MCP mapping

```rust
// Source: extends Phase 1 error taxonomy (executor-mcp/src/errors.rs) with
// CONTEXT D-06a codes.

// crates/executor-state/src/error.rs
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
        existing_source_hash: String,  // same as existing_strategy_id in content-addressed model
        existing_created_at: String,
    },

    #[error("input validation failed: {0}")]
    InvalidInput(String),
}

impl From<rusqlite::Error> for StateError {
    fn from(e: rusqlite::Error) -> Self { StateError::Storage(e.to_string()) }
}

// crates/executor-mcp/src/errors.rs additions:
use rmcp::{ErrorData as McpError, model::ErrorCode};

pub const STORAGE_NOT_FOUND: ErrorCode = ErrorCode(-32014);
pub const STORAGE_NAME_CONFLICT: ErrorCode = ErrorCode(-32015);
pub const STORAGE_ERROR: ErrorCode = ErrorCode(-32016);

pub fn map_state_error(e: StateError) -> McpError {
    match e {
        StateError::NotFound(what) => McpError::new(
            STORAGE_NOT_FOUND,
            format!("not found: {what}"),
            Some(serde_json::json!({ "code": "not_found", "resource": what })),
        ),
        StateError::NameConflict { attempted_name, existing_strategy_id, existing_created_at, .. } => {
            McpError::new(
                STORAGE_NAME_CONFLICT,
                format!(
                    "strategy name '{attempted_name}' already used by strategy_id={existing_strategy_id} \
                     (created {existing_created_at}); soft-delete that strategy to reuse the name, or choose a different name"
                ),
                Some(serde_json::json!({
                    "code": "name_conflict",
                    "attempted_name": attempted_name,
                    "existing_strategy_id": existing_strategy_id,
                    "existing_created_at": existing_created_at,
                })),
            )
        }
        StateError::InvalidInput(msg) => invalid_params(msg),
        StateError::Storage(msg) => McpError::new(
            STORAGE_ERROR,
            format!("storage error: {msg}"),
            Some(serde_json::json!({ "code": "storage_error" })),
        ),
    }
}

pub fn invalid_params(msg: impl Into<String>) -> McpError {
    McpError::new(
        ErrorCode(-32602),  // JSON-RPC 2.0 standard
        msg.into(),
        Some(serde_json::json!({ "code": "invalid_params" })),
    )
}
```

### Example 3: Input validation (D-09)

```rust
// Source: derived directly from CONTEXT D-09.
const MAX_SOURCE_BYTES: usize = 256 * 1024;            // 262144
const MAX_NAME_CHARS: usize = 128;
const MAX_DESCRIPTION_CHARS: usize = 4096;
const MAX_TAGS: usize = 16;
const MAX_TAG_CHARS: usize = 64;

pub fn validate_register(input: &StrategyRegisterInput) -> Result<(), String> {
    // source
    if input.source.is_empty() {
        return Err("source is empty (must be >= 1 byte UTF-8 text)".into());
    }
    if input.source.len() > MAX_SOURCE_BYTES {
        return Err(format!(
            "source size {} exceeds {}",
            input.source.len(), MAX_SOURCE_BYTES
        ));
    }

    // name
    if input.name.trim().is_empty() {
        return Err("name is empty or whitespace-only".into());
    }
    if input.name.chars().count() > MAX_NAME_CHARS {
        return Err(format!(
            "name length {} chars exceeds {}",
            input.name.chars().count(), MAX_NAME_CHARS
        ));
    }

    // metadata.description / tags live under input.metadata (serde_json::Value).
    // Pull them defensively — unknown fields are ignored (forward compat).
    if let Some(meta) = input.metadata.as_ref().and_then(|v| v.as_object()) {
        if let Some(desc) = meta.get("description").and_then(|v| v.as_str()) {
            if desc.chars().count() > MAX_DESCRIPTION_CHARS {
                return Err(format!(
                    "description length {} exceeds {}",
                    desc.chars().count(), MAX_DESCRIPTION_CHARS
                ));
            }
        }
        if let Some(tags) = meta.get("tags").and_then(|v| v.as_array()) {
            if tags.len() > MAX_TAGS {
                return Err(format!("tags.length {} exceeds {}", tags.len(), MAX_TAGS));
            }
            for (i, t) in tags.iter().enumerate() {
                let s = t.as_str().ok_or_else(|| format!("tags[{i}] is not a string"))?;
                if s.trim().is_empty() {
                    return Err(format!("tags[{i}] is empty or whitespace-only"));
                }
                if s.chars().count() > MAX_TAG_CHARS {
                    return Err(format!(
                        "tags[{i}] length {} exceeds {}",
                        s.chars().count(), MAX_TAG_CHARS
                    ));
                }
            }
        }
    }
    Ok(())
}
```

### Example 4: Resource read for `strategy://{id}`

```rust
// Source: extends Phase 1 resources.rs (current code always returns -32002).
// Pattern derived from rmcp 1.5 ReadResourceResult + ResourceContents docs.
use rmcp::model::{ReadResourceResult, ResourceContents};

pub(crate) async fn read_resource_impl(
    request: ReadResourceRequestParams,
    _ctx: RequestContext<RoleServer>,
    state: Arc<tokio::sync::Mutex<StateStore>>,
) -> Result<ReadResourceResult, McpError> {
    // Parse strategy://{id}. Deliberately do NOT support execution:// or journal://
    // in Phase 2 (Phase 3/6 add those).
    let uri = &request.uri;
    let Some(id) = uri.strip_prefix("strategy://") else {
        return Err(McpError::resource_not_found(
            format!("unsupported resource URI: {uri}"),
            Some(serde_json::json!({ "uri": uri, "phase": 2 })),
        ));
    };
    // Sanity check id shape — 64 hex chars.
    if id.len() != 64 || !id.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)) {
        return Err(McpError::resource_not_found(
            format!("malformed strategy id: {id}"),
            Some(serde_json::json!({ "uri": uri, "phase": 2 })),
        ));
    }

    let id = id.to_string();
    let row = tokio::task::spawn_blocking(move || {
        state.blocking_lock().get_by_id(&id)
    })
    .await
    .map_err(|e| map_state_error(StateError::Storage(format!("spawn_blocking join: {e}"))))??;

    match row {
        None => Err(McpError::resource_not_found(
            format!("strategy {uri} not found"),
            Some(serde_json::json!({ "uri": uri })),
        )),
        Some(s) => {
            let body = serde_json::to_string(&s).map_err(|e| map_state_error(
                StateError::Storage(format!("serialize strategy: {e}"))
            ))?;
            Ok(ReadResourceResult {
                contents: vec![ResourceContents::text(body, uri).with_mime_type("application/json")],
            })
        }
    }
}
```

### Example 5: Test helper — `:memory:` `StateStore`

```rust
// Source: extrapolated from Phase 1 tests/common/mod.rs; tempfile crate patterns.
use std::path::Path;

pub fn fresh_memory_store() -> Result<StateStore, StateError> {
    StateStore::open(Path::new(":memory:"))
}

pub fn seed_strategies(store: &StateStore, n: usize) -> Vec<String> {
    (0..n)
        .map(|i| {
            let source = format!("// strategy {i}\n");
            let name = format!("s{i}");
            store.register(&name, &source, None).unwrap().strategy_id()
        })
        .collect()
}
```

### Example 6: Run status enum (all seven values)

```rust
// Source: CONTEXT D-05 verbatim. Declared in executor-core so both
// executor-state (for validation) and executor-mcp (for schema export) can use it.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Canceled,
    SimulationDenied,
    PolicyDenied,
}

impl RunStatus {
    /// Phase 2 can only emit the first four. Guard against accidental use
    /// of future-reserved values (D-05c).
    pub fn phase2_emittable(self) -> bool {
        matches!(self, Self::Queued | Self::Running | Self::Succeeded | Self::Failed)
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `rusqlite 0.31.x` (pre-2024) required `cache` feature opt-in for prepared-statement cache | `rusqlite 0.39` has `cache` in default features | 2024-25 | No action — default `[dependencies] rusqlite = "0.39"` gives cache. |
| `sha2` used `digest` 0.9 macros | `sha2 0.11` uses `digest 0.10`+ with `Digest` trait method style | 2024 | Code uses `Sha256::new().update(x).finalize()` — probe-verified. |
| `ulid 0.4` had synchronous-only API | `ulid 1.x` added `Generator` for monotonicity | 2023-24 | Use `Generator::generate_from_datetime` for test determinism. |
| `chrono` v0.4.x "oldtime" default feature was a DoS concern | 0.4.44 still includes oldtime but is well-maintained | 2025 | Optional: `default-features = false, features = ["std", "clock", "serde"]` for smaller dep graph. |

**Deprecated/outdated (do not use):**
- `rusqlite`'s `bundled-full` feature — pulls `chrono`, `serde_json`, `time`, `uuid`, `url` as rusqlite features, conflicting with workspace dep management. Enable only specific features (or none — our approach) and let workspace deps handle json/chrono independently.
- `tokio-rusqlite` — not needed with `spawn_blocking`; introduces a second thread per connection.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | Agent-facing MCP tooling accepts `anyOf` and treats it equivalently to `oneOf` for XOR inputs | §Pattern 4 (Untagged Enum for XOR Input) | Claude Desktop / MCP Inspector may validate strictly. Mitigation: planner adds a test asserting `anyOf` appears in `StrategyGetInput.json` golden; if validation fails during manual testing, swap to `schema_with` override. [ASSUMED] |
| A2 | `rusqlite::Connection` with `bundled` feature works identically on Linux, macOS, and Windows for the patterns we use (WAL, partial index, FK) | §Pattern 1 (Open + Pragma + Migration) | Windows tempfile WAL behavior may differ (sidecar file locking). Mitigation: `tempfile::tempdir()` usage pattern sidesteps; CI runs on all three OS — add one WAL-smoke test. [ASSUMED] |
| A3 | Phase 2 does not need connection pooling or multi-threaded access | §DB File & Migrations (D-03d) | If a future `strategy_run_once` Phase 3 implementation starts many runs concurrently, the single `Mutex<Connection>` serializes DB access. v2 moves to pool. [CITED: CONTEXT D-03d — explicit user decision] |
| A4 | The "same source re-register returns the existing deleted row" interpretation of D-01b is correct | §Pitfall 9 | Alternative: reject with typed error pointing at soft-deleted row. Decide with user in discuss phase or during plan. [ASSUMED based on D-01b wording "same source ⇒ idempotent" being unconditional] |
| A5 | `-32014 / -32015 / -32016` are unused by rmcp internals and agents | §Code Example 2 (StateError mapping) | rmcp 1.5 `resource_not_found = -32002`, `unimplemented = -32010` (our choice). Numbers in `-32014..` are in the JSON-RPC server-defined range (`-32000..-32099`) and unused by rmcp. Planner should verify no collision with rmcp's own codes by grepping `src/model/error.rs`. [ASSUMED — quick verification step in plan] |
| A6 | 256 KiB = 262144 bytes (binary KiB), not 256000 bytes (decimal KB) | §Code Example 3 (validation) | Off-by-2% in accepted/rejected source sizes. CONTEXT D-09 says "256 KiB (262144 bytes)" so this is explicit, but double-check during implementation. [CITED: CONTEXT D-09] |
| A7 | `tags` JSON-encoded as a single TEXT column is sufficient (no JSON1 usage) | §Schema Shape | If a future phase wants `WHERE tags CONTAINS 'arb'`, requires JSON1 extension (available in bundled SQLite 3.51.3). Phase 2 doesn't need it. [CITED: CONTEXT D-04a / D-04d — no tag indexing in v1] |

## Open Questions

1. **`#[schemars(schema_with = ...)]` for strict `oneOf`?**
   - What we know: schemars 1.2 emits `anyOf` for untagged enums (probed).
   - What's unclear: does Claude Desktop's MCP schema validator treat `anyOf` with mutually exclusive required fields the same as `oneOf`? No easy way to test without a client.
   - Recommendation: ship `anyOf`; if manual testing reveals rejection, add schema override in a follow-up.

2. **Should Phase 2 also fix the Phase 1 `--config=PATH` parsing bug (review IN-01)?**
   - Phase 2 touches `config.rs` to add the `[state]` section. The fix is a single-line `strip_prefix("--config=")` addition.
   - Recommendation: fix it in Phase 2 (touch-once principle), cite the Phase 1 review finding in the commit message. Planner decides scope.

3. **Name conflict error code = -32015 definitively?**
   - `-32015` is in the JSON-RPC server-defined range, not reserved by rmcp. Any value `-32000..-32099` is valid as long as it doesn't collide.
   - Recommendation: planner greps rmcp source for any `ErrorCode(-3201X)` references; if clean, lock in `-32014/-32015/-32016`.

4. **Does `StrategyRegisterInput` still use `metadata: Option<serde_json::Value>` or does Phase 2 split it into `description`/`tags` top-level fields?**
   - Phase 1 shipped `metadata: Option<Value>`; Phase 2 CONTEXT D-09 references `description`/`tags` as if they were separate fields. Two interpretations:
     - (A) Keep `metadata: Value`, extract `description`/`tags` from its interior (validation in Example 3 above does this).
     - (B) Migrate `StrategyRegisterInput` to `{ name, source, description: Option<String>, tags: Option<Vec<String>> }`.
   - (B) is cleaner agent-facing but breaks the Phase 1 golden `StrategyRegisterInput.json` (and `schema_contract_round_trip` test payload uses `{"name":"x","source":"// noop"}` which still works).
   - Recommendation: **go with (B)** — the agent contract is cleaner and schemars produces stronger validation (`maxLength` on `description`, `maxItems` on `tags` array). Regenerate the golden in the same commit. Planner decides.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|-------------|-----------|---------|----------|
| `cargo` / Rust toolchain | Everything | ✓ | 1.94.0 (workspace edition = 2024) | — |
| `cc` C compiler | `rusqlite` `bundled` feature builds sqlite3.c | ✓ (cargo found `cc` during probe) | Apple clang (macOS) | — |
| SQLite (runtime) | `bundled` feature compiles it in | ✓ | 3.51.3 (bundled, probed) | Use system-linked if binary size matters (not needed for v1) |
| Internet for crate fetches | First build | ✓ (`cargo info` succeeded) | — | Use cargo offline cache |

**No blocking missing dependencies.** All deps are either already in workspace (`tokio`, `tracing`, `serde`, `serde_json`, `anyhow`, `thiserror`, `toml`) or crate-registry-available (`rusqlite`, `sha2`, `hex`, `ulid`, `chrono`, `tempfile`).

## Validation Architecture

### Test Framework

| Property | Value |
|----------|-------|
| Framework | Rust built-in `#[test]` + `#[tokio::test]` (no additional framework) |
| Config file | None — driven by `[package]` / `[dev-dependencies]` per crate |
| Quick run command | `cargo test -p executor-state --lib` (repository unit tests only; ~100 ms with `:memory:`) |
| Full suite command | `cargo test --workspace` (all crates, integration tests included; Phase 1 baseline: ~20 tests, Phase 2 adds ~12) |
| Lint gate | `cargo clippy --workspace --all-targets -- -D warnings` (unchanged from Phase 1) |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| STR-01 | Register new strategy creates row with `already_exists: false` | integration (stdio) | `cargo test -p executor-mcp --test stdio_handshake strategy_register_creates_row` | ❌ Wave 0 |
| STR-01 | Register same source twice ⇒ idempotent, `already_exists: true` | integration (stdio) | `strategy_register_idempotent_same_source` | ❌ Wave 0 |
| STR-01 | Register different source same name ⇒ -32015 conflict | integration (stdio) | `strategy_register_conflict_same_name_different_source` | ❌ Wave 0 |
| STR-01 | Oversized source (>256 KiB) ⇒ -32602 invalid_params | integration (stdio) | `strategy_register_rejects_oversized_source` | ❌ Wave 0 |
| STR-01 | Empty/whitespace name ⇒ -32602 invalid_params | integration (stdio) | `strategy_register_rejects_empty_name` | ❌ Wave 0 |
| STR-02 | `strategy_list` default excludes `source` payload | integration (stdio) | `strategy_list_excludes_source_payload` | ❌ Wave 0 |
| STR-02 | `strategy_list` default filters `deleted_at IS NOT NULL` | integration (stdio) | `strategy_list_filters_deleted_by_default` | ❌ Wave 0 |
| STR-02 | `strategy_get(strategy_id)` returns row including source | integration (stdio) | `strategy_get_by_id_returns_source` | ❌ Wave 0 |
| STR-02 | `strategy_get(name)` only returns active rows | integration (stdio) | `strategy_get_by_name_only_returns_active` | ❌ Wave 0 |
| STR-02 | `strategy_delete` is soft + idempotent (same deleted_at on repeat) | integration (stdio) | `strategy_delete_is_soft_and_idempotent` | ❌ Wave 0 |
| STR-02 | Soft-deleted name can be reused by new source | integration (stdio) | `soft_deleted_name_can_be_reused` | ❌ Wave 0 |
| STR-02 | `strategy://{id}` resource read returns full strategy JSON | integration (stdio) | `resource_read_strategy_uri_returns_body` | ❌ Wave 0 |
| STJ-01 | State persists across server restart (file-backed DB) | integration (spawn twice against tempdir DB) | `strategies_persist_across_restart` | ❌ Wave 0 |
| STJ-01 | Pragmas applied on open (WAL, FK ON, synchronous NORMAL) | repository unit (`executor-state`) | `cargo test -p executor-state pragmas_applied_on_open` | ❌ Wave 0 |
| STJ-01 | Schema is idempotent (second open doesn't fail) | repository unit | `schema_is_idempotent` | ❌ Wave 0 |
| STJ-01 | Partial unique index blocks duplicate active name | repository unit | `partial_unique_index_blocks_duplicate_active_name` | ❌ Wave 0 |
| STJ-02 | Run insert → get → update_status round-trip | repository unit | `run_roundtrip_insert_get_update_status` | ❌ Wave 0 |
| STJ-02 | `execution_get` on missing run ⇒ -32014 not_found | integration (stdio) | `execution_get_returns_not_found_when_empty` | ❌ Wave 0 |
| STJ-02 | Run status enum includes all 7 future-reserved values (schema golden) | snapshot | `cargo test -p executor-core --test schema_snapshots` (RunStatus golden) | ❌ Wave 0 (new golden) |
| STJ-02 | RunRepo::insert rejects phase-reserved status values | repository unit | `run_status_rejects_phase_reserved` | ❌ Wave 0 |

### Sampling Rate

- **Per task commit:** `cargo test -p executor-state --lib && cargo test -p executor-mcp --test stdio_handshake` (only affected suites; <5 s).
- **Per wave merge:** `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`.
- **Phase gate:** Full suite green + schema goldens current (`cargo test -p executor-core --test schema_snapshots` no-update) before `/gsd-verify-work`.

### Wave 0 Gaps

- [ ] `crates/executor-state/tests/schema.rs` — covers pragmas, idempotent schema, partial index.
- [ ] `crates/executor-state/tests/strategies.rs` — repository-level CRUD.
- [ ] `crates/executor-state/tests/runs.rs` — run base-model roundtrip.
- [ ] `crates/executor-mcp/tests/stdio_handshake.rs` (extended) — ~12 new `#[tokio::test]` fns per D-08a.
- [ ] `crates/executor-core/tests/schemas/RunStatus.json` — new golden asserting all 7 variants.
- [ ] `crates/executor-core/tests/schemas/StrategyGetInput.json` — new golden for untagged enum XOR shape.
- [ ] `crates/executor-core/tests/schemas/StrategyRegisterInput.json` — **regenerate** if Q4 (description/tags split) is decided in favor.
- [ ] `crates/executor-core/tests/schemas/StrategyListResponse.json`, `StrategyGetResponse.json`, `StrategyRegisterResponse.json`, `StrategyDeleteResponse.json`, `ExecutionGetResponse.json` — new response-shape goldens so future phases can't drift.
- [ ] No framework install needed — built-in `#[test]` is used (same as Phase 1).

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no (v1 is local runtime, trusted client) | — |
| V3 Session Management | no | — |
| V4 Access Control | no (single operator, stdio trust boundary) | — |
| V5 Input Validation | **yes** | `schemars` agent-facing schema + handler-side re-check (D-09); byte/char length limits; JSON type enforcement via serde |
| V6 Cryptography | **yes (content-addressing only)** | `sha2::Sha256` (FIPS 180-4); no key management in Phase 2 (Phase 6 handles signer) |
| V7 Error Handling & Logging | yes | Structured `thiserror` → MCP error code; tracing on stderr only (D-05 inherited from Phase 1) |
| V8 Data Protection | yes | Strategy source stored as TEXT in local DB; no encryption at rest in v1 (local hot-wallet runtime assumption). Deferred: SQLCipher variant as `bundled-sqlcipher` feature. |

### Known Threat Patterns for Rust + SQLite + MCP stdio

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| SQL injection via strategy name/source containing `'; DROP TABLE` | Tampering | Parameterized queries via `rusqlite::params![..]` / `named_params!{..}` (probed). NEVER string-concat SQL. |
| Resource URI traversal (`strategy://../../../etc/passwd`) | Info disclosure | Strict regex on id: `^[0-9a-f]{64}$`; fail-closed on unexpected scheme (`strategy://...` only; execution/journal not accepted in Phase 2). Example 4 above implements both checks. |
| Oversized payload DoS (10 MB source swamps disk) | DoS | D-09 byte-level check before INSERT. `-32602 invalid_params` with the byte count in the message. |
| Too-many-tags / too-many-* DoS | DoS | D-09 hard caps on tag count and tag length. |
| Malformed JSON in `tags` column poisons reads | Tampering | `serde_json::from_str` on read; if invalid, surface `StateError::Storage` — does not crash server. Probed: serde_json round-trips cleanly through rusqlite TEXT. |
| Race on partial unique index (two clients register same name simultaneously) | Integrity | **Not applicable in v1** — single `Mutex<Connection>` serializes (D-03d). v2 with pool needs `BEGIN IMMEDIATE` transactions. |
| Data exfil via `strategy_list` returning 1000 strategies' full source | Info disclosure / DoS | D-07a — `strategy_list` never returns source. |
| Resource exhaustion via many soft-deleted rows | DoS | Accept — soft-deleted rows are the observability log (journal integrity, D-02 rationale). v2 may add a compaction tool. |

## Sources

### Primary (HIGH confidence — probed or official)

- **rusqlite 0.39.0** — probed locally at `/tmp/rusqlite_probe`. Verified: Connection Send (not Sync), execute_batch multi-statement, params! and named_params! macros, transaction commit/rollback, partial unique index on `WHERE deleted_at IS NULL`, serde_json interop via TEXT columns, bundled SQLite = 3.51.3, default pragmas (FK ON in bundled, WAL rejected by `:memory:`). [VERIFIED]
- **schemars 1.2.1** — probed at `/tmp/schemars_probe`. Verified: `#[serde(untagged)]` → `anyOf` with `additionalProperties: false`; both-fields payload rejected at deserialize. [VERIFIED]
- **sha2 0.11.0** — probed at `/tmp/sha2_probe`. Verified: `Sha256::new().update(x).finalize()` API, FIPS 180-4 `abc` vector, 64-char lowercase hex output via `hex::encode`. [VERIFIED]
- **ulid 1.2.1** — probed at `/tmp/ulid_probe`. Verified: 26-char Crockford Base32 uppercase, `Ulid::new()`, `Generator::generate_from_datetime` monotonic, parse roundtrip, alphabet exclusions. [VERIFIED]
- **rmcp 1.5** — `counter_stdio` example + `common/counter.rs` confirm `Arc<tokio::sync::Mutex<T>>` pattern for shared state. [CITED: https://github.com/modelcontextprotocol/rust-sdk/blob/main/examples/servers/src/common/counter.rs]
- **rmcp 1.5 ReadResourceResult / ResourceContents** — [CITED: https://docs.rs/rmcp/1.5.0/rmcp/model/struct.ReadResourceResult.html] and [CITED: https://docs.rs/rmcp/1.5.0/rmcp/model/enum.ResourceContents.html] — `ResourceContents::text(content, uri).with_mime_type("application/json")` is the canonical builder.
- **SQLite partial indexes** — [CITED: https://www.sqlite.org/partialindex.html] — partial WHERE applies uniqueness only to rows satisfying the predicate. Probe confirms this matches D-01c expectations.
- **SQLite WAL** — [CITED: https://www.sqlite.org/wal.html] — `journal_mode=WAL`; `:memory:` cannot use WAL.
- **FIPS 180-4 (SHA-256 spec)** — [CITED: https://csrc.nist.gov/pubs/fips/180-4/final].
- **ULID spec** — [CITED: https://github.com/ulid/spec] — 26-char, Crockford Base32, lexicographic == time order.
- **tokio bridging-with-sync-code** — [CITED: https://tokio.rs/tokio/topics/bridging] — `spawn_blocking` for blocking calls; `tokio::sync::Mutex::blocking_lock` in that context.

### Secondary (MEDIUM confidence — verified via multiple sources)

- **Phase 1 artifacts:** `01-CONTEXT.md`, `01-02-SUMMARY.md`, `01-03-SUMMARY.md`, `01-REVIEW.md` — authoritative for how the existing surface works.
- **Phase 2 CONTEXT.md** — user-locked decisions; the single source of truth for scope/shape.
- **Current code:** `crates/executor-mcp/src/{config,tools,resources,server,errors}.rs` + `crates/executor-state/{src/lib.rs,Cargo.toml}` + `crates/executor-core/src/schema/*` — direct inspection.
- **STACK.md** — crate version baselines (rusqlite 0.39, schemars 1.2, rmcp 1.5).

### Tertiary (LOW confidence — or not verified this pass)

- **tokio-rusqlite docs** — read as reference for what NOT to use. Pattern it documents (per-connection dedicated thread) is inconsistent with D-03d.
- **Claude Desktop / MCP Inspector schema validation behavior on `anyOf` vs `oneOf`** — not verified; flagged as Assumption A1 above.

## Metadata

**Confidence breakdown:**
- Standard stack: **HIGH** — every version probed locally; features enumerated via `cargo metadata`.
- Architecture: **HIGH** — rmcp 1.5 handler pattern confirmed against official example; Phase 1 code inspected directly.
- Pitfalls: **HIGH** — 5 of 9 pitfalls reproduced in probe; remaining 4 cited against SQLite/rmcp/serde docs.
- Security domain: **MEDIUM** — STRIDE mapping is sound; threat model small because v1 runtime is single-operator local.
- XOR input schema: **MEDIUM** — serde runtime behavior verified; MCP client validation of `anyOf` assumed.

**Research date:** 2026-04-24
**Valid until:** 2026-05-24 (30 days — stable Rust ecosystem, no fast-moving deps)

---

*Phase: 02-strategy-state-and-journal*
*Researched: 2026-04-24*
