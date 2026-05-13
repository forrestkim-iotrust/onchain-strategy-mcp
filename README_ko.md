<p align="right"><b>한국어</b> · <a href="./README.md">English</a></p>

# onchain-strategy-mcp

AI 에이전트(예: Claude)에게 내 지갑을 *안전하게* 맡기는 방법.

---

## 1. 이게 뭐예요?

Claude한테 이렇게 시킨다고 상상해보세요: *"이 지갑에 ETH가 들어오면 USDC로 바꿔서 Aave에 예치해줘."*

지금까지 이걸 자동화할 방법은 두 가지뿐:

1. **AI한테 private key를 넘기기** — 한 번 환각하거나 prompt injection 한 방이면 다 털림.
2. **트랜잭션마다 사람이 승인** — 자동화가 아니라 그냥 느린 카피페이스트.

이 프로젝트가 **세 번째 길**입니다: AI는 *뭘 할지*만 제안하고, 본인이 *규칙*을 정하고, 내 컴퓨터 안의 작은 프로그램이 모든 행동을 규칙에 비춰 검사한 후에야 서명한다.

AI는 private key를 절대 못 봅니다. 서명은 내 컴퓨터가 합니다. 규칙 밖의 모든 행동은 조용히 거부됩니다.

---

## 2. 뭘 할 수 있어요?

작은 JavaScript 함수로 표현할 수 있는 거라면 뭐든지 — 단, 본인이 정한 한도 내에서. 이미 실제로 만들어 본 것들:

- **자동 예치.** "이 지갑에 ETH가 도착하면 USDC로 바꿔서 Aave에 넣어." 알아서 돌아감.
- **자동 리밸런싱.** "내 USDC를 가장 이자율 높은 대출 시장으로 옮기되, 가스비 $0.10보다 이득이 클 때만."
- **관측 + 반응.** "Uniswap에 큰 매수가 들어오기 직전이면 X 해." 또는 "Aave USDC 공급 이자율이 5% 넘으면 예치해."
- **여러 프로토콜 수익률 비교.** 30분마다 Aave, Compound, Moonwell APY 읽어서 기록. 이틀 후엔 데이터셋 완성.
- **과거 데이터 분석.** 임의의 과거 블록 시점의 프로토콜 상태 읽기. 30일치 APY 차트 5분이면 백필.
- **여러 단계를 한 트랜잭션으로.** approve + supply 한 번에. (EIP-7702 사용, 스마트 지갑 필요 없음)

트리거 — 언제 자동으로 도느냐:

- **Claude한테 시킬 때** (수동)
- **N분마다** (스케줄)
- **온체인에서 뭔가 일어났을 때** — 지갑에 돈이 들어왔다, 토큰이 옮겨졌다, 가격 오라클이 업데이트됐다, 특정 컨트랙트가 이벤트를 emit했다
- **새 트랜잭션이 mempool에 등장** (체인에서 유의미한 경우)

---

## 3. 어떻게 쓰나요?

