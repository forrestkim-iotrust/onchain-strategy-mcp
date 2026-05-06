# Use Cases

`onchain-strategy-mcp`의 초기 대표 유스케이스를 정리한 문서입니다.

이 문서의 목적은 다음과 같습니다.

- 이 런타임으로 실제 어떤 일이 가능한지 구체화
- MVP에서 무엇을 먼저 검증해야 하는지 정리
- 전략, account, execution mode, primitive 설계가 어떤 현실 문제를 풀기 위한 것인지 고정

## 1. 이 런타임으로 실제 생기는 일

이 시스템으로 생기는 일은 단순히 "트랜잭션 하나 보내기"가 아닙니다.

더 정확히 말하면:

- 에이전트가 전략 프로그램을 만든다
- 전략이 계속 상태를 본다
- 조건이 맞으면 action graph를 만든다
- 런타임이 그 graph를 정규화하고 검증한다
- policy/signing/broadcast 경계를 거쳐 실제 실행되거나 외부로 넘겨진다
- 결과와 판단 근거가 journal과 report로 남는다

즉 이 프로젝트는 온체인용 자동화 런타임이자 운영 루프를 만드는 도구에 가깝습니다.

## 2. 용어 기준

유스케이스에서 두 용어를 분리합니다.

`ExecutionMode`는 서명, 브로드캐스트, receipt 추적의 transport ownership입니다.

- `managed_execution`
- `detached_signing`
- `detached_execution`

`ExecutionPhase`는 유스케이스가 어떤 단계까지 진행되는지를 뜻합니다.

- `observe`
- `propose`
- `approve`
- `execute`
- `reconcile`
- `report`

따라서 `read-only`는 mode가 아니라 `observe + report`입니다. `plan + approve`도 mode가 아니라 `observe + propose + approve`입니다.

## 3. 유스케이스 분류

초기 유스케이스는 크게 다섯 종류로 나뉩니다.

### 3.1 Observe

아직 실행하지 않고 계속 상태만 읽고 판단 재료를 만드는 일.

예:

- 특정 지갑 잔고 감시
- allowance 감시
- 포지션 감시
- 이벤트 감시
- 브리지 상태 감시

### 3.2 Propose

실행 계획을 만들지만 마지막 실행은 사람이나 외부 executor가 맡는 일.

예:

- allowance revoke 제안
- treasury rebalance proposal 생성
- Safe proposal용 execution envelope 생성

### 3.3 Execute

조건이 맞으면 런타임 또는 외부 executor가 실제 실행하는 일.

예:

- gas top-up
- idle fund sweep
- allowance 부족 시 approve 후 액션 수행

### 3.4 Reconcile

단발 실행이 아니라 desired state를 유지하는 일.

예:

- 특정 비중 유지
- hedge exposure 유지
- 잔고 threshold 유지

### 3.5 Workflow

여러 단계가 연결된 멀티스텝 운영 자동화.

예:

- approve -> deposit
- bridge -> wait -> execute
- propose -> external sign -> execute -> reconcile

## 4. 초기 대표 유스케이스 10개

### P0 - 가장 먼저 검증할 것

#### 1. 잔고 / allowance / 포지션 감시

하는 일:

- 특정 account의 token balance를 감시
- allowance 상태를 감시
- 특정 contract position을 읽음
- 이상 상태를 로그나 알림으로 남김

왜 중요한가:

- 실행 권한 없이도 `source`, `condition`, `state`, `logs` 모델을 검증할 수 있다
- 가장 낮은 리스크로 runtime shape를 확인할 수 있다
- `TickInputSnapshot`과 source read memoization을 먼저 검증할 수 있다

필요한 것:

- `source.erc20_balance`
- `source.allowance`
- `source.chain_call`
- `condition.*`
- `action.notify`

추천 phase:

- `observe`
- `report`

추천 mode:

- 없음. 온체인 실행이 없으므로 transport mode가 필요하지 않다.

#### 2. Approval Hygiene

하는 일:

- 특정 spender에 대한 allowance가 너무 크거나 오래된 경우 감지
- revoke 또는 재설정 action을 제안하거나 실행

왜 중요한가:

- 실제 수요가 분명하다
- selector allowlist, contract allowlist, token spend limit 모델을 검증하기 좋다
- `NormalizedAction`, simulation, policy gate를 작게 검증할 수 있다

