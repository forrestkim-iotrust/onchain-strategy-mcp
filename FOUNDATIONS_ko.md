# Foundations

`onchain-strategy-mcp`에 대한 현재까지의 철학, 책임 경계, 전략 표현 방식, 실행 모델, 차용할 설계 패턴을 정리한 작업 메모입니다.

이 문서는 확정된 제품 문서라기보다, 레포의 방향을 고정하기 위한 설계 기준 문서입니다. 구현 전에는 특히 `TickInputSnapshot`, `NormalizedAction`, `ExecutionEnvelope`, `ExternalExecutionResult` 계약을 먼저 좁혀야 합니다.

## 1. 한 문장 정의

`onchain-strategy-mcp`는 에이전트가 온체인 전략 프로그램을 만들고, 실행하고, 관리할 수 있게 해주는 MCP 전략 런타임이다.

조금 더 정확히 말하면:

- 전략은 인터프리터 가능한 프로그램이다.
- 전략은 직접 트랜잭션을 보내지 않는다.
- 전략은 감사 가능한 action graph를 만든다.
- 런타임은 그 graph를 정규화하고 실행 규율을 강제한다.
- 실행 결과와 판단 근거는 재구성 가능한 형태로 기록된다.

## 2. 프로젝트가 무엇이고, 무엇이 아닌가

이 프로젝트는 다음이 아니다.

- 트레이딩 앱
- 지갑 UI
- 클라우드 배포 플랫폼
- 전략 마켓플레이스
- 채팅 UI
- 랜딩 페이지
- 온보딩 제품

이 프로젝트는 다음이다.

- 에이전트를 위한 전략 런타임
- 온체인 실행을 위한 제약된 실행 경계
- 전략 생명주기 관리 시스템
- 시뮬레이션, 정책, 서명 경계, 실행 보고를 포함하는 실행 규율 레이어

## 3. 핵심 철학

현재까지 정리된 핵심 철학은 다음과 같다.

### 3.1 런타임은 앱을 모른다

런타임은 특정 앱을 제품 단위로 이해하지 않는다.

- Uniswap
- Aave
- Safe
- Across
- Hyperliquid

같은 대상은 런타임 입장에서 "앱"이 아니라 capability 또는 adapter다.

핵심 문장:

> 런타임은 앱을 모른다.  
> 앱은 capability다.  
> 실행은 primitive다.

### 3.2 키는 소유하지 않는다. 실행 규율은 책임진다

런타임은 전략이 만든 실행 후보를 실제 온체인 액션으로 연결할 수 있어야 한다. 하지만 private key custody는 필수가 아니다.

핵심 문장:

> 키는 소유하지 않는다. 실행 규율은 책임진다.

여기서 실행 규율이란:

- action normalization
- deterministic input snapshot
- simulation
- policy enforcement
- approval flow
- signing boundary
- broadcasting 또는 externalization
- receipt tracking 또는 result ingestion
- append-only execution journal

### 3.3 전략은 자유 코드가 아니라 감사 가능한 프로그램이다

전략은 그냥 "임의의 JS 코드"가 아니다.

전략은:

- 읽을 수 있고
- 검증할 수 있고
- 설명할 수 있고
- 기록할 수 있고
- 재실행할 수 있어야 한다.

따라서 전략은 결국 감사 가능한 intermediate representation으로 환원되어야 한다. 또한 전략이 특정 action graph를 만든 이유를 나중에 재구성할 수 있도록, tick 실행의 입력과 source read 결과를 `TickInputSnapshot`으로 남겨야 한다.

## 4. 책임 경계

현재 논의 기준으로 런타임이 책임지는 것과 책임지지 않는 것은 아래와 같다.

### 런타임이 책임지는 것

- strategy validation
- strategy registration/update/versioning
- strategy start/stop/pause/resume
- strategy tick scheduling
- strategy-local state
- source reads through constrained APIs
- tick input snapshot persistence
- action graph generation
- action normalization
- simulation
- policy evaluation
- execution lifecycle persistence
- runtime control
- logs, receipts, structured reports