준비물: Mac/Linux, [Rust](https://rustup.rs/), [Foundry](https://book.getfoundry.sh/), [Claude Code](https://claude.ai/code).

### 1단계 — 코드 받고 빌드

```bash
git clone https://github.com/forrestkim-iotrust/onchain-strategy-mcp.git
cd onchain-strategy-mcp
cargo build --release --bin executor-mcp
```

### 2단계 — burner 지갑 만들기

에이전트가 동작할 작은 지갑. Base(또는 사용하려는 체인)에 몇 달러어치 ETH 넣기. **여기에 모아둔 돈 넣지 말 것.**

```bash
cast wallet new
# 주소와 private key 저장. 위 주소로 소액 ETH 송금.
export EXECUTOR_PRIVATE_KEY=0x여기에키
```

### 3단계 — 규칙 작성

operator config 예제 복사한 뒤 편집:

```bash
cp -R .local.example .local
# .local/config.toml 편집 — 지갑/RPC 정보
# .local/policy.toml 편집 — 에이전트가 만져도 되는 컨트랙트, 한도
```

policy는 짧은 텍스트 파일이고 기본값은 "전부 금지". 허용할 것마다 한 줄씩 명시적으로 추가: 이 컨트랙트, 이 함수, 이 금액까지.

### 4단계 — Claude Code에 연결

```bash
claude mcp add osmcp \
  -e EXECUTOR_CONFIG=$PWD/.local/config.toml \
  -e EXECUTOR_PRIVATE_KEY=$EXECUTOR_PRIVATE_KEY \
  -- $PWD/target/release/executor-mcp
```

### 5단계 — Claude에게 시키기

Claude Code 안에서:

> 내 지갑 잔액 보여주고, `examples/strategies/yield-snapshot.js`를 등록해서 한 번 돌려봐.

Claude가 이 프로젝트가 제공하는 도구로 지갑을 읽고, strategy를 등록하고, 실행합니다. 결과는 채팅창에 나옵니다.

여기서부터는 strategy 더 짜고, 트리거 붙이고, Claude한테 말로 다 시키면 됩니다.

---

## 4. 실제 use case (이미 검증된 것들)

### A. 자동 입금 깔때기

한 번만 설정하면: burner 지갑에 ETH나 USDC가 들어오면 자동으로 USDC로 변환되어 Aave에 예치됨. 가스용 ETH 약간만 남김. 그 뒤로는 손도 안 댐 — 들어오는 모든 자금이 이자를 받음.

본인이 할 일은 지갑으로 돈 보내는 것뿐.

### B. 수익률 비교기

Claude에게: *"지난 30일치 Aave / Compound / Moonwell의 USDC 공급 이자율 시간 단위로 비교해줘."* Claude가 작은 JS view 작성해서 archive RPC로 과거 블록을 읽어 표로 즉시 반환. 기다림 없음, 데이터는 이미 온체인에 있음.

이어서: *"이제부터 계속 모니터링해서 Moonwell이 5% 넘으면 알려줘."* 별도 명령 — Claude가 주기적 체크(또는 온체인 이벤트 트리거)를 붙여서 앞으로의 변동도 놓치지 않음.

두 조각, 같은 런타임: **과거는 한 번의 호출**, **실시간 감시는 트리거**. 본인 필요에 맞게.

### C. 즉각 반응자

Claude에게: *"Aave oracle이 ETH 가격 업데이트 emit하는 거 보고 있다가, 한 번의 업데이트로 2% 이상 떨어지면 내 borrow 상환해."* Claude가 log 트리거 등록. 그 정확한 이벤트가 온체인에 발화하면 strategy가 초 단위로 실행.

### D. 원자적 멀티스텝

어떤 행동들은 같이 일어나거나 둘 다 안 일어나야 함 (approve 후 사용처럼). 보통은 두 트랜잭션 + 사이의 위험 구간. 이 프로젝트는 EIP-7702(2025년 이더리움 기능)로 둘을 한 트랜잭션에 묶음. 둘 다 일어나거나 둘 다 안 일어남.

---

## 5. FAQ

**Q. 내 돈 안전한가요?**
에이전트는 policy 파일이 허락한 것만 할 수 있어요. "Aave에 USDC 예치"만 허용했다면, 토큰 매도도 못 하고, 임의 컨트랙트 approve도 못 하고, ETH 아무 데나 못 보냅니다. private key는 환경변수로 본인 컴퓨터 안에만 — 에이전트는 못 봅니다.

함정: policy의 안전성은 본인이 쓴 만큼이에요. 좁게 시작. $5로 테스트 후 확장.

**Q. AI가 잘못 판단해서 손실 날 수 있나요?**
네 — 거래나 시장 상호작용을 허용하면 정상적인 시장 손실은 발생할 수 있어요. 보호 대상은 *권한 밖 행동* (잘못된 주소로 송금, 환각된 악의 컨트랙트)이지 *시장 타이밍 실수*는 아닙니다.

**Q. 어떤 체인에서 돼요?**
Base(L2)에서 빌드·테스트됨. EVM 호환 체인이면 config 한 줄 수정으로 다 가능 — Ethereum, Arbitrum, Optimism, Polygon 등.

**Q. 비용은?**
소프트웨어는 무료. 온체인 트랜잭션은 가스비 듦. Base는 보통 $0.10 미만/액션. 더 큰 비용은 burner 지갑 자체 — $5~10 ETH + 운영할 자산 소액으로 시작.

**Q. 프로그래머여야 하나요?**
기본 버전은: 약간. 명령어 복붙 + config 파일 한 번 편집할 줄 알아야 함. Strategy 자체는 짧은 JavaScript — Claude가 본인 설명만 들으면 짜줍니다.

**Q. API key 필요해요?**
- 기본 사용: 아뇨, 공개 RPC로 OK.
- mempool 감시, 라이브 이벤트 듣기, 며칠 넘는 과거 데이터 읽기엔 [Alchemy](https://www.alchemy.com/) key 필요. 무료 tier로 취미용 충분.

**Q. "MCP"가 뭐예요?**
Model Context Protocol — Claude Code(와 비슷한 AI 클라이언트)가 외부 프로그램과 대화하는 방식. 이 프로젝트가 바로 그런 외부 프로그램. "Claude Code에 추가"한다는 건 *Claude, 너 이 친구랑 대화할 수 있어*라고 알려주는 것.

**Q. 그냥 봇 만들면 안 돼요?**
봇은 본인이 코드로 직접 짭니다. 이건 AI가 대화로 작은 strategy를 즉석에서 짜고 돌리는 동안, 단단한 policy가 권한 밖 행동을 막아주는 구조. 봇 대체라기보다 "AI한테 온체인 일 시키는 진입 장벽 낮추기"에 가까움.

**Q. 이걸로 부자 되나요?**
아뇨. 런타임이지 alpha 생성기가 아닙니다. 본인이(또는 본인이 신뢰하는 AI가) 시킨 일을 합니다. 알아서 돈 버는 거래를 찾지 않습니다. 그렇게 광고하는 건 다 사기.

**Q. 버그 있거나 질문 있을 때?**
[issue 등록](https://github.com/forrestkim-iotrust/onchain-strategy-mcp/issues). 보안 관련은 GitHub private advisory 기능 사용.

---

## License & credits

Apache 2.0. [rmcp](https://github.com/modelcontextprotocol/rust-sdk), [alloy](https://github.com/alloy-rs/alloy), [rquickjs](https://github.com/DelSkayn/rquickjs), [foundry](https://github.com/foundry-rs/foundry) 위에 빌드됨. 전부 local-first — 우리가 운영하는 서버 없음, 본인이 만들 계정 없음.

아키텍처 세부 (크레이트, 트리거 파이프라인, EIP-7702 사양)는 `crates/` 소스와 `examples/contracts/BatchExec.sol` 참고.
