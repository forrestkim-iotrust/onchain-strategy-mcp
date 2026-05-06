# Phase 1: MCP Runtime Surface - Research

**Researched:** 2026-04-24
**Domain:** Rust MCP 서버 계약 표면 (rmcp 1.5 + schemars 1.2 기반 stdio 서버)
**Confidence:** HIGH

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**Workspace skeleton**
- **D-01:** Phase 1에서 4개 crate만 생성한다:
  - `executor-mcp` — MCP 서버 엔트리, 스키마 바인딩, stdio 전송
  - `executor-core` — 도메인 타입 (Strategy / Action / ExecutionReport / PolicyDecision 등 공통 타입) 및 trait
  - `executor-state` — SQLite 저장소 경계 (Phase 2 구현, Phase 1은 crate 존재만)
  - `executor-signer` — `Signer` trait 정의만 (v1 local signer는 Phase 6 구현)
- **D-01a:** 나머지 crate(`strategy-js`, `executor-evm`, `executor-policy`)는 Phase 1에서 만들지 않는다.
- **D-01b:** Root는 `Cargo.toml` workspace + `rust-toolchain.toml`(2024 edition) + `.cargo/config.toml`(필요 시 lint 프로필) 구성.

**MCP tool surface**
- **D-02:** Roadmap이 정의한 전체 tool 목록을 스키마 포함 노출:
  - `strategy_register`, `strategy_list`, `strategy_get`, `strategy_delete`, `strategy_run_once`
  - `execution_get`
  - `policy_get`, `policy_update`
- **D-02a:** 쓰기/실행성 tool(`strategy_register`, `strategy_delete`, `strategy_run_once`, `policy_update`)은 Phase 1에서 호출 시 `McpError::Unimplemented("implemented in Phase <N>")` 반환. N은 roadmap 의존성 따라 명시.
- **D-02b:** 순수 조회성 tool(`strategy_list`, `strategy_get`, `execution_get`, `policy_get`)은 정상 응답 모양으로 돌려줄 수 있다. `strategy_list`는 빈 배열, `policy_get`은 기본값. 스키마 설명에서 placeholder임을 명시.
- **D-02c:** Tool I/O JSON Schema는 Phase 1에서 **실제 모양으로 확정**. `serde` 구조체 + `schemars::JsonSchema` derive로 정의.

**MCP resource surface**
- **D-03:** Resource URI 스킴 세 개 선언:
  - `strategy://{strategy_id}`
  - `execution://{execution_id}`
  - `journal://{execution_id}`
- **D-03a:** `resources/list`는 빈 배열. `resources/read`는 대상 없으면 "Not found".

**MCP prompt surface**
- **D-04:** `write_evm_strategy`, `review_evm_strategy` 두 prompt 선언. 제목/description/인자 스키마 확정, **본문 템플릿은 Phase 7에서 최종 작성**. Phase 1은 placeholder 문자열.

**Stdout discipline**
- **D-05:** 두 층위로 강제:
  1. **Clippy lint**: `#![deny(clippy::print_stdout, clippy::print_stderr, clippy::dbg_macro)]`
  2. **통합 테스트**: 서버 spawn 후 stdout 모든 줄이 valid JSON-RPC 2.0 메시지여야 함
- **D-05a:** stderr은 `tracing-subscriber`로 포맷.

**Server configuration**
- **D-06:** `config.toml` 로딩을 Phase 1에 도입.
- **D-06a:** Phase 1 스키마는 **최소**:
  ```toml
  [logging]
  level = "info"
  ```
- **D-06b:** 경로는 CLI 인자 또는 env(`EXECUTOR_CONFIG`) — Phase 1 planning에서 결정. 파일 없으면 내장 기본값으로 부팅.

### Claude's Discretion
- Tool/Resource/Prompt 스키마 실제 구조체 이름 및 모듈 배치 (`executor-core::schema::*` 수준에서).
- `schemars` 2020-12 Draft 활성화 옵션, `serde` 직렬화 세부.
- `rmcp` 1.5 초기화 콜백 배치 / lifecycle hook 구성.
- `tracing-subscriber` 초기화 세부 (EnvFilter, 포맷터).
- 통합 테스트 harness 세부 (`tokio::process`, line-by-line JSON 파싱).
- Clippy lint 설정 위치 (crate-level `#![deny(...)]` vs workspace-level `clippy.toml`).

### Deferred Ideas (OUT OF SCOPE)
- **Streamable HTTP transport** — v2.
- **Prompt 본문 실제 작성** — Phase 7.
- **Config hot-reload / validation UX** — v1은 시작 시 1회 로드.
- **CLI subcommands** (`executor-mcp serve`, `executor-mcp config dump` 등) — Phase 7 이후.
- **다국어 에러 메시지** — 영어 고정.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| MCP-01 | Server can run as a stdio MCP server without writing non-MCP data to stdout. | Stdout discipline: rmcp `transport::stdio()` + stderr-only tracing + clippy lint + integration test assertion ("모든 stdout 줄이 JSON-RPC 파싱 성공") |
| MCP-02 | Server exposes JSON-schema-backed tools for strategy, execution, and policy operations. | `#[tool_router]` + `#[tool]` + `Parameters<T: JsonSchema>` → rmcp가 `tools/list`에서 input/output schema 자동 방출. Phase 1은 쓰기성 tool → `McpError` (unimplemented). |
| MCP-03 | Server exposes resources for strategy details, execution reports, and journal entries. | `ServerHandler::list_resource_templates` override로 URI 템플릿 세 개(`strategy://{id}`, `execution://{id}`, `journal://{id}`) 선언. `list_resources`는 빈 배열, `read_resource`는 `McpError::resource_not_found`. |
| MCP-04 | Server exposes prompts for writing and reviewing EVM automation strategies. | `#[prompt_router]` + `#[prompt]` + `Parameters<Args: JsonSchema>`로 `write_evm_strategy`, `review_evm_strategy` 선언. 본문은 placeholder `PromptMessage::new_text` 하나. |
</phase_requirements>

## Summary

Phase 1의 본질은 **rmcp 1.5의 macro 기반 라우터(`#[tool_router]`, `#[prompt_router]`, `#[tool_handler]`, `#[prompt_handler]`)를 받아들이고, 그 위에 serde + schemars 1.2로 정의한 도메인 스키마를 얹는 것**이다. rmcp는 `schemars` feature flag가 켜져 있을 때 `Parameters<T>` 래퍼를 통해 자동으로 tool input schema를 2020-12 Draft로 방출하며, `Json<T>` 래퍼로 output schema까지 방출한다. ServerHandler trait은 26개의 기본 구현 메서드를 제공해 resource/prompt 목록과 lifecycle hook만 오버라이드하면 된다. [VERIFIED: counter.rs 예제 — rust-sdk main branch]

Stdout 규율은 세 겹(crate-level `#![deny(clippy::print_stdout, ...)]`, workspace lint, stdin/stdout duplex를 이용한 통합 테스트)을 두는 것이 SDK 자체 테스트 관례와 일치한다. rmcp는 tracing을 내부적으로 사용하며, stdio transport 위에서도 tracing 초기화(`tracing_subscriber::fmt().with_writer(std::io::stderr)`)만 해주면 stdout은 자동으로 JSON-RPC 전용이 된다. [CITED: examples/servers/src/counter_stdio.rs]

Unimplemented 에러는 `ErrorData::new(ErrorCode::INTERNAL_ERROR, ..., Some(json!({...})))` 또는 커스텀 코드로 구조화된 `data` payload를 담아 반환한다. `McpError`는 `rmcp::ErrorData`의 alias. [VERIFIED: rmcp 1.5.0 docs.rs ErrorData API]

