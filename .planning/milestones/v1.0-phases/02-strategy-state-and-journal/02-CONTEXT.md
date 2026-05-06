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
- **D-01b:** `strategy_register(name, source, metadata)` 재호출 의미론 — **두 경우를 구분**한다:
  - **같은 source** (payload와 기존 row의 source bytes가 동일) → idempotent. 기존 row 그대로 반환, 응답에 `already_exists: true` + 기존 `name`/`metadata` 포함. 새로 넘어온 name/metadata는 **덮어쓰지 않는다** (immutability). 단 name이 기존과 다르면 agent 혼란 여지가 있으므로 응답에 `existing_name: "..."` 와 `existing_metadata: {...}` 를 함께 담아 diff 가시화.
  - **다른 source가 기존 name과 충돌** (name UNIQUE 제약 위반) → **conflict 에러**로 반려 (`-32015` storage_conflict 류). 에러 payload에 `existing_strategy_id`, `existing_source_hash`, `existing_created_at` 포함 → agent는 "기존 strategy를 쓸지 / 다른 name으로 재시도할지 / 기존을 soft-delete 후 재등록할지" 를 결정 가능.
  - 에이전트가 "metadata만 바꾸고 싶다" → v1 미지원. name unique라 기존 soft-delete 후 재등록이 우회 경로 (id는 바뀜, name은 재사용 가능).
- **D-01c:** `name`은 **non-deleted strategies 사이에서 UNIQUE**. SQLite **partial unique index**로 강제한다:
  ```sql
  CREATE UNIQUE INDEX idx_strategies_name_active
    ON strategies(name) WHERE deleted_at IS NULL;
  ```
  이렇게 하면 "arb_usdc" 를 soft-delete한 뒤 같은 name으로 새 source를 등록하는 것이 가능하다 — 이전 row는 deleted_at이 세팅되어 index에서 빠지므로. Agent UX와 불변성 둘 다 확보.
- **D-01d:** Agent는 strategy를 **id(hex) 또는 name** 둘 다로 조회할 수 있다:
  - `strategy_get(strategy_id=...)` — 정확 id 조회. deleted 포함 조회 가능.
  - `strategy_get(name=...)` — **활성(deleted_at IS NULL) 중에서만** name으로 조회. 여러 개 매칭 불가 (unique). 없으면 not_found. Input 스키마는 `oneOf`로 id 또는 name 중 하나만 받도록 정의.
  - Resource URI `strategy://{id}` 는 id만 받는다 (불변 식별자 규약).
  - `strategy_run_once`, `execution_get`은 Phase 3 이후에서 id만 받되, 내부적으로 MCP 서버가 name→id resolution helper를 제공할지는 해당 Phase에서 결정.

### Delete semantics

- **D-02:** **Soft delete**. `strategies.deleted_at TIMESTAMP NULL` 컬럼. `strategy_delete(id)`는 이 컬럼만 세팅.
  - 이유: content-addressed와 자연스럽게 맞고, 과거 run이 `strategy_id`를 참조해도 dangling reference가 발생하지 않음. Journal/audit integrity 보존.
- **D-02a:** `strategy_list`는 기본값 `deleted_at IS NULL`만 반환. 스키마에 `include_deleted: bool` 인자 추가 (default false).
- **D-02b:** `strategy_get(id)`는 deleted 여부와 관계없이 반환하되, 응답 필드에 `deleted_at: Option<...>` 포함 (과거 run 추적을 위해 복구 가능한 조회 경로 필요). 반면 `strategy_get(name=...)`은 D-01d에 따라 활성 row만 대상.
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
- **D-03c:** 부팅 시 1회: connection 연결 → `PRAGMA journal_mode = WAL;`, `PRAGMA synchronous = NORMAL;`, `PRAGMA foreign_keys = ON;` 적용 → `CREATE TABLE IF NOT EXISTS` + partial unique index 실행. 실패 시 server startup error.
  - WAL 사용 목적은 **crash durability**이다 (write + checkpoint 분리, 프로세스 중단 시 복구 용이). `Mutex<Connection>` 단일 잠금 때문에 WAL의 "concurrent read-write" 이점은 v1에서 거의 소실되지만, durability 이점은 유지된다. 동시성 이점은 v2 connection pool 도입 시 활성화.
- **D-03d:** Connection wrapping: **`Mutex<rusqlite::Connection>` 단일** (v1 single-writer 가정과 일치, 풀링 금지 — 복잡도 회피). Phase 2는 read/write 모두 동일 잠금을 거친다. 성능 이슈가 현실화되면 v2에서 재평가.
- **D-03e:** `Config`에 `[state]` 섹션이 전혀 없을 때도 (Phase 1 이전 config를 그대로 쓰는 기존 사용자 대비) `StateStore::open("./state.db")` 기본값으로 부팅 가능해야 한다. `Config::state`는 `Option<StateConfig>` + `.unwrap_or_default()`.

