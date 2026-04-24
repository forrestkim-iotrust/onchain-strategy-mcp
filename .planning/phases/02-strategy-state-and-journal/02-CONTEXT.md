# Phase 2: Strategy State and Journal - Context

**Gathered:** 2026-04-24
**Status:** Ready for planning

<domain>
## Phase Boundary

런타임이 전략(strategy)과 전략 run을 **로컬 SQLite에 영속화**한다.

이 Phase가 다루는 것:
- `executor-state` crate 내부의 SQLite schema + repository layer 구현
- MCP tool 중 `strategy_register` / `strategy_list` / `strategy_get` / `strategy_delete`를 placeholder에서 실제 동작으로 전환
- Run(전략 실행 인스턴스) 레코드의 **base model** — run_id, strategy_id, started_at, status. 실제 실행 로직(JS 실행, EVM read/action, simulation, policy, signing, journal 상세)은 **Phase 3 이후**에서 채운다.

이 Phase가 다루지 않는 것 (각 Phase 책임):
- JS 샌드박스 실행, `ctx` API, source-read 기록 → Phase 3 (STR-03~05, STJ-03~04)
- EVM 상호작용, action validation → Phase 4
- Simulation / policy decision journal → Phase 5 (STJ-05)
- Signer / broadcast / receipt / tx journal → Phase 6 (STJ-06~07)

Phase 2가 통과하면: agent가 JS 소스를 register하면 파일이 영속화되고, 서버 재시작 후에도 `strategy_list`·`strategy_get`으로 조회되고, `strategy_delete`로 지울 수 있고, run 레코드가 생성될 **테이블 공간**이 존재한다.

</domain>

<decisions>
## Implementation Decisions

### Strategy identity & mutability (핵심 결정)

- **D-01:** Strategy는 **content-addressed, immutable**이다. `strategy_id = hex(sha256(source))` — 소스 텍스트 바이트의 SHA-256 해시(소문자 hex).
  - 이유: PROJECT.md의 "모든 실행은 기록으로 남는다" 제약과 journal integrity에 부합. 같은 소스는 어디서 언제 등록되든 같은 id. agent는 "수정"이 아니라 "새 버전 등록" 모델.
- **D-01a:** 해시 범위는 **source 텍스트만**. `name`/`metadata`는 id에 영향 주지 않는다. 메타만 다른 동일 소스는 **같은 strategy** (idempotent).
- **D-01b:** `strategy_register(name, source, metadata)` 재호출 의미론:
  - 같은 source → 같은 strategy_id. 이미 존재하면 **첫 등록 시의 name/metadata 유지**, 새로 넘어온 name/metadata는 **덮어쓰지 않는다** (immutability). 응답은 기존 row 그대로 반환 (agent가 "already registered" 를 명시적으로 구분할 수 있도록 응답 필드에 `already_exists: bool` 포함).
  - 에이전트가 "메타를 바꾸고 싶다" → v1은 지원하지 않음. 필요 시 v2에서 별도 `strategy_metadata_update` tool (현재 out-of-scope).
- **D-01c:** `name`은 **unique가 아닌 label**. 서로 다른 소스가 같은 name을 가질 수 있다. agent는 strategy를 **항상 id로 참조**한다. `strategy_list`는 `(name, id, created_at)`을 보여주고 agent가 필요한 것을 고른다.
- **D-01d:** `strategy_run_once`, `execution_get`, resource `strategy://{id}` 등 모든 참조 경로는 id(hex)만 받는다. name 조회는 `strategy_list` + 클라이언트 필터.

### Delete semantics

- **D-02:** **Soft delete**. `strategies.deleted_at TIMESTAMP NULL` 컬럼. `strategy_delete(id)`는 이 컬럼만 세팅.
  - 이유: content-addressed와 자연스럽게 맞고, 과거 run이 `strategy_id`를 참조해도 dangling reference가 발생하지 않음. Journal/audit integrity 보존.
