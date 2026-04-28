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
    /// Phase 5 D-10 widening: variants emittable from Phase-2..Phase-5 code
    /// paths. `Canceled` stays reserved for Phase 6. The Phase-2 method name
    /// `phase2_emittable` is renamed to `phase5_emittable` here; callers in
    /// `runs::insert_run` / `update_run_status_with_transition` are updated
    /// in lockstep.
    pub fn phase5_emittable(self) -> bool {
        matches!(
            self,
            Self::Queued
                | Self::Running
                | Self::Succeeded
                | Self::Failed
                | Self::SimulationDenied
                | Self::PolicyDenied
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
    /// Phase 5 D-10 widening: all six variants are emittable. The previous
    /// name `phase3_emittable` (which excluded `SimulationFailure` /
    /// `PolicyDenied`) is renamed to `phase5_emittable`. Reserved-variant
    /// gating is no longer needed at this layer — Phase 5 gate orchestration
    /// (executor-mcp::tools::strategy_run) directly emits all six.
    pub fn phase5_emittable(self) -> bool {
        matches!(
            self,
            Self::Noop
                | Self::Actions
                | Self::ValidationError
                | Self::RuntimeError
                | Self::SimulationFailure
                | Self::PolicyDenied
        )
    }
}

/// Per-action gate verdict (D-11). Pairs the policy and simulation gate
/// outcomes for a single normalized action. Carried in
/// [`StrategyOutcome::Actions::decisions`] on success-path responses; failure
/// paths surface the same data via the `journal://{run_id}` resource only.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ActionDecision {
    pub action_index: u32,
    pub policy: GateVerdict,
    pub simulation: GateVerdict,
}

/// Outcome of one gate (policy or simulation) for one action. `Skipped`
/// occurs when an earlier gate denied so this gate never ran (research Q-12).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GateVerdict {
    Pass,
    Skipped,
    Fail { rule: String, detail: String },
}

/// Strategy outcome — the success-shape part of [`StrategyRunResponse`].
/// Validation errors and runtime errors are surfaced as MCP errors, NOT as
/// `StrategyOutcome` variants (D-08). Phase 5 D-11 widens `Actions` with a
/// `decisions` field carrying per-action gate verdicts.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[schemars(description = "Outcome of a successful strategy run (D-08, Phase 5 D-11).")]
pub enum StrategyOutcome {
    Noop,
    Actions {
        actions: Vec<crate::schema::action::Action>,
        #[serde(default)]
        decisions: Vec<ActionDecision>,
    },
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
