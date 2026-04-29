# onchain-strategy-mcp

에이전트가 온체인 전략 프로그램을 만들고, 실행하고, 관리할 수 있게 해주는 MCP 전략 런타임입니다.

## 이 프로젝트는 무엇인가

`onchain-strategy-mcp`는 제품 화면이 아니라 전략 런타임입니다.

이 런타임은 에이전트가 다음을 할 수 있게 만드는 데 목적이 있습니다.

- 전략 생성
- account 경계에 전략 등록
- 스케줄 또는 이벤트 기반 실행
- 감사 가능한 action graph 생성
- 안전한 실행 또는 외부 실행으로의 위임
- 이후 상태, 로그, 리포트, receipt 조회

이 저장소는 다음이 아닙니다.

- 트레이딩 앱
- 지갑 UI
- 대시보드
- 클라우드 배포 시스템
- 전략 마켓플레이스

## 현재 설계 관점

현재 설계는 여섯 가지 기준 위에 서 있습니다.

### 1. 런타임은 앱을 모른다

런타임은 Aave, Uniswap, Safe, Across 같은 앱을 제품 단위로 이해하지 않아야 합니다.

앱은 capability이고, 실행은 primitive입니다.

즉 런타임은 결국 다음만 이해하면 됩니다.

- source
- transform
- condition
- flow
- action
- policy
- execution report

### 2. JavaScript는 전략 DSL이다

전략은 샌드박스된 JavaScript 함수로 작성할 수 있습니다.

하지만 JavaScript가 실행 권한자는 아닙니다. 전략 함수는 `ctx`를 통해 상태를 읽고, 판단하고, 구조화된 action graph를 반환합니다. 실행은 런타임이 담당합니다.

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

핵심 경계는 명확합니다.

- 전략 코드는 action을 제안할 수 있다
- 전략 코드는 직접 서명하거나 브로드캐스트할 수 없다
- action graph는 policy나 execution 전에 normalized action으로 컴파일되어야 한다

### 3. 런타임은 실행 규율을 책임진다

런타임은 다음을 책임집니다.

- 전략 검증
- 샌드박스 tick 실행
- action 정규화
- 실행 전 시뮬레이션
- policy 평가
- 실행 상태 관리
- journal과 report 저장
- pause / stop 같은 런타임 제어

반대로 private key custody를 반드시 소유할 필요는 없습니다.

### 4. 실행 운송 수단은 교체 가능해야 한다

기본 철학은 full runtime execution이지만, 서명과 브로드캐스트는 adapter 기반으로 분리 가능해야 합니다.

개념적 실행 모드:

- `managed_execution`
- `detached_signing`
- `detached_execution`

즉 런타임은 실행 규율을 책임지되, 모든 transport boundary를 항상 직접 소유할 필요는 없습니다.

실행 모드는 transport ownership입니다. 실행 단계와는 분리합니다.

개념적 실행 단계:

- `observe`
- `propose`
- `approve`
- `execute`
- `reconcile`
- `report`

### 5. Account가 실행 경계다

핵심 모델은 account 중심입니다.

```text
Account
  StrategyInstance
    Execution
      Action
```

여기서 account는 단순 wallet 주소가 아닙니다.

account는 다음의 경계입니다.

- signer ref
- policy
- budget
- budget reservation
- nonce lane
- execution lock
- execution mode
- chain별 주소
- strategy-local state

### 6. Recipe보다 계약이 먼저다

런타임은 고수준 프로토콜 recipe보다 오래 유지될 수 있는 계약을 먼저 노출해야 합니다.

핵심 계약:

- `TickInputSnapshot`: 전략이 판단할 때 본 입력
- `NormalizedAction`: action graph에서 컴파일된 policy-ready action
- `ExecutionEnvelope`: 외부화 가능한 실행 요청
- `ExternalExecutionResult`: 외부 executor가 보고하는 실행 결과

## 실행 생명주기

모든 action은 같은 파이프라인을 지나야 합니다.

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

런타임은 prose-only 로그보다 구조화된 실행 리포트를 우선해야 합니다.

## Primitive 모델

런타임이 작게 유지되려면 조합 가능한 primitive 중심이어야 합니다.

개념적 primitive group:

- `source.*`
- `transform.*`
- `condition.*`
- `flow.*`
- `action.*`
- `policy.*`
- `execution.*`

`cap:erc20`, `cap:aave` 같은 capability는 action graph를 만드는 helper입니다. 실행 권한자는 아닙니다.

## 대표 유스케이스

이 런타임의 목적은 단순히 "트랜잭션 한 번 보내기"보다 넓습니다.

대표적인 범주:

- observe: 잔고, allowance, 이벤트, 포지션 감시
- propose: 실행 계획, revoke proposal, Safe proposal 생성
- execute: gas top-up, idle fund sweep, 작은 managed action
- reconcile: 비중, 노출도, 잔고 threshold 유지
- workflow: bridge then act, approve then deposit, external sign then execute

## MCP 표면

공개 MCP 표면은 작고 안정적이어야 합니다.

현재 개념적 group:

- `account.*`
- `strategy.*`
- `execution.*`
- `policy.*`
- `opcode.*`

먼저 오래 유지될 수 있는 계약을 만들고, 그 다음 편의용 recipe를 올리는 방식이 맞습니다.

## 하지 않을 일

이 저장소는 다음으로 커지지 않아야 합니다.

- 랜딩 페이지
- 대시보드
- 지갑 온보딩 UX
- 클라우드 오케스트레이션
- 분석 제품 화면
- core runtime 안에 박힌 exchange-specific flow

## 문서 안내

- [README.md](./README.md)
- [FOUNDATIONS_ko.md](./FOUNDATIONS_ko.md)
- [USE_CASES_ko.md](./USE_CASES_ko.md)

## 현재 상태

이 저장소는 아직 설계 단계입니다.

현재 MVP 방향은 다음과 같습니다.

1. JavaScript 전략 검증 및 등록
2. 샌드박스된 `tick(ctx)` 실행
3. `TickInputSnapshot` 저장
4. structured action graph 반환
5. `NormalizedAction` 컴파일
6. EVM action simulation
7. policy와 account budget enforcement
8. pluggable signing / broadcasting mode
9. execution journal과 report 저장
