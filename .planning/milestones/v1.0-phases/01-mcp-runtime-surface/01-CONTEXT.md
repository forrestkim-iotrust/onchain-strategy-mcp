# Phase 1: MCP Runtime Surface - Context

**Gathered:** 2026-04-24
**Status:** Ready for planning

<domain>
## Phase Boundary

Stdio 기반 MCP 서버가 깔끔하게 부팅되고, 런타임 계약(tools / resources / prompts 목록과 JSON Schema)을 노출한다.

이 Phase는 **MCP 표면의 계약(contract)만** 증명한다. 실제 로직은 후속 Phase에서:
- 전략 등록/조회/삭제/실행 로직 → Phase 2, 3
- EVM read/action 생성 → Phase 4
- 시뮬레이션 / 정책 게이트 → Phase 5
- 로컬 서명 / 브로드캐스트 / receipt → Phase 6

Phase 1이 통과하면 MCP 클라이언트가 `initialize`를 성공시키고, `tools/list`·`resources/list`·`prompts/list`에서 향후 runtime의 전체 표면을 스키마와 함께 본다. 단, 쓰기성 tool을 호출하면 `Unimplemented("implemented in Phase N")` 에러를 받는다.

</domain>

<decisions>
## Implementation Decisions

### Workspace skeleton
- **D-01:** Phase 1에서 4개 crate만 생성한다:
  - `executor-mcp` — MCP 서버 엔트리, 스키마 바인딩, stdio 전송
  - `executor-core` — 도메인 타입 (Strategy / Action / ExecutionReport / PolicyDecision 등 공통 타입) 및 trait
  - `executor-state` — SQLite 저장소 경계 (Phase 2 구현, Phase 1은 crate 존재만)
  - `executor-signer` — `Signer` trait 정의만 (v1 local signer는 Phase 6 구현)
- **D-01a:** 나머지 crate(`strategy-js`, `executor-evm`, `executor-policy`)는 해당 Phase에서 추가한다. Phase 1에서는 빈 껍데기로 만들지 않는다.
- **D-01b:** Root는 `Cargo.toml` workspace + `rust-toolchain.toml`(2024 edition) + `.cargo/config.toml`(필요 시 lint 프로필) 구성.

### MCP tool surface
- **D-02:** Roadmap이 정의한 전체 tool 목록을 노출한다(스키마 포함):
  - `strategy_register`, `strategy_list`, `strategy_get`, `strategy_delete`, `strategy_run_once`
  - `execution_get`
  - `policy_get`, `policy_update`
- **D-02a:** 쓰기/실행성 tool(`strategy_register`, `strategy_delete`, `strategy_run_once`, `policy_update`)은 Phase 1에서 호출 시 `McpError::Unimplemented("implemented in Phase <N>")`를 반환한다. N은 roadmap 의존성에 따라 명시.
- **D-02b:** 순수 조회성 tool(`strategy_list`, `strategy_get`, `execution_get`, `policy_get`)은 정상 응답 모양을 돌려줄 수 있다. `strategy_list`는 빈 배열, `policy_get`은 기본값 policy(아직 정책 모델 없음 → 비어있는 구조) — 단, 이것도 Phase 2/5에서 실제 로직 붙기 전까지는 placeholder임을 `description` 스키마 텍스트에서 명시.
- **D-02c:** Tool 입력/출력 JSON Schema는 Phase 1에서 **실제 모양으로 확정**한다. 이유: 스키마는 계약이라 agent쪽에 전파되면 변경 비용이 크다. `serde` 구조체 + `schemars::JsonSchema` derive로 정의.

### MCP resource surface
- **D-03:** Resource URI 스킴 세 개를 서버에 선언한다:
  - `strategy://{strategy_id}`
  - `execution://{execution_id}`
  - `journal://{execution_id}`
- **D-03a:** `resources/list`는 빈 배열을 반환한다 (등록된 것이 없으므로). `resources/read`는 해당 URI 스킴이지만 대상이 없을 때 "Not found"를 반환한다. Phase 2+ 이후 등록된 항목이 채워진다.

### MCP prompt surface
- **D-04:** `write_evm_strategy`, `review_evm_strategy` 두 prompt를 선언한다. 제목/description/인자 스키마까지는 확정, **본문 템플릿은 Phase 7 문서 정리 단계에서 최종 작성**한다. Phase 1에서는 placeholder 문자열 (예: "Strategy authoring prompt — body will be finalized after ctx API stabilizes").

### Stdout discipline
- **D-05:** 두 층위로 강제한다:
  1. **Clippy lint**: `executor-mcp` crate (및 동일 프로세스에서 stdio를 공유하는 모든 bin)는 `#![deny(clippy::print_stdout, clippy::print_stderr, clippy::dbg_macro)]`. `eprintln!`도 금지하여 로깅 경로를 오로지 `tracing`으로만 강제.
  2. **통합 테스트**: Phase 1 integration test가 서버 프로세스를 spawn하고 (stdin/stdout pipe), `initialize` / `tools/list` / `resources/list` / `prompts/list` 요청/응답을 왕복시킨다. stdout으로 들어오는 **모든 줄이 valid JSON-RPC 2.0 메시지**임을 assert. 하나라도 아니면 테스트 실패.
- **D-05a:** stderr은 `tracing-subscriber`로 포맷. 개발 중 `RUST_LOG=debug`로 확인.

### Server configuration
- **D-06:** `config.toml` 로딩을 Phase 1에 도입한다(추후 phase에서 field만 추가하면 되도록).
- **D-06a:** Phase 1에서 스키마는 **최소**:
  ```toml
  [logging]
  level = "info"   # trace|debug|info|warn|error
  ```
  다른 섹션(`[state]`, `[evm]`, `[signer]`, `[policy]`)은 선언하지 않는다. 해당 Phase에서 실제 소비 코드와 함께 추가.