**Primary recommendation:** SDK의 `counter_stdio.rs` + `common/counter.rs`를 Phase 1의 reference blueprint으로 삼아 1:1 매핑하라. 자체적인 tool/resource/prompt dispatch 로직은 절대 쓰지 말고 rmcp macro에 맡긴다. `executor-core`에 도메인 타입 + `JsonSchema` derive만 깔끔하게 얹고 `executor-mcp`에서 macro로 바인딩한다.

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| MCP stdio transport | `executor-mcp` (bin) | — | rmcp `transport::stdio()`가 stdin/stdout pipe 관리 |
| Tool dispatch + schema emission | `executor-mcp` (bin) | `executor-core` (types) | `#[tool_router]`는 handler가 정의된 struct에 붙어야 함. 스키마용 구조체는 core crate. |
| Resource URI template declaration | `executor-mcp` (bin) | — | `ServerHandler::list_resource_templates` override |
| Prompt declaration | `executor-mcp` (bin) | `executor-core` (args types) | `#[prompt_router]` — args 구조체는 core에 |
| Domain types (Strategy, Action, ExecutionReport, PolicyDecision) | `executor-core` (lib) | — | Phase 2~6 공통 참조. JsonSchema derive 포함. |
| Signer trait boundary | `executor-signer` (lib) | — | Phase 6에서 local signer 구현 — Phase 1은 trait만 |
| State repository boundary | `executor-state` (lib) | — | Phase 2 구현 — Phase 1은 placeholder trait/module |
| Logging (stderr-only) | `executor-mcp` (bin) | — | `tracing-subscriber` stderr writer |
| Config loading | `executor-mcp` (bin) | — | `toml` + `serde` + 기본값 fallback |
| Clippy lint enforcement | workspace + crate | — | workspace `Cargo.toml [workspace.lints.clippy]` + crate-level `#![deny(...)]` 이중 |

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `rmcp` | `1.5.0` | MCP server SDK (tools/resources/prompts, stdio transport) | 공식 Rust SDK. 2026-04-16 릴리즈. `#[tool_router]`/`#[prompt_router]` macro 제공. [VERIFIED: crates.io API] |
| `rmcp` features | `server`, `macros`, `schemars`, `transport-io` | Phase 1에 필요한 feature만 | `transport-io`가 stdio 지원. `schemars` feature가 JsonSchema 자동 emission. `client` 불필요. [CITED: rust-sdk/examples/servers/Cargo.toml] |
| `schemars` | `1.2.1` | JSON Schema 2020-12 Draft 생성 | rmcp 1.5가 `schemars = "1.0"` (semver-compatible)로 선언. 2026-02-01 릴리즈. [VERIFIED: crates.io API + rmcp Cargo.toml] |
| `serde` | `1.0` | 구조체 직렬화 | `derive` feature. 표준. |
| `serde_json` | `1.0` | JSON value 조작 (에러 data payload) | 표준. |
| `tokio` | `1` | Async runtime | features: `macros`, `rt-multi-thread`, `io-std`, `signal`. rmcp examples와 동일. [CITED: rust-sdk/examples/servers/Cargo.toml] |
| `tracing` | `0.1` | stderr 구조화 로깅 | rmcp 내부도 tracing 사용 — 일관성. |
| `tracing-subscriber` | `0.3` | tracing writer 초기화 | features: `env-filter`, `fmt`, `std`. `RUST_LOG` 지원. |
| `anyhow` | `1.0` | main()/test error 집계 | rmcp examples 표준 패턴. |
| `toml` | `0.8` | config.toml 로딩 | serde 친화, 경량. [ASSUMED: 현재 안정 메이저] |
| `thiserror` | `2` | 도메인 에러 taxonomy | rmcp 자체가 thiserror 2 의존. 같은 메이저 맞추기. [CITED: rmcp Cargo.toml] |

### Supporting (Phase 1 optional)

| Library | Purpose | When to Use |
|---------|---------|-------------|
| `clap` 4.x | CLI arg 파싱 (config path) | env `EXECUTOR_CONFIG` 대신 `--config` flag 쓸 경우 — Discretion. |
| `uuid` 1.x | Strategy/Execution ID 예시용 | schema example value에 쓰는 정도. Phase 2부터 본격 사용. |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `#[tool_router]` macro | Manual `ServerHandler::list_tools` / `call_tool` impl | Boilerplate 폭증 + schema 수동 관리. SDK가 macro 경로를 blessed path으로 명확히 보여줌. 수동은 학습비용만 더함. |
| `schemars` 1.2 | `schemars` 0.8 (구버전) | rmcp 1.5가 1.0 series 요구 (`schemars = { version = "1.0", ... }`). 0.8은 non-starter. |
| `toml` 0.8 | `figment` / `config` crate | Phase 1 스키마가 `[logging] level = "..."` 한 줄. 의존성 추가할 이유 없음. |
| `#[prompt(name = "...", description = "...")]` attrs | 수동 `list_prompts` + `get_prompt` | macro 경로가 `Args` 구조체에서 prompt argument schema 자동 추출. 수동은 이 부분 직접 만들어야. |

**Installation (executor-mcp Cargo.toml):**
```toml
[dependencies]
rmcp = { version = "1.5", features = ["server", "macros", "schemars", "transport-io"] }
schemars = "1.2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "io-std", "signal"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt", "std"] }
anyhow = "1"
toml = "0.8"
executor-core = { path = "../executor-core" }
executor-state = { path = "../executor-state" }
executor-signer = { path = "../executor-signer" }

[dev-dependencies]
tokio = { version = "1", features = ["process", "io-util", "time", "macros"] }
```

**Version verification performed:**
- `rmcp` 1.5.0 — 2026-04-16 릴리즈 [VERIFIED: crates.io API]
- `schemars` 1.2.1 — 2026-02-01 릴리즈 [VERIFIED: crates.io API]
- Planner가 task를 만들 때 `cargo add rmcp@1.5 --features server,macros,schemars,transport-io` 실행하면 현재 버전으로 잠긴다.

## rmcp 1.5 API Reference

이 섹션은 Phase 1 Plan이 구체적 task를 내릴 때 기준이 되는 **검증된 코드 패턴**이다. 출처는 SDK 공식 `examples/servers/src/common/counter.rs` (main branch, 2026-04-24 기준).

### Import surface

```rust
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler, ServiceExt,
    handler::server::{
        router::{prompt::PromptRouter, tool::ToolRouter},
        wrapper::Parameters,
    },
    model::*,
    prompt, prompt_handler, prompt_router, schemars,
    service::RequestContext,
    tool, tool_handler, tool_router,
    transport::stdio,
};
```

주: `rmcp::schemars`는 re-export. `rmcp::ErrorData`를 `McpError`로 alias 하는 것이 SDK 관례. [CITED: counter.rs L4-L16]

### Tool 등록 패턴 (쓰기성 tool이 Unimplemented 반환하는 shape 포함)

