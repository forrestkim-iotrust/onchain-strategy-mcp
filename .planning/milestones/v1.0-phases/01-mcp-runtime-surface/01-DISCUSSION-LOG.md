# Phase 1: MCP Runtime Surface - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-04-24
**Phase:** 01-mcp-runtime-surface
**Areas discussed:** Skeleton depth, Tool/Resource/Prompt implementation depth, Stdout discipline verification, Server config surface

---

## Skeleton depth

| Option | Description | Selected |
|--------|-------------|----------|
| C. 절충 (추천) | executor-mcp + executor-core + executor-state + executor-signer (trait만). 나머지 3개는 쓸 Phase에서 추가 | ✓ |
| B. 지금 쓰는 것만 | executor-mcp + executor-core만 | |
| A. 7개 전부 | 빈 lib.rs라도 7개 crate 전부 | |

**User's choice:** C. 절충
**Notes:** Pitfalls("overbuilding before first run")와 AGENTS.md("signer는 boundary로 둔다")를 동시에 만족시키는 중간값.

---

## Tool / Resource / Prompt implementation depth

| Option | Description | Selected |
|--------|-------------|----------|
| 추천 세트 | Tool: unimplemented 에러(list류는 빈 배열 정상 응답). Resource: URI 스킴 선언 + list 빈 배열. Prompt: 제목 + placeholder | ✓ |
| 완전 스큐어몰프 모드 | 모든 surface가 구조적으로 valid한 fake 응답 반환 (fake ID, mock data) | |
| 최소 모드 | Tool만 unimplemented. Resource/Prompt는 list에도 등장 안함 | |

**User's choice:** 추천 세트
**Notes:** 초기 답변에서 "너무 어렵게 설명하는데, 이해하기 쉽게 말할 것"이라는 피드백 받고, 이후 질문들을 더 평이한 말로 재구성.

---

## Stdout discipline verification

| Option | Description | Selected |
|--------|-------------|----------|
| Level 2 + 3 (추천) | Clippy lint + 통합 테스트(stdout 라인별 valid JSON-RPC assertion) | ✓ |
| Level 2만 | Clippy lint로 println! 금지만 | |
| Level 3만 | Clippy lint 없이 통합 테스트만 | |
| Level 1 | 문서에만 적어놓고 끝 | |

**User's choice:** Level 2 + 3
**Notes:** 초기화 테스트는 어차피 성공 기준이라 통합 테스트에 assertion 한 줄 추가 비용이 거의 없다는 판단.

---

## Server config surface

| Option | Description | Selected |
|--------|-------------|----------|
| A. env only, RUST_LOG만 (추천) | tracing 레벨만 env. 나머지는 Phase별 env 추가 | |
| B. config.toml 스키마 | Phase 1에 config 로더 + 스키마 뼈대. Phase 2~6이 섹션 추가 | ✓ |
| C. 하이브리드 | env + 선택적 config file | |

**User's choice:** B. config.toml 스키마
**Notes:** Claude 추천(A)과 다른 선택. 사용자가 "설정을 한곳에"를 선호 → 초기 스키마 범위를 두 번째 질문에서 좁힘.

### Follow-up: Phase 1 config.toml 초기 스키마

| Option | Description | Selected |
|--------|-------------|----------|
| B-1. 거의 비어있음 (추천) | [logging] level 하나만 실제 읽음. 다른 섹션 선언 안함 | ✓ |
| B-2. 모든 섹션 선언(placeholder 주석) | [state], [evm], [signer], [policy] 전부 주석으로 미리 선언 | |

**User's choice:** B-1. 거의 비어있음
**Notes:** "실행되는 코드와 연결되지 않은 빈 섹션은 허세" 원칙 수용.

---

## Claude's Discretion

- Tool/Resource/Prompt 스키마의 구조체 이름 및 `executor-core` 내 모듈 배치
- `schemars` 옵션(JSON Schema 2020-12 draft 등) 및 `serde` 직렬화 세부
- `rmcp` 1.5 초기화/lifecycle hook 와이어링 세부
- `tracing-subscriber` 포맷터 및 `EnvFilter` 설정
- 통합 테스트 harness 구체 (프로세스 spawn, stdin/stdout pipe, 라인 파싱 방식)
- Clippy lint 위치 (crate-level `#![deny(...)]` vs workspace `clippy.toml`)
- `config.toml` 경로를 CLI 인자로 받을지 env(`EXECUTOR_CONFIG`)로 받을지

## Deferred Ideas

- Streamable HTTP transport — v2
- Prompt 본문 실제 작성 — Phase 7
- Config hot-reload / validation UX — v1 scope 밖
- CLI subcommands 구조 (`serve`, `config dump` 등) — Phase 7 이후
- 다국어 에러 메시지 — 에이전트 대상이라 영어 고정