### 런타임이 직접 책임지지 않아도 되는 것

- private key custody
- seed phrase
- wallet onboarding UX
- infrastructure deployment
- cloud provisioning
- dashboard
- marketplace

## 5. 실행 모드와 실행 단계

용어를 분리한다.

`ExecutionMode`는 서명, 브로드캐스트, receipt 추적의 transport ownership을 뜻한다.

`ExecutionPhase`는 전략 실행 루프 안에서 지금 무엇을 하는지 뜻한다.

### 5.1 ExecutionMode

런타임의 기본 철학은 full runtime이지만, 서명과 브로드캐스트는 adapter-first로 분리 가능해야 한다.

지원할 개념적 mode는 세 가지다.

- `managed_execution`
- `detached_signing`
- `detached_execution`

#### Managed Execution

런타임이 signer adapter와 broadcaster adapter를 사용해 end-to-end 실행한다.

```text
strategy
  -> action graph
  -> normalize
  -> simulate
  -> policy
  -> sign
  -> broadcast
  -> receipt
  -> journal
```

#### Detached Signing

서명만 외부 signer가 담당한다.

```text
strategy
  -> action graph
  -> normalize
  -> simulate
  -> policy
  -> sign request externalized
  -> signed payload returned
  -> runtime broadcasts
  -> receipt
  -> journal
```

#### Detached Execution

서명과 브로드캐스트 모두 외부 executor가 담당한다.

```text
strategy
  -> action graph
  -> normalize
  -> simulate
  -> policy
  -> execution envelope externalized
  -> external executor signs/broadcasts
  -> result reported back
  -> journal
```

이 세 가지 모드는 제품 정체성을 흐리지 않는다. 핵심은 런타임이 "실행 규율"을 책임지고, 실제 운송 수단은 교체 가능하도록 두는 것이다.

### 5.2 ExecutionPhase

`read-only`, `plan + approve`, `read-only + plan + control` 같은 표현은 `ExecutionMode`가 아니다. 이것들은 use case가 어떤 단계까지 진행되는지를 나타내는 `ExecutionPhase` 조합이다.

초기 phase는 다음처럼 본다.

- `observe`: 상태를 읽고 조건을 평가한다.
- `propose`: action graph와 plan을 만든다.
- `approve`: 사람, policy, 외부 시스템의 승인 경계를 지난다.
- `execute`: mode에 따라 실행하거나 externalize한다.
- `reconcile`: desired state와 observed state를 맞춘다.
- `report`: 실행 결과와 판단 근거를 기록한다.

예를 들어 "read-only" 유스케이스는 `observe + report`이고, "plan + approve"는 `observe + propose + approve`다. 실제 transport ownership은 별도의 `ExecutionMode`로만 표현한다.

## 6. 전략 표현 방식

### 6.1 JS 함수는 좋은 인터페이스다

사용자와 AI가 가장 쉽게 다룰 수 있는 표현은 JavaScript 함수다.

하지만 JS 함수는 실행 권한자가 아니다.

핵심 문장:

> JavaScript는 실행 권한자가 아니라, action graph를 만드는 DSL이다.

전략 함수는 다음을 할 수 있다.

- `ctx.source.*`를 통한 제한된 데이터 읽기
- 조건 판단
- flow 조합
- action node 생성
- structured graph 반환

전략 함수는 다음을 할 수 없어야 한다.

- 직접 tx broadcast
- unrestricted fetch
- 임의 파일 IO
- signer 접근
- host process 접근

### 6.2 JS 전략의 예시

```js
export async function tick(ctx) {
  const usdc = await ctx.source.erc20Balance({
    chainId: 42161,
    token: "USDC",
    account: ctx.account.address,
  });

  if (usdc.gte(100)) {
    return ctx.sequence([
      ctx.action.erc20Approve({
        chainId: 42161,
        token: "USDC",
        spender: ctx.cap.aave.pool,
        amount: "50",
      }),
      ctx.action.contractCall({
        chainId: 42161,
        to: ctx.cap.aave.pool,
        abi: "supply(address,uint256,address,uint16)",
        args: ["USDC", "50", ctx.account.address, 0],
        reason: "supply idle USDC",
      }),
    ]);
  }

  return ctx.noop("conditions not met");
}
```