```rust
#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[schemars(description = "Register a new JavaScript strategy (Phase 2).")]
pub struct StrategyRegisterInput {
    #[schemars(description = "Human-readable strategy name.")]
    pub name: String,
    #[schemars(description = "JavaScript source (will run in sandbox).")]
    pub source: String,
    #[schemars(description = "Arbitrary metadata bag.")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Clone)]
pub struct ExecutorServer {
    tool_router: ToolRouter<Self>,
    prompt_router: PromptRouter<Self>,
    // state: Arc<dyn StateRepo>, ... (Phase 2+)
}

#[tool_router]
impl ExecutorServer {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
            prompt_router: Self::prompt_router(),
        }
    }

    // ─────────── WRITE-CAPABLE TOOLS (Unimplemented in Phase 1) ───────────

    #[tool(
        name = "strategy_register",
        description = "Register a JavaScript strategy. NOT YET IMPLEMENTED — lands in Phase 2."
    )]
    async fn strategy_register(
        &self,
        Parameters(_input): Parameters<StrategyRegisterInput>,
    ) -> Result<CallToolResult, McpError> {
        Err(unimplemented_err("strategy_register", 2))
    }

    #[tool(name = "strategy_delete", description = "Delete a registered strategy. Phase 2.")]
    async fn strategy_delete(
        &self,
        Parameters(_input): Parameters<StrategyIdInput>,
    ) -> Result<CallToolResult, McpError> {
        Err(unimplemented_err("strategy_delete", 2))
    }

    #[tool(name = "strategy_run_once", description = "Execute a strategy once. Phase 3–6.")]
    async fn strategy_run_once(
        &self,
        Parameters(_input): Parameters<StrategyRunOnceInput>,
    ) -> Result<CallToolResult, McpError> {
        Err(unimplemented_err("strategy_run_once", 6))
    }

    #[tool(name = "policy_update", description = "Replace current policy. Phase 5.")]
    async fn policy_update(
        &self,
        Parameters(_input): Parameters<PolicyUpdateInput>,
    ) -> Result<CallToolResult, McpError> {
        Err(unimplemented_err("policy_update", 5))
    }

    // ─────────── READ-ONLY TOOLS (empty but valid response) ───────────

    #[tool(name = "strategy_list", description = "List registered strategies. Phase 2 fills this.")]
    async fn strategy_list(&self) -> Result<CallToolResult, McpError> {
        Ok(CallToolResult::success(vec![Content::text("[]")]))
        // Phase 2부터 structured output: Json<StrategyListOutput>로 교체
    }

    #[tool(name = "strategy_get", description = "Get a strategy by id. Phase 2.")]
    async fn strategy_get(
        &self,
        Parameters(_input): Parameters<StrategyIdInput>,
    ) -> Result<CallToolResult, McpError> {
        Err(McpError::resource_not_found(
            "strategy not found (empty store)",
            Some(serde_json::json!({ "phase": 2 })),
        ))
    }

    #[tool(name = "execution_get", description = "Get execution report by id. Phase 6.")]
    async fn execution_get(
        &self,
        Parameters(_input): Parameters<ExecutionIdInput>,
    ) -> Result<CallToolResult, McpError> {
        Err(McpError::resource_not_found(
            "execution not found (empty store)",
            Some(serde_json::json!({ "phase": 6 })),
        ))
    }

    #[tool(name = "policy_get", description = "Get current policy. Phase 5 fills this.")]
    async fn policy_get(&self) -> Result<CallToolResult, McpError> {
        let placeholder = serde_json::json!({
            "chains": [],
            "targets": [],
            "selectors": [],
            "note": "policy engine lands in Phase 5"
        });
        Ok(CallToolResult::success(vec![Content::text(placeholder.to_string())]))
    }
}
```

> **핵심:** `Parameters<T: JsonSchema>` 래퍼가 rmcp에게 "이 tool의 input schema를 T에서 뽑아내라"고 알려준다. tool description은 `#[tool(description = "...")]`에서, parameter description은 `#[schemars(description = "...")]`에서 온다. [CITED: counter.rs + prompt_stdio.rs 패턴]

### Prompt 등록 패턴

```rust
#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct WriteEvmStrategyArgs {
    /// 전략이 해야 할 일에 대한 자연어 설명
    pub intent: String,
    /// 대상 체인(예: "anvil", "base", "arbitrum-sepolia")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chain_hint: Option<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ReviewEvmStrategyArgs {
    /// 리뷰 대상 strategy_id
    pub strategy_id: String,
}

#[prompt_router]
impl ExecutorServer {
    #[prompt(
        name = "write_evm_strategy",
        description = "Author a new EVM automation strategy. Body finalized in Phase 7."
    )]
    async fn write_evm_strategy(
        &self,
        Parameters(_args): Parameters<WriteEvmStrategyArgs>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<Vec<PromptMessage>, McpError> {
        Ok(vec![PromptMessage::new_text(
            PromptMessageRole::User,
            "Strategy authoring prompt — body will be finalized after ctx API stabilizes (Phase 7).",
        )])
    }

    #[prompt(
        name = "review_evm_strategy",
        description = "Review an existing EVM strategy for safety and correctness. Body in Phase 7."
    )]
    async fn review_evm_strategy(
        &self,
        Parameters(_args): Parameters<ReviewEvmStrategyArgs>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, McpError> {
        Ok(GetPromptResult::new(vec![PromptMessage::new_text(
            PromptMessageRole::User,
            "Strategy review prompt — body will be finalized in Phase 7.",
        )])
        .with_description("Placeholder review prompt"))
    }
}
```

> prompt argument schema가 `Args` 구조체에서 자동 추출된다 — Phase 7에서 본문만 바꾸면 arguments 스키마는 그대로 유지된다. [CITED: prompt_stdio.rs + counter.rs L154-206]

### ServerHandler impl — resource templates + lifecycle

```rust
#[tool_handler]
#[prompt_handler]
impl ServerHandler for ExecutorServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_prompts()
                .enable_resources()
                .build(),
        )
        .with_server_info(Implementation::from_build_env())
        .with_protocol_version(ProtocolVersion::V_2025_11_25) // 또는 V_2025_06_18. ServerInfo default 허용.
        .with_instructions(
            "Onchain Strategy MCP — v1 runtime surface. Write-capable tools \
             return Unimplemented until their phase lands."
                .to_string(),
        )
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        // Phase 1: empty. Phase 2+ populates from state repository.
        Ok(ListResourcesResult {
            resources: Vec::new(),
            next_cursor: None,
            meta: None,
        })
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        Ok(ListResourceTemplatesResult {
            resource_templates: vec![
                ResourceTemplate {
                    uri_template: "strategy://{strategy_id}".into(),
                    name: "strategy".into(),
                    description: Some("Registered strategy (source + metadata).".into()),
                    mime_type: Some("application/json".into()),
                    ..Default::default()
                },
                ResourceTemplate {
                    uri_template: "execution://{execution_id}".into(),
                    name: "execution".into(),
                    description: Some("Execution report with status and receipt.".into()),
                    mime_type: Some("application/json".into()),
                    ..Default::default()
                },
                ResourceTemplate {
                    uri_template: "journal://{execution_id}".into(),
                    name: "journal".into(),
                    description: Some("Journal entries for an execution.".into()),
                    mime_type: Some("application/json".into()),
                    ..Default::default()
                },
            ],
            next_cursor: None,
            meta: None,
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        // Phase 1: always not found. URI 스킴 validation은 Phase 2+에서.
        Err(McpError::resource_not_found(
            "resource not found",
            Some(serde_json::json!({ "uri": request.uri, "phase": 1 })),
        ))
    }
}
```

> `ResourceTemplate` 정확한 필드는 rmcp 1.5의 `model::ResourceTemplate` 기준. `..Default::default()`로 annotations/icons/title 생략. [CITED: MCP spec 2025-11-25 resources + counter.rs L270-280]

### main() entry point

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = executor_mcp::config::load()?;
    executor_mcp::logging::init(&config)?; // stderr-only tracing subscriber

    tracing::info!(version = env!("CARGO_PKG_VERSION"), "executor-mcp starting");

    let server = ExecutorServer::new();
    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