### Schema shape (v1 minimal)

- **D-04:** Phase 2에서 도입하는 테이블은 **2개**: `strategies`, `runs`. Journal 세부 테이블(source_reads, action_records, policy_decisions, tx_reports)은 **해당 Phase에서 추가** (Phase 3/5/6). Phase 2는 base만.
- **D-04a:** `strategies` 컬럼:
  - `id TEXT PRIMARY KEY` — hex SHA-256 (64자)
  - `name TEXT NOT NULL` — label. **non-deleted 사이에서는 UNIQUE** (D-01c partial index)
  - `source TEXT NOT NULL` — JS 원본 텍스트 (UTF-8, max 256KB — D-09 input validation)
  - `description TEXT NULL` — agent가 준 메타 (자유형, 짧음, max 4096자)
  - `tags TEXT NULL` — JSON array 직렬화 (`["arb","usdc"]` 같이), 인덱싱은 v1에서 없음
  - `created_at TEXT NOT NULL` — RFC3339 UTC
  - `deleted_at TEXT NULL` — soft delete 타임스탬프
- **D-04b:** `runs` 컬럼 (base만):
  - `id TEXT PRIMARY KEY` — ULID (26자 Crockford Base32)
  - `strategy_id TEXT NOT NULL REFERENCES strategies(id)` — FK, cascade 금지 (soft delete와 일관)
  - `status TEXT NOT NULL` — **전체 enum 중 하나** (D-05 참조). Phase 2는 `queued / running / succeeded / failed`만 emit, 나머지 값도 스키마에는 미리 선언됨.
  - `started_at TEXT NOT NULL` — RFC3339 UTC, insert 시점
  - `finished_at TEXT NULL` — 종료 시점 (Phase 3+)
  - `error TEXT NULL` — 실패 메시지 (Phase 3+)
- **D-04c:** **Indexes:**
  - `CREATE UNIQUE INDEX idx_strategies_name_active ON strategies(name) WHERE deleted_at IS NULL;` (D-01c)
  - `CREATE INDEX idx_runs_strategy_id ON runs(strategy_id);`
  - `CREATE INDEX idx_strategies_deleted_at ON strategies(deleted_at);`
  - 추가 인덱스는 실제 쿼리 패턴 확인 후.
- **D-04d:** Metadata 스키마는 의도적으로 작다 (description + tags만). 추가 구조 필드(chain_ids, owner, created_by 등)는 **필요해지는 Phase에서 ALTER** — Phase 1의 "overbuilding 회피" 규칙 일관.

### Run status & lifecycle (agent-facing schema 안정성 우선)

- **D-05:** Run status enum은 **Phase 2에서 전체 7개 값을 미리 선언**한다. 이유: Phase 1 원칙 "스키마는 계약, 변경 비용 크다". Phase 5/6에서 status 값을 추가하면 agent-facing golden + 파싱 로직이 모두 깨지므로 처음부터 전체를 확정.
  ```
  #[derive(Serialize, Deserialize, JsonSchema)]
  #[serde(rename_all = "snake_case")]
  enum RunStatus {
      Queued,
      Running,
      Succeeded,
      Failed,
      Canceled,           // Phase 6 cancel 로직이 활성화
      SimulationDenied,   // Phase 5 simulation 실패
      PolicyDenied,       // Phase 5 policy 거부
  }
  ```
- **D-05a:** Phase 2는 run 행을 **INSERT하는 경로를 만들지 않는다** — 실제 JS 실행은 Phase 3. 그러나 테이블과 repository 메서드 (`RunRepo::insert`, `RunRepo::update_status`, `RunRepo::get`)는 Phase 2에서 완성. 통합 테스트는 "수동 insert → get → status update → get" 라운드트립만 커버. Phase 2 자체 코드가 emit하는 status 값은 **`queued / running / succeeded / failed` 4개로 제한** (validation).
- **D-05b:** `run_id`는 **ULID**(26자, Crockford Base32). 이유: 시간순 정렬, resource URI(`execution://{id}`)에서 UUID보다 짧고 agent-readable. `ulid` crate 사용. (strategy_id는 D-01로 해시 고정, 별개).
- **D-05c:** Phase 2는 status `canceled / simulation_denied / policy_denied`를 **어느 코드 경로에서도 써서는 안 된다** (dead value 이면서 future-reserved). 해당 값이 DB에 나타나는 건 Phase 5/6 로직 도입 후.

### Repository layer shape

