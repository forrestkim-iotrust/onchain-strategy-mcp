# Roadmap: onchain-strategy-mcp

## Milestones

- ✅ **v1.0 MVP** — Phases 1-7 (shipped 2026-05-04)
- 📋 **v1.1 Adoption** — distribution, burner UX, real-network starters, Quickstart, dogfood (planned)

## Phases

<details>
<summary>✅ v1.0 MVP (Phases 1-7) — SHIPPED 2026-05-04</summary>

- [x] Phase 1: MCP Runtime Surface (3/3 plans) — completed 2026-04-24
- [x] Phase 2: Strategy State and Journal (3/3 plans) — completed 2026-04-26
- [x] Phase 3: JavaScript Strategy Runner (3/3 plans) — completed 2026-04-27
- [x] Phase 4: EVM Context and Actions (4/4 plans) — completed 2026-04-27
- [x] Phase 5: Simulation and Policy Gate (5/5 plans) — completed 2026-04-28
- [x] Phase 6: Local Managed Execution (3/3 plans) — completed 2026-04-29
- [x] Phase 7: Examples, Tests, and Documentation (3/3 plans + UAT 5/5) — completed 2026-05-04

Full archive: [milestones/v1.0-ROADMAP.md](./milestones/v1.0-ROADMAP.md)
Audit: [milestones/v1.0-MILESTONE-AUDIT.md](./milestones/v1.0-MILESTONE-AUDIT.md)
Requirements: [milestones/v1.0-REQUIREMENTS.md](./milestones/v1.0-REQUIREMENTS.md)

</details>

### 📋 v1.1 Adoption (current milestone)

**Goal:** "AI로 에이전트 트레이딩 해보고 싶은데…"라고 시작하는 사람이 5분 안에 첫 receipt를 보게 만든다.

- [ ] **Phase 8: Distribution + Burner UX**
  Prebuilt binaries via GitHub Releases, one-line install script with checksum, `claude mcp add` one-liner, `osmcp init` / `osmcp burner new`, OS-keychain or 0600 keystore, threat-model docs.
  **Depends on:** v1.0 runtime
  **Requirements:** DIST-01, DIST-02, DIST-03, DIST-04, BUR-01, BUR-02, BUR-03, BUR-04
  **Success Criteria:**
  1. Anyone can install the binary from a GitHub Release with one curl command + checksum verification.
  2. `claude mcp add ...` connects the runtime to Claude Code in one line.
  3. `osmcp init` produces a working config + policy + burner keystore and prints the public address.
  4. `osmcp burner new` rotates the burner without ever echoing a raw private key.
  5. `docs/BURNER.md` documents the burner threat model.

- [ ] **Phase 9: Real-network starter strategies**
  Testnet self-transfer strategy on Base/OP Sepolia, matching testnet policy, mainnet-safe Base/Arbitrum burner policy template.
  **Depends on:** Phase 8
  **Requirements:** NET-01, NET-02, NET-03, NET-04
  **Success Criteria:**
  1. A funded burner can run the testnet self-transfer starter and see a real receipt within 60 seconds.
  2. The testnet starter policy locks chain, contract, selector, and value caps.
  3. The mainnet starter policy template restricts to USDC/WETH allowlist with a small spend cap and raw_call disabled.
  4. No starter strategy/policy contains private-key material or open-ended approvals.

- [ ] **Phase 10: Quickstart + Demo**
  README rewrite to lead with the 5-minute Quickstart, `claude mcp add` one-liner, anvil moved to docs/LOCAL-DEV.md, embedded ≤ 90s Claude Code natural-language demo.
  **Depends on:** Phase 8 + Phase 9
  **Requirements:** QSD-01, QSD-02, QSD-03, QSD-04
  **Success Criteria:**
  1. A new user can follow README's Quickstart on a fresh machine and see a testnet receipt in ≤ 5 minutes.
  2. README explicitly shows `claude mcp add` and `osmcp init` lines.
  3. A ≤ 90s natural-language demo is embedded under the Quickstart.
  4. The demo shows agent → write_evm_strategy → register → run → execution_get on testnet.

- [ ] **Phase 11: Dogfood + Run-2/Show-1**
  Hand the install playbook to 5 Claude Code users, sit/screenshare for first run, log friction points, track Run-2 and Show-1 for 72 hours, write GO/NO-GO decision.
  **Depends on:** Phase 10
  **Requirements:** DOG-01, DOG-02, DOG-03
  **Success Criteria:**
  1. 5 users complete install → first run with timing data captured.
  2. Run-2 and Show-1 are recorded for 72 hours.
  3. Phase produces an explicit GO/NO-GO writeup. Run-2 ≥ 2 AND Show-1 ≥ 1 → start v2 milestone. Otherwise re-open office-hours, do not keep building.

## Progress

| Phase | Milestone | Plans Complete | Status | Completed |
|-------|-----------|----------------|--------|-----------|
| 1. MCP Runtime Surface | v1.0 | 3/3 | Complete | 2026-04-24 |
| 2. Strategy State and Journal | v1.0 | 3/3 | Complete | 2026-04-26 |
| 3. JavaScript Strategy Runner | v1.0 | 3/3 | Complete | 2026-04-27 |
| 4. EVM Context and Actions | v1.0 | 4/4 | Complete | 2026-04-27 |
| 5. Simulation and Policy Gate | v1.0 | 5/5 | Complete | 2026-04-28 |
| 6. Local Managed Execution | v1.0 | 3/3 | Complete | 2026-04-29 |
| 7. Examples, Tests, and Documentation | v1.0 | 3/3 + UAT 5/5 | Complete | 2026-05-04 |
| 8. Distribution + Burner UX | v1.1 | 0/? | Not started | - |
| 9. Real-network starter strategies | v1.1 | 0/? | Not started | - |
| 10. Quickstart + Demo | v1.1 | 0/? | Not started | - |
| 11. Dogfood + Run-2/Show-1 | v1.1 | 0/? | Not started | - |