```

> `stdio()`는 `(tokio::io::stdin(), tokio::io::stdout())` duplex를 만드는 helper. tracing writer는 반드시 stderr. [CITED: counter_stdio.rs L8-14]

## Cargo Workspace Layout

### Directory tree

```text
onchain-strategy-mcp/
├── Cargo.toml                    # workspace root
├── rust-toolchain.toml           # 2024 edition pin
├── .cargo/
│   └── config.toml               # 필요 시 lint profile (Discretion)
├── config.example.toml           # 주석 달린 샘플
├── AGENTS.md
├── README.md
├── .planning/
│   └── ... (already exists)
├── crates/
│   ├── executor-mcp/
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── main.rs           # tokio main, config 로드, serve(stdio())
│   │   │   ├── lib.rs            # 재노출
│   │   │   ├── server.rs         # ExecutorServer struct + impls
│   │   │   ├── tools.rs          # #[tool_router] impl
│   │   │   ├── prompts.rs        # #[prompt_router] impl
│   │   │   ├── resources.rs      # ServerHandler resource methods
│   │   │   ├── errors.rs         # unimplemented_err(tool, phase) helper
│   │   │   ├── config.rs         # toml 로딩 + 기본값
│   │   │   └── logging.rs        # tracing_subscriber 초기화
│   │   └── tests/
│   │       └── stdio_handshake.rs # 통합 테스트 (spawn + round-trip)
│   ├── executor-core/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── schema/
│   │       │   ├── mod.rs
│   │       │   ├── strategy.rs   # StrategyRegisterInput / StrategyIdInput / Strategy
│   │       │   ├── action.rs     # Action enum (ContractCall, RawCall, Erc20*, NativeTransfer)
│   │       │   ├── execution.rs  # ExecutionReport, ExecutionIdInput
│   │       │   ├── policy.rs     # PolicyModel, PolicyDecision, PolicyUpdateInput
│   │       │   └── prompt_args.rs # WriteEvmStrategyArgs, ReviewEvmStrategyArgs
│   │       └── error.rs          # thiserror 도메인 에러
│   ├── executor-state/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── lib.rs            # Phase 1: trait placeholder만
│   └── executor-signer/
│       ├── Cargo.toml
│       └── src/
│           └── lib.rs            # Signer trait 정의만
└── target/
```

### Root `Cargo.toml`

```toml
[workspace]
resolver = "2"
members = [
  "crates/executor-mcp",
  "crates/executor-core",
  "crates/executor-state",
  "crates/executor-signer",
]

[workspace.package]
edition = "2024"
version = "0.1.0"
license = "Apache-2.0"
repository = "https://github.com/<owner>/onchain-strategy-mcp"

[workspace.dependencies]
rmcp = { version = "1.5", features = ["server", "macros", "schemars", "transport-io"] }
schemars = "1.2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "io-std", "signal"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt", "std"] }
anyhow = "1"
thiserror = "2"
toml = "0.8"

[workspace.lints.rust]
unsafe_code = "forbid"
unreachable_pub = "warn"

[workspace.lints.clippy]
# Stdout discipline (D-05): applied workspace-wide
print_stdout  = "deny"
print_stderr  = "deny"
dbg_macro     = "deny"
todo          = "warn"
unimplemented = "warn"
```

> `[workspace.lints]`는 Rust 1.74+에서 지원됨(2024 edition 포함). crate-level `#![deny(...)]`을 함께 두면 디렉티브가 IDE/CI에서 이중으로 보인다. [CITED: Cargo book + 현재 안정] [ASSUMED: 2024 edition에서도 동일 동작]

### `rust-toolchain.toml`

```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
profile = "minimal"
```

> 2024 edition은 각 crate의 `Cargo.toml`에서 `edition = "2024"`로 잠기므로, 툴체인은 stable로 충분. [VERIFIED: rmcp 자체가 `edition = "2024"`]

### `crates/executor-mcp/Cargo.toml`

```toml
[package]
name = "executor-mcp"
version.workspace = true
edition.workspace = true
license.workspace = true

[[bin]]
name = "executor-mcp"
path = "src/main.rs"

[dependencies]
rmcp.workspace = true
schemars.workspace = true
serde.workspace = true
serde_json.workspace = true
tokio.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
anyhow.workspace = true
thiserror.workspace = true
toml.workspace = true
executor-core = { path = "../executor-core" }
executor-state = { path = "../executor-state" }
executor-signer = { path = "../executor-signer" }

[dev-dependencies]
tokio = { workspace = true, features = ["macros", "rt-multi-thread", "io-std", "signal", "process", "io-util", "time"] }
serde_json.workspace = true
anyhow.workspace = true
```

### `crates/executor-core/Cargo.toml`

```toml
[package]
name = "executor-core"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
schemars.workspace = true
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
```

> `executor-core`는 **`rmcp` 의존 금지** — 순수 도메인 타입 crate. Phase 2+에서 `executor-state`가 re-use할 때 rmcp 의존성 없이 가볍게 쓰기 위함.

### `crates/executor-signer/Cargo.toml` / `crates/executor-state/Cargo.toml`

Phase 1은 각 crate에 `src/lib.rs` 하나만:

```rust
// executor-signer/src/lib.rs
use executor_core::schema::execution::SignedTransaction;

/// Phase 6에서 LocalSigner로 구현된다.
pub trait Signer: Send + Sync {
    // 실제 메서드는 Phase 6 연구 후 확정. Phase 1은 boundary만.
}
```

```rust
// executor-state/src/lib.rs
//! Strategy/execution/journal persistence boundary.
//! Phase 2에서 SQLite 구현이 이 자리에 들어온다.
```

## Unimplemented Error Shape

`rmcp::ErrorData` (alias `McpError`)의 생성자는 다음과 같다 [VERIFIED: docs.rs rmcp 1.5]:

```rust
impl ErrorData {
    pub fn new(code: ErrorCode, message: impl Into<Cow<'static, str>>, data: Option<Value>) -> Self;
    pub fn resource_not_found(message: impl Into<Cow<'static, str>>, data: Option<Value>) -> Self;
    pub fn invalid_params(message: impl Into<Cow<'static, str>>, data: Option<Value>) -> Self;
    pub fn internal_error(message: impl Into<Cow<'static, str>>, data: Option<Value>) -> Self;
    // …
}
```

JSON-RPC 표준은 `-32000 ~ -32099`를 "server-defined implementation-defined errors"로 예약. **Phase 1 권장 패턴:**

```rust
// crates/executor-mcp/src/errors.rs
use rmcp::{ErrorData as McpError, model::ErrorCode};
use serde_json::json;

/// -32010: unimplemented feature. server-defined range.
const UNIMPLEMENTED_CODE: ErrorCode = ErrorCode(-32010);

pub fn unimplemented_err(tool_name: &'static str, phase: u8) -> McpError {
    McpError::new(
        UNIMPLEMENTED_CODE,
        format!("{tool_name} is not implemented yet (lands in Phase {phase})"),
        Some(json!({
            "code": "unimplemented",
            "tool": tool_name,
            "phase": phase,
            "hint": format!("will be implemented when Phase {phase} lands"),
        })),
    )
}
```

> `ErrorCode`는 `model::ErrorCode(pub i32)` — JSON-RPC standard codes 외 custom code 허용. 실제 rmcp 소스에서 `ErrorCode::INTERNAL_ERROR`, `ErrorCode::INVALID_PARAMS` 등 상수 노출. [VERIFIED: rmcp 1.5 docs.rs ErrorCode page — CITED: 2026-04-24 fetch] [ASSUMED: `ErrorCode(pub i32)` tuple struct 구조 — planner는 `cargo doc -p rmcp` 또는 실제 import 후 확정 필요]