핵심은 `contractCall()`이 직접 실행하는 것이 아니라, action node를 반환한다는 점이다.

### 6.3 ActionGraph와 NormalizedAction

JS 함수가 반환하는 값은 사람이 작성하기 쉬운 `ActionGraph`다. `ActionGraph`는 capability alias, ABI signature, symbolic token name 같은 고수준 표현을 포함할 수 있다.

하지만 policy와 execution으로 넘어가기 전에는 반드시 `NormalizedAction`으로 컴파일되어야 한다.

```text
ActionGraph
  -> capability resolution
  -> ABI encoding
  -> effect inference
  -> risk annotation
  -> NormalizedAction
  -> simulation
  -> policy
```

`ActionGraph` 예시:

```json
{
  "kind": "sequence",
  "steps": [
    {
      "kind": "erc20_approve",
      "chain_id": 42161,
      "token": "USDC",
      "spender": "aave.pool",
      "amount": "50"
    },
    {
      "kind": "contract_call",
      "chain_id": 42161,
      "to": "aave.pool",
      "abi": "supply(address,uint256,address,uint16)",
      "args": ["USDC", "50", "$account.address", 0],
      "reason": "supply idle USDC"
    }
  ]
}
```

`NormalizedAction` 예시:

```json
{
  "action_id": "act_01H...",
  "execution_id": "exec_01H...",
  "strategy_id": "strat_idle_usdc",
  "strategy_version": "sha256:...",
  "account_id": "acct_main",
  "kind": "contract_call",
  "chain_id": 42161,
  "from": "0xAccount",
  "to": "0xAavePool",
  "value": "0",
  "selector": "0x617ba037",
  "calldata": "0x617ba037...",
  "decoded_call": {
    "signature": "supply(address,uint256,address,uint16)",
    "args": {
      "asset": "0xUSDC",
      "amount": "50000000",
      "on_behalf_of": "0xAccount",
      "referral_code": 0
    }
  },
  "expected_effects": [
    {
      "type": "erc20_delta",
      "token": "0xUSDC",
      "account": "0xAccount",
      "min_delta": "-50000000",
      "max_delta": "0"
    }
  ],
  "risk": {
    "max_native_value": "0",
    "max_spend": [
      {
        "asset": "0xUSDC",
        "amount": "50000000"
      }
    ],
    "requires_allowance": [
      {
        "token": "0xUSDC",
        "spender": "0xAavePool",
        "minimum": "50000000"
      }
    ]
  },
  "idempotency_key": "acct_main:strat_idle_usdc:sha256:..."
}
```

정책 엔진은 `ActionGraph`가 아니라 `NormalizedAction`을 입력으로 받아야 한다. 그래야 app 이름이 아니라 chain, contract, selector, calldata, token spend, expected effect 기준으로 판단할 수 있다.

## 7. TickInputSnapshot

전략 실행은 나중에 설명 가능해야 한다. 따라서 `tick(ctx)`가 실행될 때 런타임은 입력 스냅샷을 남긴다.

`TickInputSnapshot`은 다음을 포함한다.

```json
{
  "tick_id": "tick_01H...",
  "strategy_id": "strat_idle_usdc",
  "strategy_version": "sha256:...",
  "account_id": "acct_main",
  "scheduled_at": "2026-04-24T00:00:00Z",
  "started_at": "2026-04-24T00:00:01Z",
  "runtime_version": "0.1.0",
  "account_state_version": "acct_state_42",
  "strategy_state_version": "state_17",
  "policy_version": "policy_9",
  "source_reads": [
    {
      "read_id": "read_01H...",
      "op": "source.erc20_balance",
      "args_hash": "sha256:...",
      "chain_id": 42161,
      "block_number": 200000000,
      "block_hash": "0x...",
      "value": "100000000",
      "observed_at": "2026-04-24T00:00:02Z"
    }
  ],
  "limits": {
    "max_runtime_ms": 1000,
    "max_source_reads": 32,
    "max_actions": 16
  }
}
```