- **D-06b:** `config.toml` 경로는 CLI 인자 또는 env (`EXECUTOR_CONFIG` 등) — Phase 1 planning에서 결정. 파일이 없으면 내장 기본값(`level = "info"`)으로 부팅한다.

### Claude's Discretion
- Tool/Resource/Prompt 스키마의 실제 구조체 이름 및 모듈 배치 (`executor-core::schema::*` 수준에서 알아서).
- `schemars` 2020-12 Draft 활성화 옵션, `serde` 직렬화 세부.
- `rmcp` 1.5 초기화 콜백 배치 / lifecycle hook 구성.
- `tracing-subscriber` 초기화 세부 (EnvFilter, 포맷터).
- 통합 테스트 harness 세부 (`tokio::process`, line-by-line JSON 파싱 방식).
- Clippy lint 설정 위치 (crate-level `#![deny(...)]` vs workspace-level `clippy.toml`).

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Project planning
- `.planning/PROJECT.md` — 프로젝트 본질, 스코프, out-of-scope, Key Decisions
- `.planning/REQUIREMENTS.md` — 요구사항 ID(MCP-01~04가 Phase 1), traceability
- `.planning/ROADMAP.md` §"Phase 1: MCP Runtime Surface" — 목표, 성공 기준, 하위 플랜 분할
- `AGENTS.md` — 스택/crate 경계/hard boundary(특히 "stdio MCP must not write logs to stdout")

### Research briefs
- `.planning/research/STACK.md` — 선택된 라이브러리와 버전(`rmcp` 1.5, `schemars` 1.2, `tokio`, `tracing` 등), MCP 2025-11-25 스펙 인용
- `.planning/research/ARCHITECTURE.md` — crate 경계, tool/resource/prompt 목록, runtime flow
- `.planning/research/PITFALLS.md` — "Stdio logging bug", "Overbuilding before first run", "Overbroad MCP tools" 등 Phase 1이 정면으로 다루는 함정
- `.planning/research/FEATURES.md` — table-stakes / differentiators / MVP 범위

### External specs (research에서 인용된 외부 문서)
- MCP Base Protocol 2025-11-25: https://modelcontextprotocol.io/specification/2025-11-25/basic/index
- MCP Lifecycle: https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle
- MCP Transports (stdio): https://modelcontextprotocol.io/specification/2025-11-25/basic/transports
- MCP Server Features (tools/resources/prompts): https://modelcontextprotocol.io/specification/2025-11-25/server/index
- MCP Tools schema: https://modelcontextprotocol.io/specification/2025-11-25/server/tools
- Official Rust SDK (`rmcp`): https://github.com/modelcontextprotocol/rust-sdk

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- 아직 코드가 없음 (fresh project). `.planning/`에 문서만 존재.

### Established Patterns
- `AGENTS.md`가 **"strategy code returns Action[]; does not sign or broadcast"**, **"Simulation and policy must run before signing"**, **"Stdio MCP servers must not write logs to stdout"**를 hard boundary로 선언 — Phase 1 계약/테스트에 반드시 반영.
- 프로젝트 전반이 한글/영어 혼용이되, 코드/파일명/커밋 메시지는 영어, 설계 문서는 한국어 허용.

### Integration Points
- Phase 1이 생성하는 `executor-mcp` 바이너리가 이후 모든 Phase의 진입점 — `strategy-js` 통합(Phase 3), `executor-evm` 통합(Phase 4) 등 모두 여기에 와이어링됨.
- `executor-core`가 선언하는 도메인 타입(Strategy, Action, ExecutionReport, PolicyDecision)은 Phase 2~6이 공통으로 참조.
- `executor-signer::Signer` trait 경계는 Phase 6의 local signer 구현과 v2의 external signer를 분리할 수 있도록 Phase 1에 자리만 잡아둠.

</code_context>

<specifics>
## Specific Ideas

- 통합 테스트 assertion 예시: "stdout으로 들어오는 모든 줄이 `serde_json::from_str::<JsonRpcMessage>`에 성공한다" — JSON으로도 파싱 안 되는 쓰레기/warning이 섞이면 즉시 실패.
- Unimplemented tool의 에러 메시지는 구조화된 형태로: `{ "code": "unimplemented", "phase": <N>, "hint": "will be implemented when Phase <N> lands" }`. Agent가 "아직 못함/나중에 됨"을 구분할 수 있게.
- 설정 파일 예시(미래 확장 형태, Phase 1에는 logging만 존재):
  ```toml
  [logging]
  level = "info"
  # Phase 2+ will add [state], Phase 4 [evm], Phase 5 [policy], Phase 6 [signer]
  ```

</specifics>

<deferred>
## Deferred Ideas

- **Streamable HTTP transport** — v2. 인증/세션/Origin 이슈는 v1 scope 밖.
- **Prompt 본문 실제 작성** — `ctx` API 확정 후 Phase 7 문서 정리 단계에서. Phase 1은 선언만.
- **Config hot-reload / validation UX** — v1은 시작 시 1회 로드. 재시작 필요.
- **CLI subcommands 외 server bootstrap 방식** — 지금은 `executor-mcp` bin 한 개가 stdio 서버. 향후 `executor-mcp serve`, `executor-mcp config dump` 같은 subcommand 구조는 Phase 7 이후.
- **다국어 에러 메시지** — 에이전트 대상이므로 영어 고정.

</deferred>

---

*Phase: 01-mcp-runtime-surface*
*Context gathered: 2026-04-24*