- **D-02a:** `strategy_list`는 기본값 `deleted_at IS NULL`만 반환. 스키마에 `include_deleted: bool` 인자 추가 (default false).
- **D-02b:** `strategy_get(id)`는 deleted 여부와 관계없이 반환하되, 응답 필드에 `deleted_at: Option<...>` 포함 (과거 run 추적을 위해 복구 가능한 조회 경로 필요).
- **D-02c:** 이미 deleted된 strategy로 `strategy_run_once` 호출은 v1에서 **거부**한다 (`-32010` 유사한 typed error; Phase 3 run 로직 붙일 때 정확한 코드 확정).

### DB file & migrations

- **D-03:** `rusqlite 0.39` 동기 API. 이유: AGENTS.md / STACK.md 확정. sqlx나 sea-orm 도입 금지.
- **D-03a:** Config 스키마 확장 (Phase 1의 "해당 Phase에서 field 추가" 규칙):
  ```toml
  [state]
  path = "./state.db"   # default — relative to process cwd
  ```
  - 생략 시 `./state.db`. 절대경로 허용. `:memory:` 허용 (테스트용).
  - XDG 같은 OS-특정 경로 규칙은 v1에서 도입 안 함. 로컬 런타임 가정 + agent가 작업 디렉토리를 통제한다는 전제.
- **D-03b:** Migration은 **전용 crate 없이** `CREATE TABLE IF NOT EXISTS` 방식. 스키마 변경은 v2에서 필요하면 `schema_version` 테이블 도입.
- **D-03c:** 부팅 시 1회: connection 연결 → `PRAGMA journal_mode = WAL;`, `PRAGMA synchronous = NORMAL;`, `PRAGMA foreign_keys = ON;` 적용 → `CREATE TABLE IF NOT EXISTS` 실행. 실패 시 server startup error.
- **D-03d:** Connection wrapping: **`Mutex<rusqlite::Connection>` 단일** (v1 single-writer 가정과 일치, 풀링 금지 — 복잡도 회피). Phase 2는 read/write 모두 동일 잠금을 거친다. 성능 이슈가 현실화되면 v2에서 재평가.

### Schema shape (v1 minimal)

- **D-04:** Phase 2에서 도입하는 테이블은 **2개**: `strategies`, `runs`. Journal 세부 테이블(source_reads, action_records, policy_decisions, tx_reports)은 **해당 Phase에서 추가** (Phase 3/5/6). Phase 2는 base만.
- **D-04a:** `strategies` 컬럼:
  - `id TEXT PRIMARY KEY` — hex SHA-256 (64자)
  - `name TEXT NOT NULL` — label, not unique
  - `source TEXT NOT NULL` — JS 원본 텍스트
  - `description TEXT NULL` — agent가 준 메타 (자유형, 짧음)
  - `tags TEXT NULL` — JSON array 직렬화 (`["arb","usdc"]` 같이), 인덱싱은 v1에서 없음
  - `created_at TEXT NOT NULL` — RFC3339 UTC
  - `deleted_at TEXT NULL` — soft delete 타임스탬프
- **D-04b:** `runs` 컬럼 (base만):
  - `id TEXT PRIMARY KEY` — hex ULID 또는 UUIDv7 (D-05 참조)
  - `strategy_id TEXT NOT NULL REFERENCES strategies(id)` — FK, cascade 금지 (soft delete와 일관)
  - `status TEXT NOT NULL` — `queued | running | succeeded | failed` 중 하나
  - `started_at TEXT NOT NULL` — RFC3339 UTC, insert 시점
  - `finished_at TEXT NULL` — 종료 시점 (Phase 3+)
  - `error TEXT NULL` — 실패 메시지 (Phase 3+)
- **D-04c:** **Indexes (최소):** `CREATE INDEX idx_runs_strategy_id ON runs(strategy_id);`, `CREATE INDEX idx_strategies_deleted_at ON strategies(deleted_at);`. 추가 인덱스는 실제 쿼리 패턴 확인 후.
- **D-04d:** Metadata 스키마는 의도적으로 작다 (description + tags만). 추가 구조 필드(chain_ids, owner, created_by 등)는 **필요해지는 Phase에서 ALTER** — Phase 1의 "overbuilding 회피" 규칙 일관.

