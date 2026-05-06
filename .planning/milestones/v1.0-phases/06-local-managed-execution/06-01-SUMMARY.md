---
phase: 06-local-managed-execution
plan: 01
subsystem: execution
tags: [rust, alloy, signer, config, local-hot-wallet]

requires:
  - phase: 05-simulation-and-policy-gate
    provides: [policy and simulation gates before signer handoff]
provides:
  - executor-signer local signer boundary resolving private keys only from env vars
  - non-secret [signer] config parsing with receipt timeout defaults
  - Plan 06-02 contract to derive signer config at strategy_run execution boundary
checks:
  - cargo test -p executor-signer
  - cargo test -p executor-mcp signer_section
  - cargo test -p executor-mcp signer_config
  - cargo clippy -p executor-signer -p executor-mcp --all-targets -- -D warnings
affects: [phase-06-local-managed-execution, executor-signer, executor-mcp-config]

tech-stack:
  added: [alloy signer-local, alloy-primitives, thiserror, tokio, serde]
  patterns:
    - env-var-reference-only signer config
    - stable non-secret signer error taxonomy
    - execution-boundary signer resolution

key-files:
  created:
    - crates/executor-signer/src/config.rs
    - crates/executor-signer/src/error.rs
    - crates/executor-signer/src/local.rs
    - crates/executor-signer/tests/local_signer.rs
  modified:
    - Cargo.lock
    - crates/executor-signer/Cargo.toml
    - crates/executor-signer/src/lib.rs
    - crates/executor-mcp/src/config.rs

key-decisions:
  - "Signer config stores only an environment-variable name; private-key values are read only by LocalSignerHandle::from_env."
  - "Config::signer_config returns None when absent and does not touch ExecutorServer constructors."
  - "Alloy PrivateKeySigner with with_chain_id(Some(chain_id)) is the only signing primitive introduced in this plan."

patterns-established:
  - "LocalSignerHandle Debug exposes signer address metadata but never raw key material."
  - "SignerSection has no production private-key default and defaults receipt_timeout_ms to 120000."

requirements-completed: [EXE-07]

duration: 6 min
completed: 2026-04-28
---

# Phase 06 Plan 01: Signer Boundary and Local Private-Key Signer Summary

**Fail-closed local signer boundary using Alloy PrivateKeySigner plus non-secret MCP signer config parsing**

## Performance

- **Duration:** 6 min
- **Started:** 2026-04-28T15:55:55Z
- **Completed:** 2026-04-28T16:01:58Z
- **Tasks:** 2/2
- **Files modified:** 8

## Accomplishments

- Added `executor-signer` modules for `LocalSignerConfig`, `SignerError`, and `LocalSignerHandle`.
- Implemented fail-closed env-var signer resolution for missing env names, absent env vars, and invalid private keys without including raw values in errors.
- Added `[signer]` config parsing in `executor-mcp` with optional `private_key_env`, `receipt_timeout_ms = 120_000`, and `Config::signer_config()` conversion.
- Preserved the revised plan constraint: no edits to `ExecutorServer` constructors or `crates/executor-mcp/src/server.rs`.

## Task Commits

1. **Task 1: Add signer crate dependencies and non-secret local signer boundary** - `7e4174b` (feat)
2. **Task 2: Add [signer] config parsing without touching ExecutorServer constructors** - `2efdd98` (feat)

**Plan metadata:** committed separately after this summary.

## Files Created/Modified

- `Cargo.lock` - Locks Alloy local signer transitive dependencies for `executor-signer`.
- `crates/executor-signer/Cargo.toml` - Adds Alloy signer-local, primitives, serde, thiserror, and tokio dependencies.
- `crates/executor-signer/src/lib.rs` - Re-exports signer config, error, and local handle modules.
- `crates/executor-signer/src/config.rs` - Defines non-secret `LocalSignerConfig`.
- `crates/executor-signer/src/error.rs` - Defines stable non-secret signer error taxonomy.
- `crates/executor-signer/src/local.rs` - Resolves `PrivateKeySigner` from env and derives signer address.
- `crates/executor-signer/tests/local_signer.rs` - Covers missing, absent, invalid, and valid signer env behavior.
- `crates/executor-mcp/src/config.rs` - Adds `[signer]` parsing and `Config::signer_config()` tests.

## Decisions Made

- Kept private-key resolution exclusively inside `LocalSignerHandle::from_env`, not config parsing.
- Used Alloy `PrivateKeySigner::from_str(...).with_chain_id(Some(chain_id))` instead of custom cryptography.
- Avoided `ExecutorServer` constructor plumbing per revised plan and GitNexus change-risk guidance.

