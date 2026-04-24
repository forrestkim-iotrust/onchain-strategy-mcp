//! Phase 1 prompt surface — 2 placeholder prompts whose bodies are finalized in
//! Phase 7 once the strategy `ctx` API stabilizes (D-04).
//!
//! This module declares a second `impl ExecutorServer` block carrying
//! `#[prompt_router]`. The macro generates `Self::prompt_router()`, which
//! `server.rs::ExecutorServer::new()` calls to construct the router; the
//! dispatch side is handled by `#[prompt_handler(router = self.prompt_router)]`
//! on the same `impl ServerHandler` block as `#[tool_handler]` (Pitfall 6).
//!
//! Argument schemas come from
//! `executor_core::schema::prompt_args::{WriteEvmStrategyArgs, ReviewEvmStrategyArgs}`
//! via `Parameters<T>`, so `prompts/list` publishes them automatically.

use executor_core::schema::prompt_args::{ReviewEvmStrategyArgs, WriteEvmStrategyArgs};
use rmcp::{
    ErrorData as McpError, RoleServer,
    handler::server::wrapper::Parameters,
    model::{GetPromptResult, PromptMessage, PromptMessageRole},
    prompt, prompt_router,
    service::RequestContext,
};

use crate::server::ExecutorServer;

// `vis = "pub(crate)"` mirrors the `#[tool_router]` setup in `tools.rs`:
// without it the macro-generated `Self::prompt_router()` associated fn
// inherits the impl's private visibility, and `server.rs` (separate module)
// cannot call it across the module boundary (E0624).
#[prompt_router(vis = "pub(crate)")]
impl ExecutorServer {
    #[prompt(
        name = "write_evm_strategy",
        description = "Author a new EVM automation strategy. Body finalized in Phase 7 after the ctx API stabilizes."
    )]
    async fn write_evm_strategy(
        &self,
        Parameters(_args): Parameters<WriteEvmStrategyArgs>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, McpError> {
        Ok(GetPromptResult::new(vec![PromptMessage::new_text(
            PromptMessageRole::User,
            "Strategy authoring prompt — body will be finalized after ctx API stabilizes (Phase 7).",
        )])
        .with_description("Placeholder authoring prompt"))
    }

    #[prompt(
        name = "review_evm_strategy",
        description = "Review an existing EVM automation strategy for safety and correctness. Body finalized in Phase 7."
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
