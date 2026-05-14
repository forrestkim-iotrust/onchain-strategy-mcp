//! Strategy tool input + response schemas.
//!
//! Phase 2 splits the previous `metadata: Option<Value>` into top-level
//! `description: Option<String>` + `tags: Option<Vec<String>>` (D-07a /
//! RESEARCH Open Q4 option B) and adds the response types the MCP layer
//! serializes in Plan 02-02.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(description = "Register a JavaScript strategy (content-addressed; idempotent on same bundle). \
The required `source` is the `execute` function; supplying `records` and/or `view` upgrades the strategy \
to a v1.4 self-documenting bundle so `strategy://{id}/view` returns rich state. Bundles without `records` \
or `view` retain the legacy single-function semantics and the same id-hash they had pre-v1.4.")]
pub struct StrategyRegisterInput {
    #[schemars(description = "Human-readable name; UNIQUE among non-deleted strategies.")]
    pub name: String,
    #[schemars(description = "JavaScript source — executed in a sandbox starting Phase 3. Max 256 KiB.")]
    pub source: String,
    #[schemars(description = "Optional free-form description (max 4096 chars).")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[schemars(description = "Optional tags (max 16 items, each max 64 chars).")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    /// v1.4 strategy bundle: declarative `records` schema. The runtime captures \
    /// matching action effects at confirm time so the strategy's `view` function \
    /// can read them back. See `docs://strategy-bundle` for the records DSL.
    #[schemars(description = "Optional records schema for v1.4 bundle. Array of \
{ name, on, capture } specs declaring what to capture from confirmed action effects. \
Stored as canonical JSON; max 32 KiB total.")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub records: Option<Vec<RecordSpec>>,
    /// v1.4 strategy bundle: optional `view` function source. Called by \
    /// `strategy://{id}/view` resource with `(ctx, records)`; returns any JSON. \
    /// Without it, the view resource falls back to a generic balance snapshot.
    #[schemars(description = "Optional view function JS source. Same sandbox as \
strategies; max 64 KiB.")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<String>,
    /// When true, run the registration through validation + sandbox sanity \
    /// without DB insert. Returns the would-be id and any policy/sandbox \
    /// warnings. No mutation, no idempotency token consumed.
    #[schemars(description = "If true, simulate the register (validate + sandbox sanity) \
without persisting. Returns the would-be strategy_id plus any warnings. Default false.")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dry_run: Option<bool>,
}

/// One entry of a v1.4 bundle's `records` schema. Match against confirmed \
/// action effects (`on`) and capture a set of fields into the journal \
/// (`capture`). Both fields stay loosely typed (JSON values) so the records \
/// DSL can evolve without re-baking schemas; the runtime validates shape at \
/// register time.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(description = "v1.4 strategy bundle: one records-capture spec.")]
pub struct RecordSpec {
    #[schemars(description = "Lower-case identifier used as the field key in `view(ctx, records)` (e.g. \"supply\"). Must be unique within the bundle.")]
    pub name: String,
    #[schemars(description = "Match clause selecting which confirmed actions trigger this capture. \
Object with `kind` (e.g. \"contractCall\", \"erc20Transfer\", \"log\") plus kind-specific filters \
(`target`, `selector`, `token`, `from`, `to`, `address`, `topics`).")]
    pub on: serde_json::Value,
    #[schemars(description = "Map from output field name to a capture expression string. \
Expressions resolve over `args[N]`, `logs.<Event>[<filter>].<field>`, `tx.hash|block|ts|gas_used`, \
and `view.<helper>(args)`.")]
    pub capture: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "Input referencing a registered strategy by id (used by strategy_delete).")]
pub struct StrategyIdInput {
    #[schemars(description = "Strategy id returned from `strategy_register` (lower-case hex SHA-256, 64 chars).")]
    pub strategy_id: String,
}

/// Phase-3 input for the `strategy_run` MCP tool. Replaces the Phase-1
/// `StrategyRunOnceInput` placeholder; the alias below preserves the old
/// name for one phase to soften the rename.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(description = "Execute a registered JavaScript strategy once in a sandbox.")]
pub struct StrategyRunInput {
    #[schemars(description = "Strategy id (lower-case hex SHA-256, 64 chars).")]
    pub strategy_id: String,
}

/// Deprecated alias preserved for one phase. Phase 4 may delete it.
#[deprecated(note = "Use `StrategyRunInput` instead. The `_once` qualifier was a Phase-1 placeholder.")]
pub use StrategyRunInput as StrategyRunOnceInput;