## GitNexus Impact Analysis

- `Signer` upstream impact: LOW, no direct callers or affected processes.
- New signer symbols (`LocalSignerConfig`, `LocalSignerHandle`, `SignerError`) were not yet in the stale index before creation.
- `Config` upstream impact: LOW, no indexed upstream dependants.
- `load` upstream impact: LOW, one direct caller (`crates/executor-mcp/src/main.rs:main`) and one affected process.
- GitNexus change detection reported no indexed changes because the project index is stale/read-only in this worktree; direct `git status` and verification commands were used to scope changes.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Imported Alloy Signer trait for `with_chain_id`**
- **Found during:** Task 1
- **Issue:** `PrivateKeySigner::with_chain_id` is provided by the Alloy `Signer` trait and was not in scope.
- **Fix:** Imported `alloy::signers::Signer as AlloySigner` in `local.rs`.
- **Files modified:** `crates/executor-signer/src/local.rs`
- **Verification:** `cargo test -p executor-signer` passed.
- **Committed in:** `7e4174b`

**2. [Rule 3 - Blocking] Avoided unsafe env mutation in Rust 2024 tests**
- **Found during:** Task 1
- **Issue:** Workspace forbids unsafe code and Rust 2024 makes direct env mutation unsafe.
- **Fix:** Moved env-value tests into child processes configured with `Command::env`, avoiding unsafe blocks.
- **Files modified:** `crates/executor-signer/tests/local_signer.rs`
- **Verification:** `cargo test -p executor-signer` passed.
- **Committed in:** `7e4174b`

**3. [Rule 1 - Bug] Corrected invalid fixture private key**
- **Found during:** Task 1
- **Issue:** The initially used fixture key was not accepted by Alloy `PrivateKeySigner`.
- **Fix:** Switched to an anvil fixture key in tests only and asserted the corresponding signer address.
- **Files modified:** `crates/executor-signer/tests/local_signer.rs`
- **Verification:** `cargo test -p executor-signer` passed; non-test acceptance scan for the rejected fixture key returned no matches.
- **Committed in:** `7e4174b`

**4. [Rule 1 - Bug] Implemented explicit `Default` for `SignerSection`**
- **Found during:** Task 2
- **Issue:** `#[derive(Default)]` set `receipt_timeout_ms` to `0`, bypassing the intended `120_000` default for absent signer config.
- **Fix:** Added an explicit `Default` implementation using `default_receipt_timeout_ms()`.
- **Files modified:** `crates/executor-mcp/src/config.rs`
- **Verification:** `cargo test -p executor-mcp signer_section && cargo test -p executor-mcp signer_config` passed.
- **Committed in:** `2efdd98`

---

**Total deviations:** 4 auto-fixed (2 blocking, 2 bug fixes)
**Impact on plan:** All fixes were required for compilation, Rust 2024 safety, valid test fixtures, or correct fail-closed timeout defaults. No architecture changes or scope expansion.

## Issues Encountered

- GitNexus emitted stale/read-only index warnings and `detect-changes` reported no indexed changes. The required impact analysis still ran for existing symbols and returned LOW risk where resolvable.

## Known Stubs

None.

## Threat Flags

None beyond the plan threat model. This plan intentionally added the signer env-var trust boundary described in T-06-01-01 through T-06-01-05.

## User Setup Required

External local signer configuration is required before managed execution can sign transactions:

- Set `[signer].private_key_env = "EXECUTOR_PRIVATE_KEY"` in runtime config.
- Set `EXECUTOR_PRIVATE_KEY` in the local operator environment to a hex EVM private key.
- Do not commit raw private keys or production config containing secret values.

## Verification

- `cargo test -p executor-signer` — passed, 5 tests.
- `cargo test -p executor-mcp signer_section` — passed, 3 matching tests.
- `cargo test -p executor-mcp signer_config` — passed, 2 matching tests.
- `cargo clippy -p executor-signer -p executor-mcp --all-targets -- -D warnings` — passed with no issues.

## Next Phase Readiness

Plan 06-02 can derive signer config at the strategy execution boundary using `crate::config::load()?.signer_config()?`, then fail closed before any signing or broadcast attempt when config is absent or invalid.

## Self-Check: PASSED

- Found `.planning/phases/06-local-managed-execution/06-01-SUMMARY.md`.
- Found `crates/executor-signer/src/config.rs`.
- Found `crates/executor-signer/src/error.rs`.
- Found `crates/executor-signer/src/local.rs`.
- Found task commit `7e4174b`.
- Found task commit `2efdd98`.

---
*Phase: 06-local-managed-execution*
*Completed: 2026-04-28*