### Run status & lifecycle (base only)

- **D-05:** Phase 2 status enum은 4개: `queued`, `running`, `succeeded`, `failed`. `canceled` / `simulation_denied` / `policy_denied`는 **해당 로직이 붙는 Phase에서 추가** (Phase 5 policy, Phase 6 signer). Serde `#[serde(rename_all = "snake_case")]`.
- **D-05a:** Phase 2는 run 행을 **INSERT하는 경로를 만들지 않는다** — 실제 JS 실행은 Phase 3. 그러나 테이블과 repository 메서드 (`RunRepo::insert`, `RunRepo::update_status`, `RunRepo::get`)는 Phase 2에서 완성. 통합 테스트는 "수동 insert → get → status update → get" 라운드트립만 커버.
- **D-05b:** `run_id`는 **ULID**(26자, Crockford Base32). 이유: 시간순 정렬, resource URI(`execution://{id}`)에서 UUID보다 짧고 agent-readable. `ulid` crate 사용. (strategy_id는 D-01로 해시 고정, 별개).

### Repository layer shape

- **D-06:** `executor-state`는 **하나의 pub struct `StateStore`** 를 노출하고, 내부적으로 `StrategyRepo`, `RunRepo` 두 개의 non-public 섹션으로 메서드를 분리해 관리한다. trait 추상화는 v1에서 하지 않는다 (in-memory mock 필요성이 아직 없음 — 통합 테스트는 `:memory:` SQLite로 충분).
  - `StateStore::open(path: &Path) -> Result<Self>` — Connection 열고 pragmas + migrations.
  - Strategy 메서드: `register`, `list`, `get`, `soft_delete`, `is_deleted`.
  - Run 메서드: `insert_run`, `update_run_status`, `get_run`, `list_runs_for_strategy` (Phase 3+에서 확장).
- **D-06a:** 에러 타입은 `executor-state::StateError` (thiserror). `executor-mcp`에서는 MCP error code로 매핑 — not_found(-32014 같은 새 코드 또는 resource_not_found 재사용), conflict(-32015), storage(-32016) 같은 typed mapping은 planning에서 최종 확정.

### MCP tool transition (placeholder → real)

- **D-07:** `strategy_register(name, source, metadata)` 응답 모양:
  ```json
  {
    "strategy_id": "<hex sha256>",
    "name": "...",
    "created_at": "...",
    "already_exists": true|false
  }
  ```
- **D-07a:** `strategy_list(include_deleted?: bool = false)` 응답:
  ```json
  {
    "strategies": [
      { "strategy_id": "...", "name": "...", "description": "...", "tags": [...], "created_at": "...", "deleted_at": null }
    ]
  }
  ```
- **D-07b:** `strategy_get(strategy_id)` 응답: strategies row 전체 + `source` 포함. `resource strategy://{id}` 도 Phase 2에서 실제 데이터 반환 (현재는 -32002 not_found placeholder).
- **D-07c:** `strategy_delete(strategy_id)` 응답: `{ "strategy_id": "...", "deleted_at": "..." }`. 이미 삭제된 경우 그대로 같은 응답 (idempotent).
- **D-07d:** `execution_get` 은 Phase 2에서 **진짜 DB를 친다** — run row를 반환. run이 없으면 not_found 에러. 단, Phase 2 동안은 run insert 경로가 없으므로 agent가 `execution_get` 을 유의미하게 호출하지는 못함 (Phase 3에서 완성).

### Testing

