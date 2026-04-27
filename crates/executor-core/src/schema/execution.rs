//! Execution-related input + response schemas plus the Phase-6
//! `SignedTransaction` placeholder.

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

/// All run lifecycle states (D-05). Phase 2 emits only the first four
/// (`queued`/`running`/`succeeded`/`failed`); the remaining three are
/// **future-reserved** for Phase 5 (`simulation_denied`, `policy_denied`)
/// and Phase 6 (`canceled`). They are declared here at Phase 2 to lock the
/// agent-facing schema golden once and avoid contract churn later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    /// Phase 6 — cancellation pathway not yet wired.
    Canceled,
    /// Phase 5 — simulation rejection.
    SimulationDenied,
    /// Phase 5 — policy rejection.
    PolicyDenied,
}

impl RunStatus {
    /// Only these four variants may be emitted by Phase 2 code paths (D-05c).
    /// `runs::insert_run` and `runs::update_run_status` reject the others
    /// with [`crate::error::StateError::InvalidInput`] (defined in
    /// `executor-state`, not here — this method is the gate consulted at
    /// the boundary).
    pub fn phase2_emittable(self) -> bool {
        matches!(
            self,
            Self::Queued | Self::Running | Self::Succeeded | Self::Failed
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "Response for execution_get (Phase 2 base run model).")]
pub struct ExecutionGetResponse {
    pub run_id: String,
    pub strategy_id: String,
    pub status: RunStatus,
    pub started_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
