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

/// Outcome of a strategy run as recorded in `journal_actions` (D-06).
///
/// All six variants are declared at Phase-3 introduction so the schema golden
/// locks the wire shape; Phase 5 (`SimulationFailure`, `PolicyDenied`) cannot
/// trigger contract churn (D-05 future-lock pattern carry-over).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum JournalActionOutcome {
    Noop,
    Actions,
    ValidationError,
    RuntimeError,
    /// Phase 5 — simulation gate produced a failure result.
    SimulationFailure,
    /// Phase 5 — policy gate denied the action.
    PolicyDenied,
}

impl JournalActionOutcome {
    /// Phase-3 production code paths must only emit the first four (D-06
    /// future-lock). `executor-state::journal::record_action_outcome`
    /// consults this gate before INSERT and rejects reserved variants
    /// with `StateError::InvalidInput`.
    pub fn phase3_emittable(self) -> bool {
        matches!(
            self,
            Self::Noop | Self::Actions | Self::ValidationError | Self::RuntimeError
        )
    }
}

/// Strategy outcome — the success-shape part of [`StrategyRunResponse`].
/// Validation errors and runtime errors are surfaced as MCP errors, NOT as
/// `StrategyOutcome` variants (D-08).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[schemars(description = "Outcome of a successful strategy run (D-08).")]
pub enum StrategyOutcome {
    Noop,
    Actions { actions: Vec<crate::schema::action::Action> },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "Response for strategy_run (Phase 3, D-08).")]
pub struct StrategyRunResponse {
    /// ULID of the run row inserted at handler start.
    pub run_id: String,
    /// Echo of the requested strategy id.
    pub strategy_id: String,
    /// Terminal status — Phase 3 returns only on terminal status (Succeeded
    /// for normal flow; Failed paths surface as MCP errors instead).
    pub status: RunStatus,
    pub started_at: String,
    /// Always populated for a successful response — Phase 3 runs are
    /// synchronous to completion.
    pub finished_at: String,
    pub outcome: StrategyOutcome,
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