**Agent가 구조화된 data를 읽을 수 있도록:**
- `message` 필드: 사람 읽기 좋은 영어 문장
- `data` 필드: `{ code: "unimplemented", tool: "...", phase: N, hint: "..." }`

통합 테스트가 이 data 객체를 deserialize해서 `phase == 2`까지 assert하도록 작성.

## Config Loading Pattern

### `config.toml` 기본 스키마 (Phase 1)

```toml
# config.example.toml
[logging]
level = "info"   # trace | debug | info | warn | error

# Phase 2+ will add:
# [state]
# [evm]
# [policy]
# [signer]
```

### Loader

```rust
// crates/executor-mcp/src/config.rs
use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]     // Phase 2+에서 필드 추가 시 test가 잡게
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

impl Default for Config {
    fn default() -> Self { Self { logging: LoggingConfig::default() } }
}

/// 우선순위: --config CLI arg > EXECUTOR_CONFIG env > ./config.toml > 내장 default.
pub fn load() -> Result<Config> {
    let path_from_cli = std::env::args()
        .skip(1)
        .zip(std::env::args().skip(2))
        .find_map(|(flag, val)| (flag == "--config").then_some(val));
    let path_from_env = std::env::var("EXECUTOR_CONFIG").ok();
    let path_default  = std::path::PathBuf::from("config.toml");

    let path = path_from_cli
        .or(path_from_env)
        .map(std::path::PathBuf::from)
        .or_else(|| path_default.exists().then_some(path_default));

    match path {
        None => Ok(Config::default()),
        Some(p) => {
            let text = std::fs::read_to_string(&p)
                .with_context(|| format!("reading {}", p.display()))?;
            toml::from_str::<Config>(&text)
                .with_context(|| format!("parsing {}", p.display()))
        }
    }
}
```

### Logging init

```rust
// crates/executor-mcp/src/logging.rs
use anyhow::Result;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

pub fn init(cfg: &crate::config::Config) -> Result<()> {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&cfg.logging.level));
    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer().with_writer(std::io::stderr))   // CRITICAL: stderr only
        .init();
    Ok(())
}
```

> `with_writer(std::io::stderr)`가 D-05의 핵심. 이 한 줄이 누락되면 tracing은 기본으로 stdout에 쓴다 = JSON-RPC 파괴. [CITED: tracing_subscriber docs + counter_stdio.rs pattern]

## Known Pitfalls Specific to Phase 1

### Pitfall 1: Tracing 기본 writer가 stdout이라 stdio MCP를 깨뜨린다
**출처:** PITFALLS.md "Stdio logging bug"
**무엇:** `tracing_subscriber::fmt()` 기본 layer는 stdout에 쓴다. rmcp stdio transport도 stdout에 쓴다. 두 스트림이 섞이면 JSON-RPC 파서가 즉시 실패.
**왜:** `fmt::layer()` default writer = `std::io::stdout`. 아무도 의식하지 않으면 기본값이 stdout.
**어떻게 피하는가:** `.with_writer(std::io::stderr)` 명시 + 통합 테스트에서 stdout 라인별 JSON 파싱 assertion. 두 층 모두 필요.
**경고 신호:** `tools/list` 응답이 깨진 JSON으로 도착 / MCP 클라이언트가 "unexpected token" 에러.

### Pitfall 2: `println!` / `eprintln!` / `dbg!` 실수 유출
**무엇:** 평범한 디버그 습관이 stdio 규율을 깨뜨림. 특히 `dbg!`는 stderr이긴 하지만 D-05가 metadata noise 금지까지 커버.
**피하는 법:** workspace `[workspace.lints.clippy]`의 `print_stdout = "deny"`, `print_stderr = "deny"`, `dbg_macro = "deny"` + `executor-mcp/src/lib.rs` 최상단 `#![deny(clippy::print_stdout, clippy::print_stderr, clippy::dbg_macro)]`. CI에서 `cargo clippy --workspace -- -D warnings`.

### Pitfall 3: Tool schema가 macro 뒤에서 조용히 바뀐다
**무엇:** `Parameters<T>`에 들어가는 T가 tool 스키마 계약이다. 누가 필드 이름 바꾸면 agent쪽 integration이 깨진다.
**피하는 법:** `executor-core::schema::*` 구조체 각각에 대해 "snapshot test" — `schemars::schema_for!(T)`를 JSON으로 직렬화 → 저장된 golden 파일과 비교. tests가 스키마 diff를 잡게 한다. (Phase 1 task로 명시 권장.)

### Pitfall 4: `#[tool_router]` macro의 fn visibility 요구사항
**무엇:** `#[tool]`-attributed 함수는 method여야 하며, `&self` 또는 `&mut self` 시그니처 필요. `fn`(self 없음)이면 macro 컴파일 실패.
**피하는 법:** 조회성 tool(`strategy_list`, `policy_get`)도 `&self` 유지 — 상태는 Phase 1에서 안 읽더라도.

### Pitfall 5: `ServerCapabilities::builder().enable_prompts()` 안 부르면 prompt가 안 보인다
**무엇:** `get_info()`의 ServerCapabilities에 `enable_prompts()` / `enable_resources()` 누락 시 client는 `prompts/list`나 `resources/list`를 호출하지 않는다 (spec상 서버가 capability 선언 안 했으므로).
**피하는 법:** counter.rs 패턴대로 `.enable_tools().enable_prompts().enable_resources()` 세 개 모두 호출.

### Pitfall 6: `#[tool_handler]` / `#[prompt_handler]`가 `impl ServerHandler` 블록에 함께 있어야 한다
**무엇:** macro가 `ServerHandler::list_tools` / `call_tool` / `list_prompts` / `get_prompt`의 구체 impl을 이 블록에 생성한다. 같은 impl 안에서 `list_resources` / `read_resource` / `list_resource_templates`도 직접 작성해야 한다 (hand-written + macro-generated가 하나의 impl 블록 안에 공존).
**피하는 법:** counter.rs L208-294의 정확한 레이아웃을 그대로 쓰기 — 하나의 `impl ServerHandler for ExecutorServer` 블록에 `#[tool_handler]` + `#[prompt_handler]` 붙이고, 그 안에 `get_info`/`initialize`/`list_resources`/`list_resource_templates`/`read_resource`를 손으로 쓴다. [CITED: counter.rs L208-294]

### Pitfall 7: Tool/Prompt name collision
**무엇:** 두 tool이 같은 name을 쓰면 macro가 런타임에 한쪽만 라우팅. 컴파일은 통과할 수 있다.
**피하는 법:** `#[tool(name = "...")]` attribute를 항상 명시 + Phase 1 unit test: `Counter::tool_router().list_all().len() == 8` 같은 전체 등록 카운트 assertion.

### Pitfall 8: schemars 1.x와 0.8 혼용
**무엇:** rmcp 1.5가 `schemars = "1.0"` 요구하는데, 사용자 crate가 `schemars = "0.8"` 쓰면 trait impls 비호환.
**피하는 법:** workspace 수준에서 `schemars = "1.2"` 단일 버전 잠금. `cargo tree | grep schemars`로 중복 확인.

### Pitfall 9: Spawn test에서 stderr이 안 drain되어 pipe block
**무엇:** 통합 테스트가 child process의 stdin/stdout만 들고 stderr을 흘려보내지 않으면, log가 많이 쌓일 때 OS 파이프 버퍼가 차서 child가 block될 수 있다.
**피하는 법:** `Command::stderr(Stdio::piped())` 후 별도 tokio task로 `tokio::io::copy(&mut child_stderr, &mut tokio::io::sink())`. 또는 test 중 `RUST_LOG=error`로 로그 최소화.