필요한 것:

- `source.allowance`
- `condition.gt`
- `action.erc20_approve`
- `NormalizedAction`
- policy gate

추천 phase:

- 초기: `observe + propose + approve`
- 이후: `observe + propose + approve + execute + report`

추천 mode:

- 초기: 없음 또는 `detached_execution`
- 이후: `managed_execution`

#### 3. Gas Top-Up Keeper

하는 일:

- account의 native gas가 임계치 아래로 떨어지면 자동으로 소액 top-up

왜 중요한가:

- 멀티 account 운영에서 실제로 자주 필요한 자동화다
- 단순 transfer 기반이라 MVP에 적합하다
- budget reservation, policy, account 경계 검증에 좋다

필요한 것:

- `source.native_balance`
- `condition.lt`
- `action.native_transfer`
- account budget
- budget reservation
- nonce lane

추천 phase:

- `observe + propose + approve + execute + report`

추천 mode:

- `managed_execution`

#### 4. Idle Funds Sweep

하는 일:

- wallet에 일정 이상 놀고 있는 자금이 있으면 지정한 운영 account나 vault로 이동

왜 중요한가:

- transfer 중심의 가장 단순한 실행형 유스케이스다
- structured execution report와 journal을 검증하기 좋다

필요한 것:

- `source.erc20_balance`
- `condition.gte`
- `action.contract_call`
- `action.notify`
- budget reservation

추천 phase:

- `observe + propose + approve + execute + report`

추천 mode:

- `managed_execution`

### P1 - 이 프로젝트의 본질을 보여주는 것

#### 5. Bridge Then Act

하는 일:

- 브리지 상태를 감시
- fill 완료를 확인한 뒤 다음 액션 실행

예:

- Arbitrum -> Base 브리지 완료 후 target contract call

왜 중요한가:

- 멀티스텝 workflow를 검증할 수 있다
- state persistence와 retry가 필수라 런타임다운 특성이 드러난다
- `ExternalExecutionResult` ingestion을 실제로 검증할 수 있다

필요한 것:

- `source.bridge_status`
- `condition.eq`
- `flow.sequence`
- `flow.retry`
- `action.contract_call`
- `ExecutionEnvelope`
- `ExternalExecutionResult`

추천 phase:

- `observe + propose + execute + report`
- 실패 시 `reconcile`

추천 mode:

- 초기: `detached_signing`
- 이후: `managed_execution`

#### 6. Idle Stablecoin Supply

하는 일:

- 일정 이상 놀고 있는 stablecoin을 lending market에 공급

예:

- Aave
- Morpho

왜 중요한가:

- capability 기반 앱 확장 모델을 검증할 수 있다
- approve + deposit 시퀀스를 잘 보여준다
- capability output이 `NormalizedAction`으로 컴파일되는 과정을 검증하기 좋다

필요한 것:

- `cap:erc20`
- `cap:aave` 또는 `cap:morpho`
- `action.erc20_approve`
- `action.contract_call`
- expected effect annotation

추천 phase:

- 초기: `observe + propose + approve`
- 이후: `observe + propose + approve + execute + report`

추천 mode:

- 초기: `detached_execution`
- 이후: `managed_execution`

#### 7. Yield Rebalance

하는 일:

- 두 프로토콜의 조건을 비교하고 자금을 이동 제안 또는 실행

예:

- protocol A의 공급 금리가 protocol B보다 충분히 낮으면 migrate

왜 중요한가:

- `source -> transform -> condition -> action sequence` 전체를 보여준다
- strategy-local memory와 cooldown도 같이 검증하기 좋다

필요한 것:

- price or rate source
- `transform.percent_change` 또는 비교 transform
- `condition.*`
- `flow.sequence`
- account budget
- cooldown state

추천 phase:

- 초기: `observe + propose + approve`

추천 mode:

- `detached_execution`

#### 8. Delta / Exposure Rebalance

하는 일:

- 특정 exposure가 허용 범위에서 벗어나면 hedge 또는 unwind

왜 중요한가:

- desired state vs observed state 모델의 핵심 사례다
- 이 프로젝트가 executor보다 runtime에 가깝다는 점을 가장 잘 보여준다

필요한 것:

- `source.position`
- `transform.*`
- `condition.*`
- `action.contract_call` 또는 external execution
- persistent strategy state
- account-level risk budget

