---
phase: 06
slug: local-managed-execution
status: draft
nyquist_compliant: true
wave_0_complete: false
created: 2026-04-28
---

# Phase 06 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust `cargo test` |
| **Config file** | `Cargo.toml` |
| **Quick run command** | `cargo test --workspace` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run the task-specific `<automated>` command
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd-verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 06-01-01 | 06-01 | 1 | EXE-07 | T-06-01 | Signer config fails closed when env var/private key is absent or invalid | unit | `cargo test signer` | created in wave | ⬜ pending |
| 06-01-02 | 06-01 | 1 | EXE-07 | T-06-02 | Private-key material is never exposed to strategy code, journals, reports, or errors | unit | `cargo test signer` | created in wave | ⬜ pending |
| 06-02-01 | 06-02 | 2 | EXE-08 | T-06-03 | Approved transactions broadcast only after simulation and policy approval | unit/integration | `cargo test execution` | created in wave | ⬜ pending |
| 06-02-02 | 06-02 | 2 | EXE-09 | T-06-04 | Runtime waits for receipts and records confirmed/failed status, gas, tx hash, and errors | unit/integration | `cargo test execution` | created in wave | ⬜ pending |
| 06-02-03 | 06-02 | 2 | STJ-06 | T-06-05 | Failed receipts halt remaining action broadcasts and persist failed status | unit/integration | `cargo test execution` | created in wave | ⬜ pending |
| 06-03-01 | 06-03 | 3 | STJ-07 | T-06-06 | `execution_get` returns persisted execution status by run/execution ID | unit | `cargo test execution_get` | created in wave | ⬜ pending |
| 06-03-02 | 06-03 | 3 | STJ-07 | T-06-07 | `execution://{run_id}` returns the same receipt-backed report source as the tool | unit | `cargo test execution_resource` | created in wave | ⬜ pending |
| 06-03-03 | 06-03 | 3 | EXE-09 | T-06-08 | Reports include signer address, action index, tx hash, receipt status, gas used, and execution error | unit | `cargo test execution` | created in wave | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

Existing Rust test infrastructure covers all phase requirements. Add focused tests in the implementation waves before or alongside each behavior change.

---

## Manual-Only Verifications

All phase behaviors have automated verification.

---

## Threat Reference Mapping

| Validation Ref | Plan Threat Ref |
|----------------|-----------------|
| T-06-01 | T-06-01-01 |
| T-06-02 | T-06-01-04 |
| T-06-03 | T-06-02-01 |
| T-06-04 | T-06-02-03 |
| T-06-05 | T-06-02-02 |
| T-06-06 | T-06-03-01 |
| T-06-07 | T-06-03-02 |
| T-06-08 | T-06-03-03 |

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Implementation tasks cover all MISSING references
- [x] No watch-mode flags
- [x] Feedback latency < 30s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** approved 2026-04-28