- **D-08:** Integration test는 `:memory:` SQLite + 실제 `StateStore`로 작성. mocking 금지 — v1 규모에서 실DB가 빠르고 진실성이 높다.
- **D-08a:** Phase 1의 `tests/stdio_handshake.rs` 패턴에 추가: `strategy_register_idempotency`, `strategy_list_filters_deleted`, `strategy_get_returns_source`, `strategy_delete_is_soft`, `run_roundtrip_insert_get_update`.
- **D-08b:** `StateStore` 단위 테스트는 `crates/executor-state/tests/`에 별도로 — MCP 레이어 거치지 않는 repository-level 계약.

### Claude's Discretion

- `executor-state` 내부 모듈 분할 (`schema.rs` / `strategies.rs` / `runs.rs` / `error.rs` vs 단일 `lib.rs`).
- `rusqlite` feature flags (`bundled`, `serde_json` 등) 선택. Bundled sqlite 포함 여부는 planning에서 binary size vs. portability trade-off으로.
- SHA-256 crate 선택 (`sha2` 유력 — 이미 STACK.md "Supporting" 목록에 있음).
- ULID crate (`ulid` 유력, feature flag 없이 stable).
- Datetime 직렬화 (`chrono` vs `time` crate) — 한 번만 고르면 됨. RFC3339 UTC 고정.
- Config loader 확장: `[state]` 섹션을 Phase 1 `Config` struct에 어떻게 붙일지 (옵션으로 optional field, 생략 시 default).
- MCP error code 신규 할당 정확한 수치 (예: storage_error = -32016).

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Project planning
- `.planning/PROJECT.md` — Constraints §"Observability" ("모든 run은 journal로 남아야 한다") — soft delete 결정 근거
- `.planning/REQUIREMENTS.md` — Phase 2 requirements (STR-01, STR-02, STJ-01, STJ-02)
- `.planning/ROADMAP.md` §"Phase 2: Strategy State and Journal" — 3-plan 분할, success criteria
- `AGENTS.md` §"Technology Stack" — `rusqlite` 확정, §"Hard Boundaries"

### Prior phase artifacts
- `.planning/phases/01-mcp-runtime-surface/01-CONTEXT.md` — Phase 1 스키마·config 패턴 (`[state]` 섹션 확장 방식 일관성)
- `.planning/phases/01-mcp-runtime-surface/01-01-SUMMARY.md` — `executor-state` crate stub 현황
- `.planning/phases/01-mcp-runtime-surface/01-02-SUMMARY.md` — `Config` loader 구조 (확장 포인트)
- `.planning/phases/01-mcp-runtime-surface/01-03-SUMMARY.md` — MCP tool placeholder → real 전환 지점
- `.planning/phases/01-mcp-runtime-surface/01-REVIEW.md` — alert: `config.rs` `--config=PATH` 파싱 이슈 (Phase 2 config 확장 시 같이 고칠지 판단)

### Research briefs
- `.planning/research/STACK.md` — `rusqlite 0.39`, `sha2`, `ulid`, `tempfile` 위치
- `.planning/research/ARCHITECTURE.md` — state 계층 책임, crate 경계, resource URI 모양

### External specs
- SQLite WAL mode: https://www.sqlite.org/wal.html — D-03c의 pragma 근거
- SHA-256 (FIPS 180-4): https://csrc.nist.gov/pubs/fips/180-4/final — hash 스펙
- ULID spec: https://github.com/ulid/spec — run_id 포맷
- MCP tool error codes: https://modelcontextprotocol.io/specification/2025-11-25/server/tools — neu storage/not_found 에러 코드 할당 기준

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `crates/executor-state/` — Phase 1에서 빈 crate로 존재. `Cargo.toml`에 `rusqlite` 의존 추가 + `src/lib.rs` 채우면 됨.
- `crates/executor-mcp/src/config.rs` — Phase 1 Config loader. `[state]` 섹션 추가 위치.
- `crates/executor-mcp/src/tools.rs` — 8개 tool dispatch. `strategy_register` / `strategy_list` / `strategy_get` / `strategy_delete` / `execution_get` 의 `unimplemented_phase_hint` 호출을 실제 `StateStore` 호출로 교체.
- `crates/executor-mcp/src/resources.rs` — `strategy://{id}` URI template. `read_resource` 가 현재 항상 `resource_not_found` 반환 → Phase 2에서 실제 조회 분기.
- `crates/executor-mcp/tests/common/mod.rs` + `tests/stdio_handshake.rs` — 통합 테스트 harness. Phase 2 테스트는 이 위에 추가.
- `crates/executor-core/src/schema/strategy.rs`, `.../execution.rs` — 입력 스키마 구조체 (Phase 1에서 정의). 응답 스키마는 Phase 2에서 `executor-core`에 추가하거나 `executor-mcp` local types로.