### Pitfall 10: JSON-RPC framing — Content-Length headers?
**무엇:** LSP는 `Content-Length` 헤더 framing이지만 MCP stdio는 **단순 line-delimited JSON**이다. 한 줄 = 한 메시지. 혼동하면 파서 잘못 짠다.
**피하는 법:** MCP 2025-11-25 transports spec 명시 — "Messages are delimited by newlines, and **MUST NOT** contain embedded newlines". 통합 테스트 파서는 `BufReader::lines()` 하나면 끝. [CITED: MCP spec 2025-11-25 basic/transports]

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Tool dispatch table | 수동 `match tool_name` | `#[tool_router]` + `#[tool_handler]` | SDK가 macro로 name-to-fn 맵, schema 주입, `list_tools` 전부 생성. 수동 route는 schema 누락/이름 오타 가능. |
| JSON Schema 생성 | 손으로 `serde_json::json!({...})` | `schemars::JsonSchema` derive | 2020-12 Draft 정확, required 자동 계산, `#[serde(skip_serializing_if)]` 반영. |
| JSON-RPC framing | `serde_json::Deserializer::from_reader` raw | `rmcp::transport::stdio()` | rmcp가 cancellation, progress, ping, shutdown 모두 처리. |
| Prompt argument 선언 | 수동 `PromptArgument` vec | `#[prompt]` + `Parameters<Args>` | `required`/`description` 자동 추출. |
| MCP error type | 커스텀 enum | `rmcp::ErrorData` (`McpError`) | JSON-RPC 2.0 error shape 스펙 준수 + `data` payload 자유. |
| Stdout lock | `io::Stdout::lock()` custom | rmcp transport | transport가 stdout에 **오직** JSON-RPC만 쓴다 — lint + test로 보강. |
| Config loader | 커스텀 parser | `serde` + `toml` crate | `#[serde(default)]`, `deny_unknown_fields`. |

**핵심 insight:** Phase 1은 `counter.rs` 파일 하나의 구조 지도를 그대로 재생산하되 도메인 이름만 바꾸는 작업이다. 자체 dispatch/schema/route logic을 단 한 줄도 쓰지 말 것.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (Rust 2024 edition stable) + tokio test harness (`#[tokio::test]`) |
| Config file | 각 crate의 `Cargo.toml` `[dev-dependencies]` — 별도 test runner 없음 |
| Quick run command | `cargo test -p executor-mcp --lib` (unit, <5s) |
| Full suite command | `cargo test --workspace --all-features && cargo clippy --workspace --all-targets -- -D warnings` |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| MCP-01 | stdio 서버가 non-MCP를 stdout에 쓰지 않는다 | integration | `cargo test -p executor-mcp --test stdio_handshake stdout_is_pure_jsonrpc` | ❌ Wave 0 |
| MCP-01 | `#![deny(clippy::print_stdout, ...)]` lint CI | lint/integration | `cargo clippy --workspace --all-targets -- -D warnings` | ❌ Wave 0 (설정 파일) |
| MCP-02 | `tools/list`가 8개 tool + JSON Schema 반환 | integration | `cargo test -p executor-mcp --test stdio_handshake tools_list_returns_full_surface` | ❌ Wave 0 |
| MCP-02 | 쓰기성 tool 호출 시 Unimplemented 에러 data 반환 | integration | `cargo test -p executor-mcp --test stdio_handshake write_tools_return_unimplemented` | ❌ Wave 0 |
| MCP-02 | 각 tool input struct의 스키마가 snapshot과 일치 | unit | `cargo test -p executor-core schema_snapshots` | ❌ Wave 0 |
| MCP-03 | `resources/list`는 빈 배열, `resources/templates/list`는 세 URI 스킴 반환 | integration | `cargo test -p executor-mcp --test stdio_handshake resource_surface_declared` | ❌ Wave 0 |
| MCP-03 | `resources/read` 임의 URI는 `-32002` resource_not_found | integration | `cargo test -p executor-mcp --test stdio_handshake read_resource_returns_not_found` | ❌ Wave 0 |
| MCP-04 | `prompts/list`가 두 prompt + arguments 스키마 반환 | integration | `cargo test -p executor-mcp --test stdio_handshake prompts_list_returns_two_prompts` | ❌ Wave 0 |
| MCP-04 | `prompts/get` 호출 시 placeholder PromptMessage 반환 | integration | `cargo test -p executor-mcp --test stdio_handshake get_prompt_returns_placeholder` | ❌ Wave 0 |

### Integration Test Harness Shape

통합 테스트는 바이너리를 spawn하고 line-delimited JSON-RPC로 대화한다.

```rust
// crates/executor-mcp/tests/stdio_handshake.rs
use anyhow::Result;
use serde_json::{json, Value};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::time::{Duration, timeout};

struct ServerProc {
    child: Child,
    stdin: tokio::process::ChildStdin,
    stdout: BufReader<tokio::process::ChildStdout>,
}

async fn spawn_server() -> Result<ServerProc> {
    let bin = env!("CARGO_BIN_EXE_executor-mcp");
    let mut child = Command::new(bin)
        .env("RUST_LOG", "error")  // stderr 소음 최소화
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    // drain stderr — 파이프 block 방지
    let stderr = child.stderr.take().unwrap();
    tokio::spawn(async move {
        let mut r = BufReader::new(stderr);
        let mut buf = String::new();
        while r.read_line(&mut buf).await.unwrap_or(0) > 0 { buf.clear(); }
    });

    let stdin = child.stdin.take().unwrap();
    let stdout = BufReader::new(child.stdout.take().unwrap());
    Ok(ServerProc { child, stdin, stdout })
}

async fn send(proc: &mut ServerProc, msg: Value) -> Result<()> {
    let line = serde_json::to_string(&msg)? + "\n";
    proc.stdin.write_all(line.as_bytes()).await?;
    proc.stdin.flush().await?;
    Ok(())
}

async fn recv(proc: &mut ServerProc) -> Result<Value> {
    let mut line = String::new();
    timeout(Duration::from_secs(5), proc.stdout.read_line(&mut line)).await??;
    // KEY ASSERTION: 모든 stdout 라인이 JSON-RPC 파싱 성공해야 한다
    let v: Value = serde_json::from_str(line.trim_end())
        .map_err(|e| anyhow::anyhow!("stdout line is not JSON-RPC: {:?} — line={:?}", e, line))?;
    assert_eq!(v.get("jsonrpc").and_then(Value::as_str), Some("2.0"),
        "message missing jsonrpc: 2.0");
    Ok(v)
}

async fn initialize(proc: &mut ServerProc) -> Result<Value> {
    send(proc, json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "phase1-test", "version": "0" }
        }
    })).await?;
    let res = recv(proc).await?;
    send(proc, json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    })).await?;
    Ok(res)
}

#[tokio::test]
async fn stdout_is_pure_jsonrpc() -> Result<()> {
    let mut proc = spawn_server().await?;
    let _ = initialize(&mut proc).await?;

    // tools/list 요청 후 응답도 JSON-RPC여야 함
    send(&mut proc, json!({ "jsonrpc":"2.0","id":2,"method":"tools/list" })).await?;
    let r = recv(&mut proc).await?;
    assert_eq!(r["id"], 2);
    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn tools_list_returns_full_surface() -> Result<()> {
    let mut proc = spawn_server().await?;
    let _ = initialize(&mut proc).await?;
    send(&mut proc, json!({ "jsonrpc":"2.0","id":2,"method":"tools/list" })).await?;
    let r = recv(&mut proc).await?;
    let tools = r["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for expected in [
        "strategy_register","strategy_list","strategy_get","strategy_delete",
        "strategy_run_once","execution_get","policy_get","policy_update",
    ] {
        assert!(names.contains(&expected), "missing tool: {expected}");
    }
    // 스키마 존재 확인
    for t in tools {
        assert!(t.get("inputSchema").is_some(), "tool {} missing inputSchema", t["name"]);
    }
    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn write_tools_return_unimplemented() -> Result<()> {
    let mut proc = spawn_server().await?;
    let _ = initialize(&mut proc).await?;
    send(&mut proc, json!({
        "jsonrpc":"2.0","id":2,"method":"tools/call",
        "params":{"name":"strategy_register","arguments":{"name":"x","source":"//"}}
    })).await?;
    let r = recv(&mut proc).await?;
    let err = &r["error"];
    assert_eq!(err["data"]["code"], "unimplemented");
    assert_eq!(err["data"]["phase"], 2);
    proc.child.kill().await?;
    Ok(())
}
```

