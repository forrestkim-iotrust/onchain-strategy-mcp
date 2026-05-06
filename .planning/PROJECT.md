# onchain-strategy-mcp

## What This Is

`onchain-strategy-mcp`는 외부 AI agent가 작성한 EVM 자동화 전략을 안전하게 실행하는 local-first MCP 런타임이다.

전략은 agent가 결정한다. 전략, venue, bridge, route, 진입/청산 조건까지 모두 agent의 책임이다. 런타임은 그 plan을 받아 검증, 시뮬레이션, policy 검사, 로컬 서명, 브로드캐스트, receipt 수집, journal 기록을 수행한다. 즉, “agent는 trade를 찾고, 런타임은 wallet 권한을 통제한다.”

v1.0에서는 sandbox JavaScript + 작은 `ctx` API + 로컬 hot-wallet signer 조합으로 이 경계를 증명했다. v1.1부터는 “주변 사람이 깔아 쓰는” 단계로 넘어간다.

이 프로젝트는 dashboard, marketplace, hosted custody, 지갑 제품, alpha generation agent가 아니다. 외부 agent가 EVM 위에서 자동화 plan을 안전하게 실행할 수 있게 하는 execution 레이어다.

## Core Value

AI agent가 EVM 자동화 plan을 실제 온체인 실행으로 바꾸되, 모든 실행은 burner-wallet 경계, simulation, policy 검사를 거치고 receipt와 journal로 남아야 한다.

## Current State (after v1.0 MVP)

**Shipped:**
- 4-crate Rust 워크스페이스 (executor-core/state/policy/signer/evm/mcp + strategy-js).
- rmcp 1.5 stdio MCP 서버: 8 tools, 3 resource templates, 2 prompts (`write_evm_strategy`, `review_evm_strategy`).
- QuickJS 기반 sandbox runtime — host access blocked (file/process/network/private key 차단).
- Generic EVM contract read/call, ERC20/native helper, raw call action.
- Action validation → simulation → deny-by-default policy → local signer → broadcast → receipt → execution_get / execution://{run_id}.
- 24/24 plans complete, 36/36 requirements satisfied, 512 workspace tests + clippy clean, anvil-feature verification suite (3 passed) + safety suite (2 passed).

**Tech stack:** Rust 2024 edition, rmcp 1.5, tokio, serde + schemars, alloy, rquickjs, rusqlite, tracing, thiserror.

## Current Milestone: v1.1 Adoption

**Goal:** Make "AI로 에이전트 트레이딩 해보고 싶은데…"라고 시작하는 사람이 5분 안에 첫 receipt를 보게 만든다.

**Target features:**
- Prebuilt binary distribution (GitHub Releases) + `claude mcp add` 한 줄 install
- `osmcp init` / `osmcp burner new` UX (사용자가 raw private key를 만지지 않음)
- Testnet starter strategy + mainnet-safe starter policy
- 5-minute Quickstart README + Claude Code 자연어 demo (≤ 90초)
- 5명 dogfood + Run-2/Show-1 측정 → GO/NO-GO 결정

**Key context:**
- v1.1 wallet 경계는 burner only. session wallet / Safe / smart-account은 v1.2+로 미룸.
- Strategy generation과 route 선택(브릿지/venue)은 외부 agent 책임. 우리는 plan을 받는 쪽.
- 첫 사용자는 본인 주변 (이미 Claude Code 쓰는 사람들). 측정은 Run-2, Show-1.

## Next Milestone Goals (v1.1 Adoption)

v1.1의 핵심 목표는 **“AI로 에이전트 트레이딩 해보고 싶은데…”라고 시작하는 사람이 5분 안에 첫 receipt를 보게 만드는 것**이다.

Wedge 가설: distribution과 burner UX의 friction을 0에 가깝게 만들면 “안 쓸 수가 없는” 순간이 만들어진다. 첫 사용자는 본인 주변 (Claude Code 이미 쓰는 사람들)이고, 검증은 Run-2(자발적 두 번째 실행)와 Show-1(자발적 공유) 두 지표로 한다.

핵심 설계 결정:
- **v1.1 wallet 경계는 burner only.** Session wallet, Safe sub-account, smart-account 통합은 v1.2+로 미룬다.
- **Distribution = Claude Code MCP install.** prebuilt binary on GitHub Releases + `claude mcp add ...` 한 줄.
- **Strategy generation은 외부 agent의 일.** 런타임은 plan을 받는 쪽이지 alpha를 만드는 쪽이 아니다.
- **Route 선택(브릿지/venue/체인)은 agent가 한다.** 런타임은 명시적 route plan을 검증·실행한다.

