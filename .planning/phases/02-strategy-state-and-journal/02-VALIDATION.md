---
phase: 02
slug: strategy-state-and-journal
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-24
---

# Phase 02 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution. Details to be filled in by planner using 02-RESEARCH.md §"Validation Architecture" and 02-CONTEXT.md D-08 testing plan.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | `cargo test` (Rust built-in, tokio `#[tokio::test]` for async) |
| **Config file** | `Cargo.toml` workspace + per-crate `[dev-dependencies]` |
| **Quick run command** | `cargo test -p executor-state` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~5-15 seconds (phase 1 baseline: 0.22s for 20 tests; phase 2 adds ~12 tests + repository-level) |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p executor-state -p executor-mcp` (targeted)
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd-verify-work`:** Full suite + `cargo clippy --workspace --all-targets -- -D warnings` must be green
- **Max feedback latency:** ~30 seconds (clippy + tests)

---

## Per-Task Verification Map

*To be populated by planner. Each task should map to one or more automated verification commands, or declare Wave 0 dependency.*

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| _pending planner fill_ | | | | | | | | | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

Planner should identify fixtures/helpers needed before main tasks. Expected candidates (per CONTEXT.md D-08):

- [ ] `crates/executor-state/tests/common/mod.rs` — shared test fixture (open `:memory:` StateStore, seed helpers)
- [ ] `crates/executor-mcp/tests/common/mod.rs` — extend with state-aware spawn helper (server with in-tempfile DB)

*Final Wave 0 set confirmed by planner.*

---

## Manual-Only Verifications

*Expected: none. All Phase 2 behaviors should have automated coverage because this is a pure persistence layer with a well-defined API surface. Planner confirms.*

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| _none expected_ | | | |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending (planner fills during plan generation; executor flips `nyquist_compliant: true` after Wave 0 lands)