중요한 규칙:

- source read 결과는 tick 안에서 memoize한다.
- 같은 tick에서 같은 source read는 같은 값을 반환해야 한다.
- chain read는 가능한 경우 block number 또는 block hash를 포함한다.
- price, HTTP, 외부 API 같은 nondeterministic source는 `observed_at`, provider id, response hash를 포함한다.
- replay는 최소한 같은 snapshot으로 같은 action graph를 다시 만들 수 있어야 한다.

## 8. Primitive / Opcode 모델

더 쉬운 확장성과 AI 친화성을 위해 전략은 opcode 혹은 primitive 조합으로 이해하는 것이 좋다.

전략을 구성하는 핵심 계층:

- source
- transform
- condition
- flow
- action
- state

예상 opcode group은 다음과 같다.

### source

- `source.price`
- `source.chain_call`
- `source.erc20_balance`
- `source.native_balance`
- `source.allowance`
- `source.strategy_state`
- `source.event_log`

### transform

- `transform.decode_abi`
- `transform.percent_change`
- `transform.window`
- `transform.normalize_units`
- `transform.map`
- `transform.filter`

### condition

- `condition.gt`
- `condition.gte`
- `condition.lt`
- `condition.eq`
- `condition.and`
- `condition.or`
- `condition.changed`
- `condition.cooldown_elapsed`

### flow

- `flow.sequence`
- `flow.branch`
- `flow.parallel`
- `flow.retry`
- `flow.timeout`
- `flow.guard`

### action

- `action.contract_call`
- `action.erc20_approve`
- `action.native_transfer`
- `action.external_execution`
- `action.notify`
- `action.memory_set`

JS 전략은 이 opcode를 사람이 쓰기 쉬운 문법으로 조합하는 레이어라고 볼 수 있다.

## 9. Capability 모델

앱별 지식은 core runtime이 아니라 capability로 분리한다.

예:

- `cap:erc20`
- `cap:aave`
- `cap:uniswap-v3`
- `cap:safe`
- `cap:across`

capability의 역할:

- helper 제공
- ABI/call builder 제공
- common target alias 제공
- action graph 생성 도우미 제공
- expected effect와 risk annotation 생성 보조

capability는 실행 권한자가 아니다. capability는 결국 primitive action을 생성하는 helper일 뿐이다. capability가 만든 action도 runtime의 normalization, simulation, policy를 통과해야 한다.

## 10. 다중 계정 모델

다중 계정은 나중 기능이 아니라 초기 도메인 모델이다.

최소 모델:

```text
Account
  StrategyInstance
    Execution
      Action
```

여기서 `Account`는 단순 wallet 주소가 아니라 실행과 정책의 경계다.

Account는 다음을 가질 수 있다.

- wallet refs
- signer refs
- policy id
- execution mode
- chain-specific addresses
- budgets
- nonce lanes
- execution locks

예시:

```json
{
  "account_id": "acct_main_arbitrum",
  "wallet_refs": [
    {
      "chain_id": 42161,
      "address": "0x...",
      "signer_ref": "safe_main",
      "nonce_lane": "evm:42161:0x..."
    }
  ],
  "policy_id": "policy_low_risk",
  "execution_mode": "detached_execution",
  "budget_policy_id": "budget_main"
}
```

또한 전략 정의와 전략 인스턴스를 분리해야 한다.

```text
StrategyDefinition
  code_hash
  language
  source
  declared_permissions

StrategyInstance
  strategy_id
  definition_id
  account_id
  status
  schedule
  strategy_state
```

그래야 같은 전략을 여러 account에서 독립적으로 돌릴 수 있다.

### 10.1 Account 동시성 규칙

