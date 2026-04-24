//! Action enum placeholder. Phase 4가 실제 variant(ContractCall, RawCall, Erc20*,
//! NativeTransfer)를 채운다. Phase 1은 enum 껍데기만 유지해 downstream crate가
//! 경로(`executor_core::schema::action::Action`)를 미리 참조할 수 있게 한다.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    // Phase 4에서 실제 variant가 채워짐. Phase 1은 enum 껍데기만.
    Noop,
    // TODO(phase-4): ContractCall, RawCall, Erc20Approve, Erc20Transfer, NativeTransfer variants
}
