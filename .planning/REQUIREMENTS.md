# Requirements — onchain-strategy-mcp v1.1 Adoption

**Milestone goal:** "AI로 에이전트 트레이딩 해보고 싶은데…"라고 시작하는 사람이 5분 안에 첫 receipt를 보게 만든다.

**Wedge:** Distribution + burner UX의 friction을 0에 가깝게. 첫 사용자는 본인 주변(이미 Claude Code 쓰는 사람들). 검증 지표는 Run-2(자발적 두 번째 실행)와 Show-1(자발적 공유).

---

## v1.1 Requirements

### Distribution (DIST)

- [ ] **DIST-01**: Repo는 tagged release 시 darwin-arm64 / darwin-x64 / linux-x64 prebuilt binary를 GitHub Releases에 자동 publish한다.
- [ ] **DIST-02**: 사용자는 한 줄 install script (curl ... | sh)로 binary를 `~/.local/bin/onchain-strategy-mcp`에 받을 수 있다.
- [ ] **DIST-03**: Release artifact에는 SHA256 checksum이 포함되고, install script가 이를 검증한다.
- [ ] **DIST-04**: 사용자는 `claude mcp add onchain-strategy-mcp <binary path>` 한 줄로 Claude Code에 연결할 수 있다.

### Burner UX (BUR)

- [ ] **BUR-01**: `osmcp init`은 config.toml + policy.toml + burner keystore를 `~/.onchain-strategy-mcp/`에 생성하고 burner의 공개 주소를 출력한다.
- [ ] **BUR-02**: Burner private key는 OS keychain 또는 0600-mode 파일에만 저장되며 stdout/log/journal에 노출되지 않는다.
- [ ] **BUR-03**: `osmcp burner new`는 새 burner를 생성하고 기존 burner를 회전(rotate)한다. 사용자는 raw private key를 직접 다루지 않는다.
- [ ] **BUR-04**: `docs/BURNER.md`는 burner의 위협 모델(메인 지갑 절대 사용 금지, max 손실 = burner 잔액, key 노출 시 회복 절차)을 명시한다.

### Real-network Starter (NET)

- [ ] **NET-01**: `examples/strategies/testnet-self-transfer.js`는 Base Sepolia 또는 OP Sepolia에서 burner의 native balance가 threshold 이상이면 0.0001 ETH를 자기 자신에게 transfer하는 안전한 starter strategy를 제공한다.
- [ ] **NET-02**: `examples/policies/testnet-starter.toml`은 위 strategy의 chain/from-address/selector만 허용하는 starter policy를 제공한다.
- [ ] **NET-03**: `examples/policies/mainnet-base-burner.toml`은 chain 8453에서 USDC/WETH allowlist + 작은 spend cap ($5 equivalent) + raw_call 비활성을 강제하는 mainnet-safe template을 제공한다.
- [ ] **NET-04**: 자금 들어간 burner가 testnet starter strategy를 strategy_run으로 실행하면 60초 안에 receipt까지 완료된다.

### Quickstart & Demo (QSD)

- [ ] **QSD-01**: README 첫 섹션은 install → first run을 6단계 이내, 5분 안에 안내한다 (anvil 설명은 docs/LOCAL-DEV.md로 분리).
- [ ] **QSD-02**: README는 `claude mcp add` 한 줄과 `osmcp init` 한 줄을 명시적으로 보여준다.
- [ ] **QSD-03**: README hero 아래에 90초 이내의 Claude Code 자연어 demo (asciinema 또는 video)가 임베드된다.
- [ ] **QSD-04**: 자연어 demo는 사용자가 Claude에게 strategy 작성을 요청 → write_evm_strategy → register → run → execution_get까지 보여준다.

### Dogfood & Measurement (DOG)

- [ ] **DOG-01**: 5명의 Claude Code 사용자가 install → first run을 시도하고, install→first-run 시간이 측정된다.
- [ ] **DOG-02**: 72시간 동안 Run-2 (자발적 두 번째 strategy run)와 Show-1 (자발적 친구/커뮤니티 공유) 지표가 기록된다.
- [ ] **DOG-03**: Dogfood 결과 문서는 GO/NO-GO 결정을 명시한다 (Run-2 ≥ 2 AND Show-1 ≥ 1 → v2 시작 / 그 외 → office-hours 재진입).

---

## Future Requirements (Deferred)

- Session wallet / Safe / smart-account integration
- Strategy template marketplace
- Hosted runtime / scheduler / autonomous reconcile loop
- Detached execution / external signer protocol
- Multi-account orchestration
- TypeScript compiler / type hints

---

## Out of Scope (v1.1)

- **Dashboard / web UI** — 제품은 MCP runtime + CLI. 브라우저 UI는 없음.
- **Hosted custody** — local-first, burner-only. 클라우드 키 보관 없음.
- **Mainnet "이거 사라" 추천** — alpha generation은 외부 agent 책임. 우리는 plan을 받는 쪽.
- **Real-network starter on Ethereum mainnet** — gas 비싸고 risk 큰 첫 경험. testnet → Base/Arbitrum mainnet starter policy까지가 v1.1.
- **iOS/Android wrapper** — Claude Code MCP 환경이 1차 채널. 모바일은 별 milestone.
- **Strategy 자동 generation** — 우리는 strategy를 작성하지 않는다. agent의 일.

---

## Traceability

| REQ-ID | Phase | Status |
|--------|-------|--------|
| DIST-01 | Phase 8 | Pending |
| DIST-02 | Phase 8 | Pending |
| DIST-03 | Phase 8 | Pending |
| DIST-04 | Phase 8 | Pending |
| BUR-01 | Phase 8 | Pending |
| BUR-02 | Phase 8 | Pending |
| BUR-03 | Phase 8 | Pending |
| BUR-04 | Phase 8 | Pending |
| NET-01 | Phase 9 | Pending |
| NET-02 | Phase 9 | Pending |
| NET-03 | Phase 9 | Pending |
| NET-04 | Phase 9 | Pending |
| QSD-01 | Phase 10 | Pending |
| QSD-02 | Phase 10 | Pending |
| QSD-03 | Phase 10 | Pending |
| QSD-04 | Phase 10 | Pending |
| DOG-01 | Phase 11 | Pending |
| DOG-02 | Phase 11 | Pending |
| DOG-03 | Phase 11 | Pending |

**Coverage:** 19/19 v1.1 requirements mapped to 4 phases (8, 9, 10, 11).
