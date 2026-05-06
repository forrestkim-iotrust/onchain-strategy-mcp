# onchain-strategy-mcp — Retrospective

## Milestone: v1.0 — MVP

**Shipped:** 2026-05-04
**Phases:** 7 | **Plans:** 24 | **Tasks:** 48

### What Was Built

A local-first MCP runtime that lets an AI agent register sandboxed JavaScript strategies, run them through a controlled pipeline (validate → simulate → policy → local-signer → broadcast → receipt → journal), and inspect receipt-backed reports through `execution_get` and `execution://{run_id}`. 36/36 milestone requirements satisfied. 512 workspace tests + clippy clean. Anvil-backed `verification_examples` (3 passed) and `verification_safety` (2 passed) lock the safety boundary end to end.

### What Worked

- **Boundary-first crate split.** `executor-core` stayed pure-domain so policy, simulation, signer, EVM adapter, JS runner, journal could ship as independent crates without circular deps. Made Phase 6 signer boundary cheap.
- **Deny-by-default policy from Phase 5.** No retrofit was needed when Phase 6 added live broadcast — policy already gated everything.
- **Plan/Verification cadence.** Per-plan SUMMARY + per-phase VERIFICATION + UAT made milestone audit a 30-minute job at the end, not a week.
- **rmcp 1.5 prompt+tool symmetry on one `impl ServerHandler`.** Pitfall 6 (split impl blocks) was avoided early; Phase 1 set the pattern and later phases reused it without churn.

### What Was Inefficient

- **Phase 6 documented an "anvil + real key" human-verification step.** The Phase 7 anvil-feature suite covered the same path automatically; the human step ended up acknowledged at milestone close. Should have been written as automated from day one.
- **Vision-extracted design tokens were not authoritative.** During the design pass, GPT-4o vision extraction returned Arial/black hex values that conflicted with DESIGN.md (which mandated Barlow Condensed / `#0B0E0D` / acid green signal). Treating extracted tokens as a hint, not a source-of-truth, would save a re-write.
- **Pretext height calc set as `height: Npx` overlapped following content.** Switched to `min-height` only after the user reported visual overlap. Default to `min-height` from day one for any predicted layout.
- **Workflow's no-domain WebSearch path failed mid-session.** The `allowed_domains: []` shape returned 400; `WebFetch` against landing pages also failed with a system role error. Visual inspection through the gstack `browse` binary worked, and was faster anyway.

### Patterns Established

- **`Action[]` as the runtime contract.** Strategies propose, runtime decides. No graph engine, no opcode VM. Held up across all 24 plans.
- **Env-var-only signer reference (`private_key_env = "EXECUTOR_PRIVATE_KEY"`).** Raw keys never appear in committed config, fixtures, snapshots, or logs.
- **`build_execution_report` shared by `execution_get` and `execution://{run_id}`.** Tool and resource never drift in shape.
- **Schema goldens + stdio integration tests as the regression line.** Adding/removing a field always touches both, which catches drift.

### Key Lessons

1. **Treat extracted tokens as suggestions, not truth.** Source-of-truth = `DESIGN.md` (or equivalent) + the user's verbal overrides. Vision/automation is upstream input.
2. **Default predicted layouts to `min-height`, not `height`.** Browser-natural overflow > clipped/overlapping content when measurement is approximate.
3. **Anvil-feature CI > human-verification line items.** If a phase ends with "human needed for live RPC," move it into the next phase's automated suite immediately, not after the milestone.
4. **The wedge isn't the runtime — it's install friction.** v1.0 produced a clean runtime; v1.1's job is to make "MCP install + burner + first run" sub-5-minute. The runtime is necessary but not sufficient for adoption.

### Cost Observations

Not formally tracked across this milestone. Phase 04 had the heaviest plan-checker iteration cost (alloy 2.0 trait imports + per-crate dep pinning); Phase 05 had the longest single plan (05-01 normalization at ~25 min). Phase 06 was efficient because Phase 5 had already locked the gate boundary.

## Cross-Milestone Trends

(First milestone — trends will start populating from v1.1 forward.)

| Milestone | Phases | Plans | Tests | Workspace LOC | Time-to-first-run |
|-----------|--------|-------|-------|---------------|-------------------|
| v1.0 | 7 | 24 | 512 (54 suites) | (Rust workspace, 7 crates) | n/a (anvil-only) |
| v1.1 | (planned 4) | TBD | TBD | TBD | target ≤ 5 min |