## Requirements

### Validated (v1.0)

- ✓ MCP server가 작고 명확한 agent-facing runtime surface를 제공한다 — v1.0 (MCP-01..04)
- ✓ 에이전트가 JavaScript 전략을 등록하고 실행할 수 있다 — v1.0 (STR-01..05, STJ-01..04)
- ✓ 전략 코드는 제한된 `ctx` API만 쓴다 — v1.0 (STR-04)
- ✓ 전략은 직접 트랜잭션을 보내지 않고 `Action[]`을 반환한다 — v1.0 (STR-05)
- ✓ ABI 기반 generic EVM contract read/call + raw calldata + ERC20/native helper — v1.0 (CTX-01..09)
- ✓ 서명 전 simulation과 policy 검사 — v1.0 (EXE-01..06, POL-01..06)
- ✓ Local signer 서명, broadcast, receipt — v1.0 (EXE-07..09)
- ✓ strategy run, source read, action, policy decision, tx hash, receipt, error journal — v1.0 (STJ-05..07)
- ✓ Local EVM 예제 + safety regression suite + 문서 — v1.0 (VER-01..05)

### Active (v1.1 Adoption)

- [ ] Prebuilt binary 배포 채널이 존재한다 (GitHub Releases, darwin-arm64/darwin-x64/linux-x64).
- [ ] 사용자가 한 줄 install + `claude mcp add` 한 줄로 Claude Code에 연결할 수 있다.
- [ ] `osmcp init`이 config + policy + burner keystore를 안전한 위치에 생성하고 공개 주소를 출력한다.
- [ ] `osmcp burner new`가 burner를 회전시킨다. 사용자는 raw private key를 다루지 않는다.
- [ ] 자금이 들어간 burner로 testnet (Base Sepolia 또는 OP Sepolia)에서 안전한 starter strategy 1건이 receipt까지 완료된다.
- [ ] Mainnet-safe starter policy (Base 또는 Arbitrum) 템플릿이 chain/contract/selector/spend cap을 좁게 잠근다.
- [ ] README 첫 섹션이 install → first run을 5분 안에 안내한다 (anvil 내용은 docs/LOCAL-DEV.md로 분리).
- [ ] Claude Code 자연어 demo (≤ 90초)가 README hero 아래에 임베드된다.
- [ ] 5명에게 dogfood + 72h 측정. Run-2 ≥ 2 AND Show-1 ≥ 1 → v2 시작 / 그 외 → office-hours 재진입.

### Out of Scope (유지)

- TypeScript compiler — 필요해지면 별도 milestone.
- Custom DSL / opcode VM / workflow DAG — `Action[]`로 충분.
- External signer protocol / detached execution — burner-only로 wedge 검증 후 검토.
- Session wallet / Safe / smart-account 통합 — v1.1 wedge 흐림 방지.
- Multi-tenant hosting / dashboard / landing page (제품 UI) — runtime이 본체. 마케팅용 product intro 페이지는 별개 자산.
- Strategy marketplace / capability registry — alpha 공급은 외부 agent와 사용자 몫.
- Scheduler / long-running reconcile loop — v1.1까지는 명시적 register/run/status.
- Protocol-specific recipe catalog — 특정 앱을 core에 박지 않는다.
- Alpha generation / research agent — 우리는 “받는 쪽” 런타임. agent가 외부에서 plan을 만든다.

## Context

v1.0 마무리와 함께 제품 포지셔닝이 명확해졌다.

- **타깃은 개인 DeFi 헌터**. 수가 많고 확산성이 좋다. 지불력은 낮지만 distribution이 압도적으로 싸다.
- **Pull은 “신경 쓸 게 너무 많다”의 해소**. agent 트레이딩을 시작하려면 보통 RPC, signer, nonce, gas, simulation, policy, journaling을 직접 짜야 한다. 우리 제품은 MCP install 하나로 그걸 압축한다.
- **첫 사용자 = 본인 주변**. 이미 Claude Code를 쓰는 사람들이 자연 채널.
- **검증 지표:** Run-2 (24~72h 안의 자발적 두 번째 실행), Show-1 (자발적 공유). 호의 ≠ demand.