- **D-06:** `executor-state`는 **하나의 pub struct `StateStore`** 를 노출하고, 내부적으로 `StrategyRepo`, `RunRepo` 두 개의 non-public 섹션으로 메서드를 분리해 관리한다. trait 추상화는 v1에서 하지 않는다 (in-memory mock 필요성이 아직 없음 — 통합 테스트는 `:memory:` SQLite로 충분).
  - `StateStore::open(path: &Path) -> Result<Self>` — Connection 열고 pragmas + migrations.
  - Strategy 메서드: `register`, `list`, `get_by_id`, `get_by_name`, `soft_delete`, `is_deleted`.
  - Run 메서드: `insert_run`, `update_run_status`, `get_run`, `list_runs_for_strategy` (Phase 3+에서 확장).
- **D-06a:** 에러 타입은 `executor-state::StateError` (thiserror). `executor-mcp`에서는 MCP error code로 매핑 — `not_found(-32014)`, `name_conflict(-32015)`, `storage_error(-32016)` 같은 typed mapping. 정확한 수치는 planning에서 확정.

### MCP tool transition (placeholder → real)

- **D-07:** `strategy_register(name, source, metadata)` 응답 모양 (same-source idempotent 경로):
  ```json
  {
    "strategy_id": "<hex sha256>",
    "name": "<원래 등록 시의 name>",
    "created_at": "<원래 등록 시각>",
    "already_exists": true,
    "existing_name": "<동일 source 기존 row의 name>",
    "existing_metadata": { "description": "...", "tags": [...] }
  }
  ```
  처음 등록일 때: `already_exists: false`, `existing_name` / `existing_metadata`는 생략(또는 null). `name conflict` 경로는 에러로 반환 (D-01b).
- **D-07a:** `strategy_list(include_deleted?: bool = false)` 응답 — **source는 포함하지 않는다** (대용량 JS가 매 list 호출마다 복제되는 것을 방지):
  ```json
  {
    "strategies": [
      {
        "strategy_id": "...",
        "name": "...",
        "description": "...",
        "tags": [...],
        "created_at": "...",
        "deleted_at": null
      }
    ]
  }
  ```
- **D-07b:** `strategy_get(strategy_id=... | name=...)` 응답 — strategies row 전체 + `source` 포함. name 인자를 쓰면 활성만 조회됨 (D-01d). `resource strategy://{id}` 도 Phase 2에서 실제 데이터 반환 (source 포함, 현재는 -32002 not_found placeholder).
- **D-07c:** `strategy_delete(strategy_id)` 응답: `{ "strategy_id": "...", "deleted_at": "..." }`. 이미 삭제된 경우 **동일 `deleted_at`**을 그대로 다시 반환 (idempotent, 에러 아님).
- **D-07d:** `execution_get` 은 Phase 2에서 **진짜 DB를 친다** — run row를 반환. run이 없으면 not_found 에러. 단, Phase 2 동안은 run insert 경로가 없으므로 agent가 `execution_get` 을 유의미하게 호출하지는 못함 (Phase 3에서 완성).

### Input validation (Phase 2에서 확정)

- **D-09:** `strategy_register` 입력 validation을 **스키마 + tool 진입점 양쪽에서** 강제한다 (스키마는 agent에게 알려주는 계약, tool 코드는 실제 방어).
  - `source`: 비어있지 않음 (`len >= 1`), UTF-8 텍스트, **max 256 KiB** (262144 bytes). 초과 시 `-32602 invalid_params` + 메시지에 `source size {N} exceeds 262144`.
  - `name`: 비어있지 않음, **max 128자** (Unicode scalar values). 공백만 있는 name 거부. 허용 문자는 제한하지 않음 (agent가 이름 방식 자유).
  - `description`: optional, max 4096자.
  - `tags`: optional, max 16개, 각 tag max 64자, 공백만 있는 tag 거부. 중복 허용 여부는 입력 그대로 저장 (agent 책임).
  - `metadata` 전체 payload가 agent가 예상 못한 필드를 포함하면 **무시** (forward-compat). 단 타입이 틀리면 거부.
- **D-09a:** `strategy_delete`는 input에 strategy_id만 받는다. UUID/ULID/기타 형식이 아닌 이상한 값은 스키마 레벨에서 (pattern: `^[0-9a-f]{64}$`) 거부.
- **D-09b:** validation 실패 시 agent-facing 메시지는 **어떤 제한이 깨졌는지** 명시 (예: "source exceeds 256 KiB"). 일반적 "invalid input" 금지.

### Testing