> **핵심 assertion:** `recv()` 안의 `serde_json::from_str` 실패 = `MCP-01` 위반 즉시 감지. `println!`/`eprintln!`이 어쩌다 유출되면 이 라인이 바로 터진다. [CITED: MCP 2025-11-25 transports spec — "messages delimited by newlines, MUST NOT contain embedded newlines"]

### Stdout Discipline Enforcement

**두 층위 (D-05):**

1. **Clippy lint** — 컴파일 타임에 차단
   ```toml
   # workspace Cargo.toml
   [workspace.lints.clippy]
   print_stdout = "deny"
   print_stderr = "deny"
   dbg_macro    = "deny"
   ```
   각 crate `src/lib.rs` / `src/main.rs` 최상단 이중 방어:
   ```rust
   #![deny(clippy::print_stdout, clippy::print_stderr, clippy::dbg_macro)]
   ```
   CI: `cargo clippy --workspace --all-targets --all-features -- -D warnings`

2. **Runtime assertion** — 통합 테스트에서 모든 stdout 라인이 JSON-RPC 2.0 파싱 성공 + `jsonrpc: "2.0"` 필드 존재.

### Schema Contract Stability Test

```rust
// crates/executor-core/tests/schema_snapshots.rs
use executor_core::schema::strategy::*;
use schemars::schema_for;

fn assert_schema_matches_golden(name: &str, schema: impl serde::Serialize) {
    let actual = serde_json::to_string_pretty(&schema).unwrap();
    let path = format!("tests/schemas/{name}.json");
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("missing golden file: {path}. Run UPDATE_SCHEMAS=1 to create."));
    if std::env::var("UPDATE_SCHEMAS").is_ok() {
        std::fs::write(&path, &actual).unwrap();
        return;
    }
    assert_eq!(actual.trim(), expected.trim(),
        "schema drift for {name}. Review and run UPDATE_SCHEMAS=1 if intentional.");
}

#[test]
fn strategy_register_input_schema_stable() {
    assert_schema_matches_golden("StrategyRegisterInput", schema_for!(StrategyRegisterInput));
}
```

> 모든 tool input struct + 두 prompt args struct에 대해 golden 파일 유지. Phase 2+가 필드 추가하려면 golden 업데이트 = 의식적인 계약 변경 행위.

### Sampling Rate
- **Per task commit:** `cargo test -p executor-mcp --lib && cargo clippy -p executor-mcp -- -D warnings`
- **Per wave merge:** `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`
- **Phase gate:** 위 full suite + Claude Desktop 수동 smoke test

### Wave 0 Gaps
- [ ] `crates/executor-mcp/tests/stdio_handshake.rs` — MCP-01~04 integration (신규)
- [ ] `crates/executor-core/tests/schema_snapshots.rs` + `tests/schemas/*.json` — MCP-02 schema stability (신규)
- [ ] Workspace `Cargo.toml` `[workspace.lints.clippy]` 설정 — MCP-01 lint (신규)
- [ ] `Cargo.toml` workspace + 4 crate skeleton — 전체 (신규)
- [ ] `rust-toolchain.toml` — 2024 edition pin (신규)
- [ ] CI workflow (`.github/workflows/ci.yml`) 실행 `cargo test --workspace` + `cargo clippy -- -D warnings` — 방어 3층 (선택: Phase 1에 포함할지 discretion)

## Code Examples

### 전체 `executor-mcp/src/main.rs` 최소형

