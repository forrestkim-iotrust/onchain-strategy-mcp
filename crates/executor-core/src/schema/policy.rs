//! Policy input schema placeholder — Phase 5가 실제 PolicyModel로 교체한다.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "Replace current policy (Phase 5 fills out the real shape).")]
pub struct PolicyUpdateInput {
    #[schemars(description = "Opaque policy metadata accepted as a placeholder until Phase 5 finalises the policy model.")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}
