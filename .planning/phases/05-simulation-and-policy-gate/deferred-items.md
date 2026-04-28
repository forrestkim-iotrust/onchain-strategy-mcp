# Deferred Items — Phase 05

## From Plan 05-02 execution

### 8x clippy::field_reassign_with_default in `crates/executor-evm/tests/read_contract_anvil.rs`

- **Origin:** Phase 4 (pre-existing — predates this plan).
- **Lines:** 72, 73, 100, 101, 165, 166, 222, 223 (approx — `let mut cfg = EvmConfig::default(); cfg.rpc_url = ...`).
- **Why deferred:** Out of scope per executor scope-boundary rule (only auto-fix
  issues directly caused by current task). The read_contract_anvil tests are
  Phase 4 code untouched by Plan 05-02 except for the one signature update
  (3-arg `EvmConfig::from_raw`). All `read_contract_anvil` tests still pass.
- **Suggested fix (future plan):** rewrite as struct-update syntax —
  `let cfg = EvmConfig { rpc_url: ..., ..EvmConfig::default() };`
  (the same pattern used in new `simulate_anvil.rs` and `simulate_timeout.rs`).