```rust
// Source pattern: rust-sdk examples/servers/src/counter_stdio.rs
#![deny(clippy::print_stdout, clippy::print_stderr, clippy::dbg_macro)]

use anyhow::Result;
use executor_mcp::{ExecutorServer, config, logging};
use rmcp::{ServiceExt, transport::stdio};

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

### `executor-core/src/schema/strategy.rs` 예시

```rust
// Source pattern: rust-sdk examples/servers/src/common/counter.rs L35-54
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "Input for strategy_register (Phase 2).")]
pub struct StrategyRegisterInput {
    /// Human-readable strategy name
    pub name: String,
    /// JavaScript source (runs in sandbox in Phase 3)
    pub source: String,
    /// Arbitrary metadata
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct StrategyIdInput {
    pub strategy_id: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct StrategyRunOnceInput {
    pub strategy_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ExecutionIdInput {
    pub execution_id: String,
}
```

### `executor-core/src/schema/policy.rs` placeholder

```rust
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "Policy model — fields finalized in Phase 5.")]
pub struct PolicyModel {
    #[serde(default)] pub chains: Vec<u64>,
    #[serde(default)] pub targets: Vec<String>,
    #[serde(default)] pub selectors: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_native_value_wei: Option<String>, // u256 as decimal string
    #[serde(default)] pub allow_raw_calls: bool,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct PolicyUpdateInput {
    pub policy: PolicyModel,
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| MCP 1.0 / 2024-11-05 initial spec | 2025-11-25 spec (latest stable) | 2025-11-25 | Resource templates, elicitation, completion, task manager 지원. rmcp 1.5가 모두 지원. |
| schemars 0.8 (draft-07) | schemars 1.2 (2020-12 Draft) | schemars 1.0 release (2024-12) | rmcp 1.5가 1.x 요구. 0.8은 호환 안 됨. |
| rmcp 초기 manual handler impl | rmcp 1.x `#[tool_router]` / `#[prompt_router]` macro | rmcp 1.0 series | Boilerplate 70~80% 감소 + schema 자동 emission. |
| Rust 2021 edition | Rust 2024 edition | stable 1.85 (2025-02) | rmcp 자체가 `edition = "2024"`. 우리도 맞추기. |

**Deprecated/outdated:**
- MCP 2024-11-05 스펙: rmcp `ProtocolVersion::V_2024_11_05` 상수는 남아있지만 2025-11-25이 현행. `ProtocolVersion::V_2025_11_25` 사용. [ASSUMED: 상수 이름 — planner는 실제 `use rmcp::model::ProtocolVersion;` 후 IDE/rustdoc에서 확정]
- 직접 stdout 쓰기 (`println!`): rmcp 예제도 `eprintln!`만 사용. stdio server에서는 `tracing` 만.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | `ProtocolVersion::V_2025_11_25` 상수명이 rmcp 1.5에 존재 | rmcp 1.5 API Reference (get_info) | Planner가 실제 import로 확정 — 없으면 `ServerInfo::new(...)`만 쓰고 `.with_protocol_version` 생략 (SDK default 사용). 별 문제 아님. |
| A2 | `ErrorCode`는 `pub struct ErrorCode(pub i32)` tuple struct | Unimplemented Error Shape | API가 builder만 노출할 가능성 — 그 경우 `McpError::internal_error` 사용 + `data.code` 필드로 구분. 기능적 영향 없음. |
| A3 | `ResourceTemplate`에 `uri_template`/`name`/`description`/`mime_type` 필드 존재 + `Default` 파생 | rmcp 1.5 API Reference (list_resource_templates) | 필드 이름이 camelCase라면 serde rename 처리 되어 있을 것. 실제 struct inspection으로 Wave 0에서 확정. |
| A4 | `toml` 0.8이 현재 안정 메이저 | Standard Stack | 실제 버전 확인: `cargo add toml` 시 자동으로 최신. 위험 낮음. |
| A5 | `[workspace.lints.clippy]` 선언이 2024 edition에서 의도대로 작동 | Cargo Workspace Layout | Rust 1.74+ 지원 사양. 매우 낮은 위험. [CITED: Cargo book] 그러나 전파 규칙이 crate opt-in이므로 각 crate `Cargo.toml`에 `[lints] workspace = true` 추가 필요 — 실제 코드에서 확정. |
| A6 | PromptMessage API 시그니처 (`::new_text`, `PromptMessageRole::User`) | Prompt 등록 패턴 | counter.rs에서 검증됨 [CITED]. 그러나 1.5 구체 시그니처 변동 없는지 planner가 확인. |
| A7 | `CallToolResult::success(vec![Content::text(...)])` — `Content::text` 생성자 | Tool 등록 패턴 | counter.rs에서 검증됨 [CITED: counter.rs L84-86]. 안전. |

## Open Questions (RESOLVED)

1. **CI 워크플로우를 Phase 1 scope에 넣을까?**
   - RESOLVED: Phase 1 scope에서 **deprioritize**. workspace `[workspace.lints.clippy]` + crate-level `#![deny]` + `cargo clippy --workspace -- -D warnings` 로컬 통과만으로 MCP-01 완결로 간주. `.github/workflows/ci.yml`은 Phase 7 문서/테스트 정리 단계 또는 별도 chore 작업으로 미룸.

2. **`list_resources` 빈 배열 vs 생략 응답?**
   - RESOLVED: **빈 배열** + `next_cursor: None` + `meta: None`. MCP 2025-11-25 스펙이 `resources` capability 선언 시 `resources/list`를 필수 지원으로 규정하므로 빈 배열이 스펙 부합. counter.rs 패턴 그대로 채택.

3. **Config path 발견 우선순위**
   - RESOLVED: CLI `--config` > env `EXECUTOR_CONFIG` > cwd `./config.toml` > 내장 default(`[logging] level = "info"`) 순서. 모든 source 부재 시 내장 default로 무중단 부팅. 위 `## Config Loading Pattern` 섹션의 `config.rs` 예제가 이 순서를 구현.

4. **rmcp 1.5 `ErrorCode` 생성 패턴 (Plan 02 fallback 명시)**
   - RESOLVED (planner-side fallback): Plan 02는 우선 `ErrorCode(-32010)` tuple 생성자를 시도하고, public이 아닐 경우 `McpError::internal_error("unimplemented", Some(json!({...})))` + `data.code: "unimplemented"` 필드로 의미 분기를 보존한다. agent는 `data.code` 문자열로 판별하므로 wire-level 변별성은 유지. 정확한 wire `code` 값은 Plan 02 Wave 2 시작 시 `cargo doc --open -p rmcp`로 5분 내 확정 후 PLAN.md done에 기록.

5. **`PromptRouter::new()` / `ResourceTemplate::default()` 가용성 (Plan 02/03 fallback 명시)**
   - RESOLVED (planner-side fallback): Plan 02 Task 2는 `prompt_router: PromptRouter::new()`를 시도하되 private이면 빈 `#[prompt_router] impl Server { }` stub을 Plan 02에 미리 두고 Plan 03에서 prompt 함수만 추가. Plan 03 Task 1의 `ResourceTemplate { ..Default::default() }`는 Default 미구현 시 모든 필드 명시 (uri_template / name / description / mime_type / annotations 등 — counter.rs와 docs.rs `model` 모듈 1차 확인 후 Wave 3 시작 시 필드 리스트를 PLAN.md done에 기록).

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| `rustc` stable + 2024 edition 지원 | 전체 workspace | 확인 필요 | ≥ 1.85 권장 | — (blocker) |
| `cargo` | 빌드/테스트 | `rustc`와 묶음 | 매칭 | — |
| Claude Desktop (수동 smoke test) | Phase gate manual check | 사용자 machine에 있음 가정 | — | MCP Inspector (`npx @modelcontextprotocol/inspector`)로 대체 가능 |
| `git` | 커밋 관리 | Yes (확인됨) | — | — |

**Missing dependencies with no fallback:** 없음 — Phase 1은 순수 Rust.
**Missing dependencies with fallback:** Claude Desktop이 없으면 MCP Inspector로 tools/list 육안 검증.

**Rust 2024 edition 요구사항:** `rust-toolchain.toml`의 `channel = "stable"`은 1.85 이상일 때 자동 해결. 사용자가 구버전이면 `rustup update stable`로 해결.

## Sources

### Primary (HIGH confidence)
- **rust-sdk** main branch — `examples/servers/src/common/counter.rs` — https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/examples/servers/src/common/counter.rs [fetched 2026-04-24]
- **rust-sdk** main — `examples/servers/src/counter_stdio.rs` — https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/examples/servers/src/counter_stdio.rs [fetched 2026-04-24]
- **rust-sdk** main — `examples/servers/src/structured_output.rs` — https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/examples/servers/src/structured_output.rs [fetched 2026-04-24]
- **rust-sdk** main — `examples/servers/Cargo.toml` — https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/examples/servers/Cargo.toml [fetched 2026-04-24]
- **rust-sdk** main — `crates/rmcp/Cargo.toml` — https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/crates/rmcp/Cargo.toml [fetched 2026-04-24]
- **crates.io API** — rmcp 1.5.0, schemars 1.2.1 verified current
- **docs.rs** rmcp 1.5.0 — `ServerHandler` trait, `ErrorData` struct, `model` module [fetched 2026-04-24]
- **MCP 2025-11-25 spec** — `/server/resources` page — URI template + error code -32002 for resource_not_found

### Secondary (MEDIUM confidence)
- `.planning/research/STACK.md` / `ARCHITECTURE.md` / `PITFALLS.md` — 프로젝트 내부 research briefs
- MCP 2025-11-25 spec `/basic/transports`, `/basic/lifecycle`, `/server/tools`, `/server/prompts`

### Tertiary (LOW confidence)
- `toml` 0.8 현재 안정 버전 [ASSUMED — cargo add 시 자동 해결]
- `[workspace.lints.clippy]` 2024 edition 전파 동작 [ASSUMED A5 — low risk]

## Metadata

**Confidence breakdown:**
- Standard stack (versions): HIGH — crates.io + rmcp Cargo.toml로 교차 검증
- Architecture patterns: HIGH — counter.rs 전체 소스 직접 인용
- rmcp API Reference: HIGH (핵심 shape) / MEDIUM (세부 field names 일부 assumed)
- Unimplemented error shape: MEDIUM — docs.rs API summary 기반, `ErrorCode` tuple struct 가정 존재
- Pitfalls: HIGH — SDK 예제 + 프로젝트 PITFALLS.md + MCP transport spec 결합
- Validation architecture: HIGH — 통합 테스트 하니스가 counter.rs의 `test_client_enqueues_long_task` 패턴 직접 차용

**Research date:** 2026-04-24
**Valid until:** 2026-05-24 (rmcp 1.x 메이저 버전 유지되는 동안; 1.6+가 나와도 core API는 안정. crate 버전만 업데이트)

---

## RESEARCH COMPLETE
