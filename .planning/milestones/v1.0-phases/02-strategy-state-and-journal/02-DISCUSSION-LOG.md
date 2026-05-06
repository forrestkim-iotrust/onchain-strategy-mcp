# Phase 2: Strategy State and Journal - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-04-24
**Phase:** 02-strategy-state-and-journal
**Rounds:** 2 (initial + reflection/revision)
**Areas discussed with user:** B (Strategy identity & mutability), 1 (name semantics after reflection), 4 (run status enum after reflection), 5 (input validation after reflection)
**Areas defaulted (with user's explicit go-ahead "defaults from principles"):** A (DB file & migrations), C (Metadata shape), E (Delete semantics), F (ID format for runs), 2 (re-register diff visibility), 3 (list vs get response shape), WAL rationale

---

## Round 1 — Initial discussion

### B. Strategy identity & mutability (user-resolved in round 1)

| Option | Description | Selected |
|--------|-------------|----------|
| B-1 Overwrite | `name` unique, 같은 name 재호출은 source/metadata 덮어쓰기. strategy_id 유지 | |
| B-2 Reject duplicate | `name` unique, 같은 name 재호출은 에러. agent는 delete 후 register | |
| B-3 Append version | 매 register는 새 strategy_id 생성. name은 label, history 자동 보존 | |
| B-4 Content-addressed | `id = hash(source)`. 같은 source는 영원히 같은 id. 불변. name은 비-unique label | ✓ (round 1) |

**User's choice (round 1):** B-4 — "불변이 낫겠어. 4번이 좋을듯. 해시로"

**Initial follow-through (round 1, later partially revised in round 2):**
- 해시 함수: SHA-256 (hex 소문자 64자)
- 해시 범위: source 텍스트만
- 재등록: idempotent, 기존 row 반환
- **name uniqueness: 없음** ← 이 부분은 round 2에서 뒤집힘 (agent UX 후퇴 우려)

### A. DB file location & migrations (defaulted)

| Option | Description | Selected |
|--------|-------------|----------|
| config.toml `[state].path` + cwd-relative default | 명시적 path, `:memory:` 허용, 없으면 `./state.db` | ✓ |
| XDG data dir default | OS-specific conventions | |
| rusqlite_migration crate | Versioned migration system | |
| CREATE TABLE IF NOT EXISTS on boot | Phase 1 "해당 Phase에서 추가" 규칙 일관 | ✓ |

**Default rationale:** Phase 1 config 확장 패턴과 일관. v1 local single-operator.

### C. Metadata shape (defaulted)

| Option | Description | Selected |
|--------|-------------|----------|
| 자유 JSON blob | agent 마음대로 | |
| 최소 구조화: description + tags | 다른 필드는 필요한 Phase에서 ALTER | ✓ |
| 풍부한 구조화: description, tags, chain_ids, owner, created_by 등 | overbuilding 우려 | |

### D. Run status enum (defaulted round 1, revised round 2)

| Option | Description | Selected (round 1) | Selected (round 2) |
|--------|-------------|----|----|
| 4-state minimum: queued / running / succeeded / failed | Phase 2 base만 | ✓ | |
| **7-state full declaration up-front** | queued/running/succeeded/failed + canceled/simulation_denied/policy_denied. Phase 2는 4개만 emit | | ✓ |
| 3-state: started / succeeded / failed | 가장 적게 | | |

### E. Delete semantics (defaulted)

| Option | Description | Selected |
|--------|-------------|----------|
| Hard delete | 행 삭제, runs FK 처리 필요 | |
| Soft delete (deleted_at) | list 기본 필터, 과거 run 참조 무결성 보존 | ✓ |
| Reject if runs exist | 단순하지만 agent UX 제약 | |

### F. ID format for runs (defaulted)

| Option | Description | Selected |
|--------|-------------|----------|
| ULID | 26자 Crockford Base32, 시간순, agent-readable | ✓ |
| UUID v4 | 36자, 완전 랜덤 | |
| Short prefix + random | human-readable prefix | |

---

## Round 2 — Claude의 self-review 후 사용자 확정

**트리거:** 사용자가 "너가 보기에 어때? 뭔가 애매한거 없었어?" 로 reflection 요청. Claude가 5개 지점을 문제 제기, 사용자가 "그렇게 해" 로 Claude 추천을 승인.

### Issue 1: `name`이 비-unique label이면 agent UX가 후퇴한다 (REVISED)

**Claude's analysis:** round 1에서 "strategy는 content-addressed이므로 name은 단순 label, unique할 필요 없다" 라고 결정했는데, agent가 "내 arb_usdc 전략 돌려줘" 식의 자연스러운 접근을 할 수 없음. list → 정렬 → pick 3단계가 매번 필요.

**Options presented:**
- (a) name을 non-deleted row들 사이에서 UNIQUE 강제 (SQLite partial unique index). 같은 name 다른 source = conflict 에러. Soft-delete 후 같은 name 재사용 가능.
- (b) 현재 유지 + `strategy_resolve_name(name)` helper tool.

**User's choice:** (a) — "그렇게 해"

**Impact on CONTEXT.md:**
- D-01b: same-source idempotent 경로 + different-source-same-name conflict 에러 경로 구분
- D-01c: `CREATE UNIQUE INDEX ... WHERE deleted_at IS NULL` partial index 강제
- D-01d: `strategy_get`이 id 또는 name 둘 다 받도록 `oneOf` 스키마
- D-04a: name 컬럼 주석에 unique 명시
- D-04c: partial unique index 추가
- D-07: response에 `existing_name` / `existing_metadata` 추가 (same-source idempotent 경로에서 diff 가시화)
- D-08a: `strategy_register_conflict_same_name_different_source`, `soft_deleted_name_can_be_reused`, `strategy_get_by_name_only_returns_active` 테스트 추가

### Issue 2: Silent same-source re-register가 agent를 혼란시킬 수 있다 (MINOR FIX)

**Claude's analysis:** `already_exists: true`만으로는 "왜 내 새 name이 응답에 없지?" 모멘트를 방지 못함.

**Options presented:** (a) conflict 에러로 반려 vs (b) 현재대로 silent + diff 필드 추가.

**User's choice:** (b) via "그렇게 해" — issue 1의 name-unique 결정 때문에 same-source-different-name 시나리오는 이제 드문 케이스. `existing_name` / `existing_metadata` 필드로 diff를 가시화하는 선에서 해결.

**Impact on CONTEXT.md:** D-07 response shape에 필드 추가.

### Issue 3: `strategy_list`가 source를 포함하는지 불분명 (SPEC GAP FIX)

**Problem:** source는 KB 단위가 될 수 있는데 list 호출마다 반복 전송하면 낭비.

**Resolution:** D-07a에서 list 응답은 source 제외 명시. `strategy_get`만 source 포함 (D-07b).

### Issue 4: `run.status` enum을 점진적으로 확장하면 Phase 1의 "스키마 안정성" 원칙과 어긋난다 (REVISED)

**Claude's analysis:** Phase 2는 4개 값만 선언하고 Phase 5/6에서 canceled/simulation_denied/policy_denied 추가하면 agent-facing schema golden이 세 번 바뀜. Phase 1 CONTEXT가 "스키마는 계약, 변경 비용 크다" 라고 선언했는데 모순.

**Options presented:**
- (a) Phase 2에서 전체 7개 값 미리 선언. Phase 2는 4개만 emit, 나머지는 future-reserved.
- (b) status를 open string으로. type safety 손실.
- (c) 현재대로 점진 확장.

**User's choice:** (a) — "그렇게 해"

**Impact on CONTEXT.md:**
- D-05: 전체 7개 enum 선언, `#[serde(rename_all = "snake_case")]`
- D-05a: Phase 2 코드는 4개 값만 emit (validation 레벨 제약)
- D-05c: future-reserved 3개 값은 Phase 2에서 써서는 안 됨 명시
- D-04b: runs.status 컬럼 주석 업데이트
- D-08a: `run_status_schema_includes_future_variants` 테스트 추가 (계약 증명)

### Issue 5: `strategy_register` 입력 validation 스펙 누락 (NEW SPEC)

**Problem:** 빈 source, 10MB source, 긴 name, 등을 어떻게 처리할지 round 1에서 언급 없음. Phase 2가 "agent 입력을 실제로 저장하는 첫 단계" 라 hygiene 기초가 여기서 확정돼야 함.

**Resolution:** D-09 신설.
- source: non-empty UTF-8, max 256 KiB
- name: non-empty, max 128 Unicode scalars
- description: optional, max 4096자
- tags: max 16개, 각 max 64자
- unknown metadata fields: 무시 (forward-compat)
- validation 실패 시 `-32602 invalid_params` + 구체적 한계값 메시지

**Impact:** D-04a 컬럼 주석에 한도 표시, D-09 전체 신설, D-08a에 `rejects_oversized_source` / `rejects_empty_name` 테스트 추가.

### Minor: WAL + single-Mutex 트레이드오프 (CLARIFICATION)

**Concern:** WAL의 "concurrent read-write" 이점이 단일 뮤텍스 때문에 활용되지 않음.

**Resolution:** D-03c에 명시 — "WAL 목적은 crash durability. concurrency 이점은 v2 connection pool 도입 시." 현재 결정 유지.

### Minor: Legacy config without `[state]` section (NEW D-03e)

**Concern:** Phase 1 config 파일을 그대로 쓰는 기존 사용자가 있을 수 있음.

**Resolution:** D-03e 신설 — `Config::state: Option<StateConfig>`, 없으면 `StateConfig::default() → path = "./state.db"`.

---

## Deferred Ideas (최종)

Captured in CONTEXT.md `<deferred>` section. Key items:
- Metadata mutation tool (v2)
- Strategy versioning by name — v1은 unique name + soft-delete-then-reuse (v2에서 본격 versioning)
- Connection pool (`r2d2_sqlite`) when single-Mutex becomes a bottleneck
- Migration system (`schema_version` table) when schema changes needed
- XDG/OS-specific data dir (v2)
- Binary blob strategies / import-export bundle (v2)
- source 크기 상한 tunable화 (실사용 후 재평가)

## Claude's Discretion (not prompted, final)

- `executor-state` 내부 모듈 분할
- rusqlite feature flags (bundled vs system)
- sha2 / ulid / chrono|time crate 선택
- Config struct 확장 세부 구조 (D-03e 요구사항 충족 범위)
- MCP error code 신규 할당 정확한 수치 (D-06a 가이드 기반)
- `strategy_get`의 `oneOf` input 스키마 표현 방식