- **D-08:** Integration test는 `:memory:` SQLite + 실제 `StateStore`로 작성. mocking 금지 — v1 규모에서 실DB가 빠르고 진실성이 높다.
- **D-08a:** Phase 1의 `tests/stdio_handshake.rs` 패턴에 추가되는 테스트:
  - `strategy_register_idempotent_same_source`
  - `strategy_register_conflict_same_name_different_source`
  - `strategy_register_rejects_oversized_source`
  - `strategy_register_rejects_empty_name`
  - `strategy_list_excludes_source_payload`
  - `strategy_list_filters_deleted_by_default`
  - `strategy_get_by_id_returns_source`
  - `strategy_get_by_name_only_returns_active`
  - `strategy_delete_is_soft_and_idempotent`
  - `soft_deleted_name_can_be_reused`
  - `run_roundtrip_insert_get_update_status`
  - `run_status_schema_includes_future_variants` (D-05 계약 증명: canceled/simulation_denied/policy_denied가 enum에 선언돼 있음)
- **D-08b:** `StateStore` 단위 테스트는 `crates/executor-state/tests/`에 별도로 — MCP 레이어 거치지 않는 repository-level 계약.

### Claude's Discretion

- `executor-state` 내부 모듈 분할 (`schema.rs` / `strategies.rs` / `runs.rs` / `error.rs` vs 단일 `lib.rs`).
- `rusqlite` feature flags (`bundled`, `serde_json` 등) 선택. Bundled sqlite 포함 여부는 planning에서 binary size vs. portability trade-off으로.
- SHA-256 crate 선택 (`sha2` 유력 — 이미 STACK.md "Supporting" 목록에 있음).
- ULID crate (`ulid` 유력, feature flag 없이 stable).
- Datetime 직렬화 (`chrono` vs `time` crate) — 한 번만 고르면 됨. RFC3339 UTC 고정.
- Config loader 확장: `[state]` 섹션을 Phase 1 `Config` struct에 어떻게 붙일지 (D-03e 요구사항 충족하는 선에서).
- MCP error code 신규 할당 정확한 수치 (D-06a 가이드 기반).
- `strategy_get`의 `oneOf` input 스키마 표현 (schemars/serde tag 방식).

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
- SQLite partial indexes: https://www.sqlite.org/partialindex.html — D-01c 근거
- SHA-256 (FIPS 180-4): https://csrc.nist.gov/pubs/fips/180-4/final — hash 스펙
- ULID spec: https://github.com/ulid/spec — run_id 포맷
- MCP tool error codes: https://modelcontextprotocol.io/specification/2025-11-25/server/tools — storage/not_found/conflict 에러 코드 할당 기준

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
- **Error mapping 패턴:** `executor-mcp::errors::unimplemented_err`가 `-32010`으로 리턴. Phase 2에서 storage/not_found/conflict 에러를 같은 모듈에 추가.
- **테스트 패턴:** `spawn_server` + `initialize` + `tools/call` JSON-RPC roundtrip. Phase 2 신규 테스트는 register → list → get → delete 시퀀스.
- **스키마 golden:** `crates/executor-core/tests/schemas/*.json`에 input/output 골든. Phase 2에서 응답 스키마가 확정되면 golden 추가 (환경변수 `UPDATE_SCHEMAS=1`). Run status enum의 전체 7개 값(D-05)이 golden에 기록되어야 future-proof.

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
- `strategy_register` name conflict 에러 메시지 예시: `"strategy name 'arb_usdc' already used by strategy_id=3a1f...c7 (created 2026-04-24T10:00Z); soft-delete that strategy to reuse the name, or choose a different name"`.

</specifics>

<deferred>
## Deferred Ideas

- **Metadata mutation (`strategy_metadata_update` tool)** — v1 immutability 원칙과 상충. 필요하면 v2에서 별도 mutation table로.
- **Strategy versioning by name** — 같은 name으로 여러 id history 관리 UX (예: "latest" alias). v1은 unique name + soft-delete-then-reuse 패턴. 본격적 versioning은 v2.
- **Connection pool** — `Mutex<Connection>` 병목이 실제로 관찰되면 `r2d2_sqlite` 도입. v1 범위 아님.
- **Migration system** — `schema_version` 테이블 + versioned migrations. 스키마 변경이 생기는 시점 (Phase 3+)에 필요해지면.
- **XDG/OS-specific data dir** — `dirs` crate 기반 기본 경로. v1은 cwd-relative 단순 모델.
- **Binary blob strategies** — v1은 UTF-8 텍스트 JS만. WASM, bytecode 등은 v2.
- **Strategy import/export** — JSON/tar.gz bundle. v1은 register/get + 외부 도구.
- **source 크기 256 KiB 상향** — 실제 전략이 제한을 맞으면 config으로 tunable 하게 만들거나 상한 올림. v1 관찰 후 판단.

</deferred>

---

*Phase: 02-strategy-state-and-journal*
*Context gathered: 2026-04-24 (initial + revision pass)*