같은 account에서 여러 전략이 동시에 실행될 수 있으므로 account-scoped concurrency primitive가 필요하다.

MVP의 최소 규칙:

- `BudgetReservation`: policy 통과 후 실제 실행 전까지 spend capacity를 hold한다.
- `NonceLane`: 같은 chain/address의 transaction ordering을 직렬화한다.
- `InFlightExecutionLock`: 같은 idempotency key 또는 같은 resource를 중복 실행하지 않는다.
- `StateRevision`: strategy state와 account state는 revision을 갖고 optimistic update를 수행한다.

상태 예시:

```text
budget reservation: requested -> held -> consumed | released | expired
nonce lane: available -> reserved -> submitted -> confirmed | failed
execution lock: acquired -> completed | expired
```

이 규칙이 없으면 두 전략이 같은 allowance, nonce, budget을 동시에 소비할 수 있고, journal이 실제 실행 순서를 설명하지 못한다.

## 11. 실행 생명주기

모든 액션은 같은 실행 파이프라인을 가져야 한다.

```text
tick(ctx)
  -> source reads
  -> persist tick snapshot
  -> action graph
  -> normalize
  -> simulate
  -> policy check
  -> budget reservation
  -> approval request
  -> sign or externalize
  -> broadcast or externalize
  -> watch or ingest result
  -> persist report
```

중요한 점:

- simulation 전에는 실행하지 않는다.
- policy를 통과하지 못한 액션은 signer로 가지 않는다.
- budget reservation이 잡히지 않은 액션은 실행하지 않는다.
- 모든 결과는 structured report로 남는다.
- execution journal은 append-only다.

## 12. ExecutionEnvelope와 ExternalExecutionResult

`detached_execution`에서도 런타임은 실행 규율을 잃으면 안 된다. 외부 executor로 넘기는 요청과 되돌아오는 결과는 표준화한다.

### 12.1 ExecutionEnvelope

`ExecutionEnvelope`는 외부 executor가 실행해야 하는 policy-approved 요청이다.

```json
{
  "execution_id": "exec_01H...",
  "action_id": "act_01H...",
  "account_id": "acct_main",
  "strategy_id": "strat_idle_usdc",
  "strategy_version": "sha256:...",
  "mode": "detached_execution",
  "chain_id": 42161,
  "from": "0xAccount",
  "to": "0xAavePool",
  "value": "0",
  "selector": "0x617ba037",
  "calldata": "0x617ba037...",
  "simulation": {
    "status": "success",
    "gas_estimate": "182000",
    "simulated_at_block": 200000000
  },
  "policy": {
    "status": "approved",
    "policy_version": "policy_9",
    "decision_id": "decision_01H..."
  },
  "budget_reservation_id": "resv_01H...",
  "idempotency_key": "acct_main:strat_idle_usdc:sha256:...",
  "reason": "supply idle USDC",
  "created_at": "2026-04-24T00:00:03Z",
  "expires_at": "2026-04-24T00:10:03Z"
}
```

### 12.2 ExternalExecutionResult

외부 executor는 결과를 `ExternalExecutionResult`로 report한다.

```json
{
  "execution_id": "exec_01H...",
  "action_id": "act_01H...",
  "account_id": "acct_main",
  "chain_id": 42161,
  "executor_ref": "safe_service_main",
  "signer_ref": "safe_main",
  "status": "confirmed",
  "tx_hash": "0x...",
  "submitted_at": "2026-04-24T00:01:00Z",
  "observed_at": "2026-04-24T00:01:20Z",
  "block_number": 200000003,
  "receipt_status": "success",
  "gas_used": "178203",
  "error": null
}
```

필수 규칙:

- `execution_id`는 런타임이 만든 값이어야 한다.
- `chain_id`와 `tx_hash`가 있으면 런타임은 receipt를 검증하거나 나중에 reconcile할 수 있어야 한다.
- failed result는 `error.code`, `error.message`, retry 가능 여부를 포함해야 한다.
- stale 또는 unknown `execution_id`는 journal에 기록하되 state transition에는 반영하지 않는다.
- result ingestion은 idempotent해야 한다.