추천 phase:

- `observe + propose + approve + execute + reconcile + report`

추천 mode:

- `detached_execution`

### P2 - 팀/기관 방향

#### 9. Safe Proposal Automation

하는 일:

- 전략이 execution envelope를 만들고 Safe proposal 또는 외부 signer flow로 넘김

왜 중요한가:

- signer/broadcast 분리 철학을 잘 보여준다
- 팀 환경, approval flow, external execution mode 검증에 좋다

필요한 것:

- `action.external_execution`
- `execution.externalize`
- `execution.report_result`
- `ExecutionEnvelope`
- `ExternalExecutionResult`
- account-level policy

추천 phase:

- `observe + propose + approve + execute + report`

추천 mode:

- `detached_execution`

#### 10. Treasury Policy Guard

하는 일:

- treasury account를 계속 감시
- 정책 위반 시 자동 pause, notify, proposal 생성

예:

- 특정 token exposure 초과
- 특정 wallet gas 부족
- 특정 protocol allowance 과다

왜 중요한가:

- 운영 자동화, 감사, 다중 계정, policy, runtime control이 모두 만나는 사례다

필요한 것:

- `source.*`
- `condition.*`
- `action.notify`
- `strategy.pause`
- `action.external_execution`
- account-scoped policy

추천 phase:

- `observe + propose + approve + reconcile + report`

추천 mode:

- 기본은 없음. 외부 proposal을 넘길 때만 `detached_execution`

## 5. 현실적인 초기 우선순위

처음부터 모든 것을 다 구현하려고 하면 레포가 금방 다시 커진다.

가장 현실적인 MVP 3개는 다음과 같다.

### MVP 1. 잔고 / allowance 감시

왜:

- 가장 안전하다
- JS/ctx/source 모델을 검증할 수 있다
- 실행기 없이도 가치를 준다
- `TickInputSnapshot`을 가장 먼저 검증할 수 있다

### MVP 2. Approval Hygiene

왜:

- action graph, normalization, simulation, policy 모델을 검증할 수 있다
- 사용자에게도 바로 이해되는 가치가 있다

### MVP 3. Bridge Then Act

왜:

- 멀티스텝 workflow의 본질을 검증할 수 있다
- state persistence, retry, externalized execution 결과 수집까지 확인 가능하다

## 6. 제품적으로 가장 중요한 축

초기 유스케이스들을 보면 결국 이 프로젝트는 세 축으로 수렴한다.

### Observe

- 상태를 읽는다
- 데이터를 쌓는다
- 조건을 판단한다

### Propose

- 실행 계획을 만든다
- simulation과 policy를 붙인다
- 사람이 승인하거나 외부 executor로 넘긴다

### Execute

- managed 또는 detached 방식으로 실제 실행한다
- 결과를 구조화해 남긴다

이 세 축이 살아 있으면, 지원 앱이 늘어나도 레포의 본질은 흔들리지 않는다.

## 7. 설계에 주는 시사점

이 유스케이스 목록은 단순한 아이디어 모음이 아니라, 설계 방향을 압박하는 기준이다.

이 목록이 의미하는 바:

- `source.*`는 초기에 강해야 한다
- `action.*`는 단순하고 작아야 한다
- `flow.*`는 sequence, branch, retry 정도만 있어도 많은 문제를 푼다
- account-scoped policy와 state는 필수다
- `TickInputSnapshot`은 read-only MVP에서도 필요하다
- `NormalizedAction`은 policy 전 단계의 필수 계약이다
- `ExternalExecutionResult` 모델은 MVP에서도 필요하다
- budget reservation과 nonce lane은 managed execution 전에 필요하다

즉, 이 프로젝트는 "범용 트랜잭션 전송기"보다 "상태를 보고, 계획을 만들고, 조건이 맞으면 조심스럽게 실행하는 런타임"에 훨씬 가깝다.

## 8. 지금 기준의 가장 중요한 결론

초기 구현은 다음 순서로 가는 것이 가장 합리적이다.

1. Read + Alert
2. Plan + Approve
3. Externalized Execution
4. Small Managed Actions
5. Multi-step Workflow
6. Reconcile-style Strategies

이 순서를 따르면 위험을 통제하면서도, `onchain-strategy-mcp`가 실제로 무엇을 위한 런타임인지 자연스럽게 드러난다.