/// XOR input for `strategy_get`: agent supplies exactly one of
/// `strategy_id` (content-addressed) or `name` (active strategies only).
///
/// Modeled as a flat struct with two optional fields instead of an
/// `#[serde(untagged)]` enum so that the emitted JSON Schema has no
/// top-level `anyOf`/`oneOf` (Anthropic's MCP `input_schema` rejects
/// top-level unions). XOR is validated in the tool handler.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StrategyGetInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strategy_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "Response for strategy_register (D-07).")]
pub struct StrategyRegisterResponse {
    pub strategy_id: String,
    pub name: String,
    pub created_at: String,
    pub already_exists: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub existing_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub existing_description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub existing_tags: Option<Vec<String>>,
    /// Surfaced when the existing row is soft-deleted (Pitfall 9).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<String>,
    /// v1.5 Track 1B: static extraction of contracts + selectors the strategy
    /// source will touch when executed. Shape:
    /// `{ "0xCONTRACT": ["selector1", ...], "_extraction": "complete"|"incomplete", "_warnings": [...] }`.
    /// On the idempotent (already_exists) path this echoes the value cached at
    /// the FIRST registration — re-deriving from the same source is identical
    /// by construction.
    #[schemars(description = "v1.5: static extraction of contracts + selectors the strategy will touch \
(regex-derived, cached at register time). Shape: `{ \"0xCONTRACT\": [\"selector\", ...], \
\"_extraction\": \"complete\"|\"incomplete\", \"_warnings\": [...] }`.")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contracts_touched: Option<serde_json::Value>,
    /// v1.5 Track 1C: derived alignment between `contracts_touched` and the
    /// **currently active** policy revision. Shape:
    /// `{ verdict: "satisfied"|"partial"|"missing"|"incomplete", missing: [...], remediation: "..." }`.
    /// Recomputed on every register call (and every resource read) — NEVER
    /// cached — because the policy can change without the strategy changing.
    #[schemars(description = "v1.5: derived alignment between contracts_touched and the active policy. \
Verdict ∈ satisfied | partial | missing | incomplete. Always recomputed against the live policy — not cached.")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_alignment: Option<serde_json::Value>,
    /// v1.8 name-anchored lineage: stable identifier preserved across
    /// re-registrations of the same name. Triggers, runs, and records attach
    /// to a lineage, not to a specific version.
    #[schemars(description = "v1.8: stable lineage identifier — preserved across re-registrations of the same name.")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lineage_id: Option<String>,
    /// v1.8: 1-based position of THIS version within its lineage. 1 means
    /// the first time `name` was registered; 2 means the second iteration,
    /// and so on. Present on Created / ReplacedVersion / AlreadyExists.
    #[schemars(description = "v1.8: 1-based version of this row within its lineage. 1 = first register, 2 = second iteration, ...")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
    /// v1.8: present when this register call replaced a previously active
    /// version of the same lineage (the old row was soft-deleted as part
    /// of the transaction). Carries which bundle parts changed so agents
    /// and UIs can render "what's new" badges.
    #[schemars(description = "v1.8: details about the version this register superseded. Present only when register replaced an existing active row of the same name.")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaced_version: Option<ReplacedVersionInfo>,
}

/// v1.8: surfaces the SCOPE of a same-name re-registration. Compares the
/// new bundle against the previously active row's bundle so agents (and
/// the UI's "what's new" badge) know what kind of change just landed.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "v1.8: details of the version a register call superseded.")]
pub struct ReplacedVersionInfo {
    /// `strategy_id` of the row that was just soft-deleted.
    pub previous_id: String,
    /// 1-based version of the row that was just superseded.
    pub previous_version: u32,
    pub previous_view_changed: bool,
    pub previous_records_changed: bool,
    pub previous_execute_changed: bool,
    /// v1.10: whether the bundle's `actions` map changed across the bump.
    /// True iff the previous and current rows have different canonical
    /// `actions_json`. False on legacy bundles that never used actions.
    #[serde(default)]
    pub previous_actions_changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "List item — note: `source` is intentionally absent (D-07a).")]
pub struct StrategyListItem {
    pub strategy_id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<String>,
    /// v1.8 name-anchored lineage: stable across version bumps. Optional
    /// for back-compat with old clients.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lineage_id: Option<String>,
    /// v1.8: 1-based version of this row within its lineage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "Response for strategy_list (D-07a).")]
pub struct StrategyListResponse {
    pub strategies: Vec<StrategyListItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "Response for strategy_get (D-07b) — includes source.")]
pub struct StrategyGetResponse {
    pub strategy_id: String,
    pub name: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<String>,
    /// v1.8: stable lineage anchor — preserved across re-registrations of
    /// the same name. Optional for back-compat with old clients.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lineage_id: Option<String>,
    /// v1.8: 1-based version of this row within its lineage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "Response for strategy_delete (D-07c) — idempotent.")]
pub struct StrategyDeleteResponse {
    pub strategy_id: String,
    pub deleted_at: String,
}
