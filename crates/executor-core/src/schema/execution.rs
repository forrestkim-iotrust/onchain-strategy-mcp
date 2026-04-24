//! Execution-related input schemas plus the Phase-6 `SignedTransaction` placeholder.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "Input for execution_get (Phase 2 implements persistence).")]
pub struct ExecutionIdInput {
    /// Opaque execution identifier returned from a previous `strategy_run_once`.
    #[schemars(description = "Opaque execution identifier returned from a previous `strategy_run_once`.")]
    pub execution_id: String,
}

/// Placeholder. Phase 6에서 rlp 인코딩된 tx payload 필드 추가.
#[derive(Debug, Clone)]
pub struct SignedTransaction;