코드 상태:
- v1.0 코드 ~Rust 워크스페이스, 512 workspace tests, clippy clean.
- 예제는 anvil 31337 placeholder 기반. mainnet/testnet starter는 v1.1 작업.
- Distribution은 git clone + cargo build 이외 채널 없음 — v1.1의 첫 작업.

알려진 deferred:
- Phase 6 “live local-chain anvil + 실키” 인간 검증은 v1.0 audit에서 acknowledged. v1.1 dogfood가 자연스럽게 그 자리를 대체.

## Constraints

- **Runtime boundary**: 전략 JavaScript는 private key, filesystem, arbitrary network, process API, direct RPC client에 접근할 수 없어야 한다.
- **Execution safety**: simulation과 policy check는 signing보다 먼저 실행되어야 한다.
- **Custody model**: v1.1은 burner-only local hot-wallet. 사용자는 raw private key를 다루지 않고, `osmcp init` / `osmcp burner new`가 keystore를 0600 또는 OS keychain에 저장.
- **Distribution**: prebuilt binary + Claude Code MCP install이 1차 채널. 첫 사용자 경험은 Claude Code 안에서 끝난다.
- **Time-to-first-run**: 처음 install 하는 사용자가 자금 들어간 burner로 testnet receipt까지 5분 안.
- **EVM generality**: ERC20/단일 protocol에 갇히지 않도록 generic ABI contract read/call/raw call 유지.
- **Observability**: 모든 run은 source read, proposed action, policy decision, tx hash, receipt, error를 journal로 남긴다.
- **Wedge protection**: dashboard, marketplace, scheduler, external signer, hosted custody, alpha generation agent를 v1.1에 넣지 않는다.

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| v1은 EVM 자동화 실행 런타임이다 | 제품 본질. agent가 alpha를 만들면 우리는 실행 권한을 통제한다 | ✓ Good — v1.0 shipped (36/36 reqs, 512 tests) |
| 전략 언어는 plain JavaScript | compiler 복잡도 피하고 agent가 쓰기 쉽다 | ✓ Good — QuickJS sandbox + ctx API 안정 |
| 전략은 작은 `ctx` API만 쓴다 | host access 차단, 문서화 쉬움 | ✓ Good — sandbox_host_globals + sandbox_limits regression 통과 |
| 전략은 `Action[]`을 반환한다 | opcode VM/graph engine보다 단순하면서 구조화 | ✓ Good — Action enum + per-variant validators 통과 |
| Generic EVM read/write 지원 | app-specific core 없이도 대부분 작업 표현 가능 | ✓ Good — CTX-01..09 모두 satisfied |
| v1은 local signer | detached execution보다 단순하고 첫 경험 완결 | ✓ Good — Phase 6 LocalSignerHandle::from_env |
| Signer는 boundary로 분리 | 나중에 external signer 확장 가능 | ✓ Good — executor-signer crate 분리 유지 |
| Strategy generation은 외부 agent 책임 | 우리는 alpha 회사가 아니다 | — Pending — v1.1 dogfood로 검증 |
| Route 선택(bridge/venue)은 agent | 런타임이 router brain이 되면 wedge 흐려진다 | — Pending — v1.1 |
| v1.1 wallet 경계는 burner-only | session wallet은 과하다, friction 최소화가 우선 | — Pending — Phase 8 구현 후 |
| Distribution은 Claude Code MCP install | prebuilt binary + `claude mcp add` 한 줄, 첫 사용자 환경 그대로 | — Pending — Phase 8/10 |
| 검증 지표는 Run-2 + Show-1 | 호의가 demand로 착각되지 않게 | — Pending — Phase 11 dogfood |

## Evolution

This document evolves at phase transitions and milestone boundaries.

**After each phase transition** (via `$gsd-transition`):
1. Requirements invalidated? -> Move to Out of Scope with reason
2. Requirements validated? -> Move to Validated with phase reference
3. New requirements emerged? -> Add to Active
4. Decisions to log? -> Add to Key Decisions
5. "What This Is" still accurate? -> Update if drifted

**After each milestone** (via `$gsd-complete-milestone`):
1. Full review of all sections
2. Core Value check — still the right priority?
3. Audit Out of Scope — reasons still valid?
4. Update Context with current state

---
*Last updated: 2026-05-04 after v1.0 milestone*
