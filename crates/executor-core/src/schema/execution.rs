//! Execution-related placeholders. Task 1 exposes only `SignedTransaction` so
//! `executor-signer` can reference it; Task 2 adds `ExecutionIdInput` with
//! the `JsonSchema` derive.

/// Placeholder. Phase 6에서 rlp 인코딩된 tx payload 필드 추가.
#[derive(Debug, Clone)]
pub struct SignedTransaction;
