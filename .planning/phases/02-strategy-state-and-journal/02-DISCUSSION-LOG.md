# Phase 2: Strategy State and Journal - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-04-24
**Phase:** 02-strategy-state-and-journal
**Areas discussed:** B (Strategy identity & mutability)
**Areas defaulted (no user input requested):** A (DB file & migrations), C (Metadata shape), D (Run status enum), E (Delete semantics), F (ID format for runs)

---

## B. Strategy identity & mutability (user-resolved)

| Option | Description | Selected |
|--------|-------------|----------|
| B-1 Overwrite | `name` unique, 같은 name 재호출은 source/metadata 덮어쓰기. strategy_id 유지 | |
| B-2 Reject duplicate | `name` unique, 같은 name 재호출은 에러. agent는 delete 후 register | |
| B-3 Append version | 매 register는 새 strategy_id 생성. name은 label, history 자동 보존 | |
| B-4 Content-addressed | `id = hash(source)`. 같은 source는 영원히 같은 id. 불변. name은 비-unique label | ✓ |

**User's choice:** B-4 — 불변이 낫다. 해시로.

**Rationale captured from user:** 불변성을 선호. content-addressed 모델로.

**Follow-through decisions made by Claude (no further prompt, all recorded in CONTEXT.md D-01~D-01d):**
- 해시 함수: SHA-256, hex 소문자 64자 (STACK.md의 `sha2` crate 사용 예정)
- 해시 범위: **source 텍스트만**. metadata/name은 id에 영향 X
- 재등록 의미론: 기존 row 그대로 반환 + 응답에 `already_exists: true`. name/metadata는 **덮어쓰지 않는다**
- name uniqueness: 없음. agent는 항상 strategy_id로 참조
- "metadata 수정" use case: v1 미지원. v2 deferred (별도 metadata mutation table)

---

## A. DB file location & migrations (defaulted, user agreed to "defaults from principles")

| Option | Description | Selected |
|--------|-------------|----------|
| config.toml `[state].path` + cwd-relative default | 명시적 path, `:memory:` 허용, 없으면 `./state.db` | ✓ |
| XDG data dir default | OS-specific conventions | |
| rusqlite_migration crate | Versioned migration system | |
| CREATE TABLE IF NOT EXISTS on boot | Phase 1 "해당 Phase에서 추가" 규칙 일관 | ✓ |

**Default rationale:** Phase 1 config 확장 패턴과 일관. v1은 local single-operator runtime이므로 OS data dir까지 고려할 필요 낮음. Migration은 스키마 변경이 실제로 생길 때 도입.

---

## C. Metadata shape (defaulted)

| Option | Description | Selected |
|--------|-------------|----------|
| 자유 JSON blob | agent 마음대로 | |
| 최소 구조화: description + tags | 다른 필드는 필요한 Phase에서 ALTER | ✓ |
| 풍부한 구조화: description, tags, chain_ids, owner, created_by 등 | overbuilding 우려 | |

**Default rationale:** Phase 1의 "필요한 Phase에서 채운다" 규칙. Over-schema 회피.

---

## D. Run status enum (defaulted)

| Option | Description | Selected |
|--------|-------------|----------|
| 4-state minimum: queued / running / succeeded / failed | Phase 2 base만, 나머지는 해당 Phase에서 | ✓ |
| 7+ state including canceled / policy_denied / simulation_denied | Phase 5/6 대비 미리 확보 | |
| 3-state: started / succeeded / failed | 가장 적게 | |

**Default rationale:** "Overbuilding before first run" pitfall 회피. policy_denied/simulation_denied/canceled는 해당 로직이 실제로 붙는 Phase에서 추가하는 것이 타입/journal 일관성에 유리.

---

## E. Delete semantics (defaulted)

| Option | Description | Selected |
|--------|-------------|----------|
| Hard delete | 행 삭제, runs FK 처리 필요 | |
| Soft delete (deleted_at) | list 기본 필터, 과거 run 참조 무결성 보존 | ✓ |
| Reject if runs exist | 단순하지만 agent UX 제약 | |

**Default rationale:** PROJECT.md Constraints §"Observability" — "모든 run은 journal로 남아야 한다." Soft delete가 이 원칙과 content-addressed 모델과 자연스럽게 맞음.

---

## F. ID format for runs (defaulted)

| Option | Description | Selected |
|--------|-------------|----------|
| ULID | 26자 Crockford Base32, 시간순, agent-readable | ✓ |
| UUID v4 | 36자, 완전 랜덤 | |
| Short prefix + random (예: `run_01HGK...`) | human-readable prefix | |

**Default rationale:** resource URI `execution://{id}` 에서 UUID보다 짧고, 시간순 sortable이라 journal/run list에서 유용. `strategy_id`는 D-01로 해시 고정이라 별개.

---

## Claude's Discretion (not prompted)

- `executor-state` 내부 모듈 분할
- rusqlite feature flags (bundled vs system)
- sha2 / ulid crate 구체 버전
- chrono vs time datetime crate 선택
- Config struct 확장 세부 구조
- MCP error code 신규 할당 수치

## Deferred Ideas

Captured in CONTEXT.md `<deferred>` section. Key items:
- Metadata mutation tool (v2)
- Strategy versioning by name (v2)
- Connection pool (when single-Mutex becomes bottleneck)
- Migration system (when schema changes needed)
- XDG/OS-specific data dir (v2)
- Binary blob strategies / import-export (v2)
