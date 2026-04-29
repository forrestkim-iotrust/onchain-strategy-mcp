# onchain-strategy-mcp

AI agent가 로컬 EVM 자동화 전략을 작성, 실행, 감사할 수 있게 해주는 MCP 런타임입니다.

## 이 런타임이 하는 일

`onchain-strategy-mcp`는 local-first MCP 런타임입니다. 에이전트는 샌드박스된 JavaScript 전략을 등록하고, 런타임을 통해 실행하며, prose 로그가 아니라 구조화된 리포트를 받습니다.

v1 실행 루프는 다음과 같습니다.

```text
strategy_register
  -> strategy_run
  -> sandboxed JS returns Action[]
  -> action validation
  -> EVM simulation
  -> policy check
  -> local hot-wallet signing
  -> broadcast to configured RPC
  -> receipt wait
  -> execution_get or execution://{run_id}
```

전략 JavaScript는 `ctx`로 action을 제안할 수 있지만 private key를 받지 않으며 직접 서명, 브로드캐스트, 파일 접근, process API, 임의 네트워크 클라이언트를 사용할 수 없습니다. 런타임은 strategy run, source read, policy/simulation decision, execution action row, receipt, error를 로컬 SQLite state에 기록합니다.

이 저장소는 hosted custody, 마켓플레이스, 프로토콜 recipe catalog가 아닙니다. 제품 UI나 장기 실행 자동화 daemon이 아니라 로컬 MCP 런타임입니다.

## 로컬 hot-wallet 안전 모델

v1은 non-noop 전략이 승인된 실행 경로에 도달했을 때만 로컬 hot-wallet private key를 사용합니다. 서명 전에는 simulation과 policy check가 반드시 통과해야 합니다.

Signer config에는 환경 변수 이름만 저장합니다.

```toml
[signer]
private_key_env = "EXECUTOR_PRIVATE_KEY"
receipt_timeout_ms = 120000
```

원문 개인키(raw private key) 값은 `[signer].private_key_env`가 가리키는 operator 환경 변수 안에만 있어야 합니다. 원문 개인키를 `config.example.toml`, runtime config, strategy JavaScript, README 예시, 로그, prompt, issue report에 커밋하거나 출력하지 마세요. 전략 파일에는 public address와 placeholder는 둘 수 있지만 signer secret은 넣지 않습니다.

Policy는 deny-by-default입니다. 로컬 policy는 좁게 유지하세요: 정확한 chain ID, 정확한 contract address, 허용 selector, native value 제한, ERC20 spend 제한, 그리고 꼭 필요하지 않으면 `raw_call` 비활성화.

## 로컬 Anvil 예제

체크인된 예제는 로컬 Anvil 스타일 fixture에서 runtime loop를 보여줍니다.

- `examples/strategies/erc20-approve.js`는 ERC20 approve action을 만듭니다.
- `examples/strategies/generic-counter-call.js`는 generic ABI `increment()` contract call을 만듭니다.
- `examples/policies/local-anvil.toml`은 chain `31337`과 정확한 selector allowlist policy 예시입니다.
- `config.example.toml`은 secret 없이 local state, EVM RPC, policy, signer section을 보여줍니다.

일반적인 agent/operator 흐름:

1. Anvil을 실행하고 strategy/policy 예시의 placeholder address에 맞는 로컬 contract를 배포하거나 대체합니다.
2. 필요하면 `config.example.toml`을 커밋하지 않는 로컬 config 파일로 복사합니다.
3. `EXECUTOR_PRIVATE_KEY`는 operator shell 환경 변수에만 설정합니다. 원문 키를 커밋된 파일에 쓰지 않습니다.
4. 전략 JS를 작성할 때 MCP prompt `write_evm_strategy`를 사용하고, 등록 전 `review_evm_strategy`로 검토합니다.
5. `examples/strategies/erc20-approve.js` 같은 체크인된 source를 `strategy_register`로 등록합니다.
6. `strategy_run`으로 실행합니다.
7. 반환된 run ID를 `execution_get` 또는 `execution://{run_id}` resource로 조회해 receipt-backed action report를 확인합니다.
8. journal resource에서 source read, validation, simulation, policy, action outcome을 검토합니다.

예제 검증 테스트는 `examples/strategies/erc20-approve.js`와 `examples/strategies/generic-counter-call.js`를 직접 읽어 실행하므로, 이 파일들은 문서용 snippet이 아니라 실제 runtime input입니다.

## Verification

예제, policy, simulation, sandbox, execution report를 바꾸기 전후에 다음 검증을 실행하세요.

```bash
cargo test -p executor-mcp --features anvil-tests --test verification_examples -- --nocapture
cargo test -p executor-mcp --test verification_safety
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

검증 suite는 로컬 예제, `verification_examples`, `verification_safety`의 policy/simulation/sandbox 경계, workspace regression, lint 상태를 확인합니다.

## 문서 안내

- [README.md](./README.md) — English overview and usage notes.
- [AGENTS.md](./AGENTS.md) — agent/operator workflow and command checklist.
- [FOUNDATIONS_ko.md](./FOUNDATIONS_ko.md) — 프로젝트 foundation notes.
- [USE_CASES_ko.md](./USE_CASES_ko.md) — use-case notes.