상태 전이:

```text
proposed
  -> normalized
  -> simulated
  -> policy_approved
  -> budget_held
  -> externalized
  -> submitted
  -> confirmed | failed | expired | cancelled
```

`managed_execution`과 `detached_signing`도 내부적으로 같은 상태 전이를 사용하되, externalized 단계가 짧거나 생략될 수 있다.

## 13. 정책 모델

정책은 앱 이름이 아니라 primitive 기준으로 걸어야 한다.

나쁜 예:

```json
{
  "allow_apps": ["uniswap", "aave"]
}
```

좋은 예:

```json
{
  "chains": [42161],
  "contracts": {
    "0xaf88...": {
      "selectors": ["0x095ea7b3", "0xa9059cbb"],
      "max_spend": {
        "USDC": "100000000"
      }
    }
  },
  "default": "deny"
}
```

즉 정책은 다음을 기준으로 판단한다.

- chain
- contract
- function selector
- calldata shape
- token spend
- native value
- expected effect
- account budget
- strategy permission

## 14. 런타임이 차용할 설계 패턴

현재까지 논의에서 유효하다고 본 차용 대상은 다음과 같다.

### 14.1 Terraform

차용 포인트:

- plan/apply 분리
- 실행 전 명시적인 실행 계획
- detached execution과 자연스럽게 결합 가능

### 14.2 Kubernetes Controller

차용 포인트:

- desired state vs observed state
- reconcile loop
- idempotent 실행 모델

### 14.3 Temporal / Durable Workflow

차용 포인트:

- durable history
- restart-safe workflow
- step/activity 분리
- retry semantics

### 14.4 Node-RED / n8n

차용 포인트:

- node/edge 기반 전략 표현
- source/transform/condition/action 분리
- 나중에 시각화 가능

### 14.5 Account Abstraction

차용 포인트:

- execution envelope
- signer/bundler/executor 분리 가능
- 런타임은 request validity를 책임짐

## 15. MVP 방향

MVP는 product surface를 넓히기보다 core runtime을 구현하는 데 집중한다.

### MVP에 포함할 것

1. JavaScript 전략 검증 및 등록
2. sandboxed `tick(ctx)` 실행
3. `TickInputSnapshot` 저장
4. structured `ActionGraph` 반환
5. `NormalizedAction` 생성
6. EVM simulation path
7. policy gate
8. account-scoped budget reservation
9. pluggable signing/broadcast mode
10. execution journal
11. external execution result ingestion
12. account-scoped strategy state

### MVP에서 보류할 것

- 랜딩 페이지
- 웹 대시보드
- 클라우드 배포
- wallet onboarding
- strategy marketplace
- 복잡한 exchange-specific recipe
- 조직/팀 RBAC
- billing

## 16. 열린 질문

아직 명확히 고정되지 않은 질문도 있다.

### 16.1 JS 런타임 엔진

후보:

- QuickJS 계열
- Rhai
- Deno isolate 계열

현재 방향:

- 에이전트 친화성을 위해 JS가 유리
- 다만 sandbox와 runtime budget이 강해야 함

### 16.2 source API의 범위

어디까지 기본 제공할지 아직 열려 있다.

예:

- price source를 기본 내장할지
- generic HTTP source를 허용할지
- chain read만 허용할지

source API가 넓어질수록 `TickInputSnapshot`의 provider metadata와 replay 규칙도 강해져야 한다.

### 16.3 capability distribution

- capability를 코드 패키지로 둘지
- MCP tool 형태로 둘지
- strategy source와 함께 번들링할지

## 17. 현재 기준의 가장 짧은 정의

현재까지 논의를 가장 짧게 요약하면 다음과 같다.

> `onchain-strategy-mcp`는 샌드박스된 JavaScript 전략 함수와 감사 가능한 실행 primitive를 기반으로, 에이전트가 온체인 전략 프로그램을 만들고 실행/관리할 수 있게 해주는 MCP 런타임이다.