### Established Patterns
- **Config 확장 패턴:** 각 Phase에서 새 섹션을 `Config` struct에 `Option<Xxx>` 필드로 추가. Phase 1 `[logging]` 전례 따름.
- **Error mapping 패턴:** `executor-mcp::errors::unimplemented_err`가 `-32010`으로 리턴. Phase 2에서 storage/not_found 에러를 같은 모듈에 추가.
- **테스트 패턴:** `spawn_server` + `initialize` + `tools/call` JSON-RPC roundtrip. Phase 2 신규 테스트는 register → list → get → delete 시퀀스.
- **스키마 golden:** `crates/executor-core/tests/schemas/*.json`에 input/output 골든. Phase 2에서 응답 스키마가 확정되면 golden 추가 (환경변수 `UPDATE_SCHEMAS=1`).

### Integration Points
- `ExecutorServer::new(config)` — Phase 2는 config의 `[state].path`를 받아 `StateStore::open`을 여기서 호출하고 server 필드에 보관.
- `#[tool_handler]` impl 내부 tool 메서드 → `self.state_store.xxx()` 호출 경로.
- `#[prompt_handler]` 는 Phase 2에서 건드리지 않는다 (Phase 7에서 본문 확정).

</code_context>

<specifics>
## Specific Ideas

- `strategy_id` 표기: hex 소문자 64자 고정. 출력 예: `"3a1f...c7"`. agent prompt에서 보기 안 좋다는 피드백 나오면 v2에서 짧은 alias (예: 첫 16자) 도입.
- `run_id` 표기: ULID 26자, Crockford Base32 대문자 (예: `01HGK...`). Resource URI: `execution://01HGK...`.
- Test fixture: `:memory:` DB에 미리 정해진 source 2-3개 seed한 helper. `common::seed_strategies(&store, n)` 같은 편의 함수.
- `PRAGMA foreign_keys = ON`은 **반드시** 매 connection open 마다 재호출 (SQLite는 connection scoped).
- Soft delete와 FK 일관성: `runs.strategy_id REFERENCES strategies(id)` 는 `ON DELETE RESTRICT` (hard delete 막힘) 또는 `NO ACTION`. cascade는 명시적으로 금지.

</specifics>

<deferred>
## Deferred Ideas

- **Metadata mutation (`strategy_metadata_update` tool)** — v1 immutability 원칙과 상충. 필요하면 v2에서 별도 mutation table로.
- **Strategy versioning by name** — 같은 name으로 여러 id 관리 UX (예: "latest" alias). v1은 agent가 직접 list + select. 패턴이 반복되면 v2에서 helper tool.
- **Connection pool** — `Mutex<Connection>` 병목이 실제로 관찰되면 `r2d2_sqlite` 도입. v1 범위 아님.
- **Migration system** — `schema_version` 테이블 + versioned migrations. 스키마 변경이 생기는 시점 (Phase 3+)에 필요해지면.
- **XDG/OS-specific data dir** — `dirs` crate 기반 기본 경로. v1은 cwd-relative 단순 모델.
- **Binary blob strategies** — v1은 UTF-8 텍스트 JS만. WASM, bytecode 등은 v2.
- **Strategy import/export** — JSON/tar.gz bundle. v1은 register/get + 외부 도구.

</deferred>

---

*Phase: 02-strategy-state-and-journal*
*Context gathered: 2026-04-24*
