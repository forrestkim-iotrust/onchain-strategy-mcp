<p align="right"><b>한국어</b> · <a href="./README.md">English</a></p>

# onchain-strategy-mcp

Claude 같은 AI에게 내 지갑을 안전하게 맡기는 방법.

---

## 1. 뭐 하는 거예요?

**AI한테 온체인에서 *실제로* 일하라고 시킬 수 있는 도구입니다.** 지금처럼 "코드만 짜주는 AI" 말고요.

지금 Claude한테 *"ETH 좀 USDC로 바꾸고 Aave에 넣어줘"* 하면 어떻게 되죠? 파이썬 코드 한 덩어리 던져주고 "이거 돌려보세요" 합니다. 그 다음은 다 사람 몫이에요. RPC 연결, 키 관리, 가스 계산, 로그 남기기, 에러 처리. 한 번 자동화하려고 셋업하는 데만 반나절. 결국 사용자가 운영자가 됩니다.

이 프로젝트는 그 운영자 자리를 컴퓨터한테 넘깁니다. AI는 *뭘 하고 싶은지*만 짧은 자바스크립트로 표현하고, 내 노트북에서 조용히 도는 작은 프로그램이 나머지를 다 처리해요:

- 체인에 붙고
- 일단 시뮬레이션해보고
- 내가 만든 규칙(어떤 컨트랙트만, 어떤 함수만, 얼마까지)에 비춰 한 동작씩 검사하고
- 통과한 것만 로컬에서 서명 — private key는 컴퓨터 밖으로 안 나갑니다
- 브로드캐스트하고, 영수증 받고, 모든 결정을 로컬에 기록

체인 위에서 일어나는 일에 *반응*도 합니다. "USDC가 이 지갑으로 들어오면 저 strategy 실행" 같은 거.

한 줄로: **AI는 우리 컴퓨터 안에서 코드를 짜고 운영하는 개발자가 되고, 규칙은 내가 쥡니다.**

---

## 2. 뭘 할 수 있어요?

작은 자바스크립트로 표현되는 거라면 뭐든지 — 단, 내가 허락한 범위 안에서. 실제로 만들어 본 것들:

- **자동 입금.** 지갑에 ETH 들어오면 USDC로 바꿔서 Aave에 넣기. 손 댈 일 없음.
- **자동 리밸런싱.** 가장 이자율 높은 대출 시장으로 USDC 이동 — 가스비보다 이득이 클 때만.
- **이벤트 반응.** Uniswap에서 큰 매수가 일어나면 X 하기. Aave USDC 이자율이 5% 넘으면 예치하기.
- **수익률 비교.** Aave / Compound / Moonwell 같은 곳들 이자율을 한 화면에서 비교. 과거 30일치도 즉시.
- **과거 분석.** 어떤 컨트랙트의 임의 시점 상태 읽기. 30일 APY 차트가 5분이면 만들어짐.
- **여러 단계 한 트랜잭션으로 묶기.** approve + supply 같은 거 한 번에. (EIP-7702 사용, 스마트 지갑 없어도 됨)

자동 실행 방식 (트리거):

- 내가 Claude한테 시킬 때 (수동)
- N분마다 (스케줄)
- 온체인에서 뭔가 일어났을 때 — 입금, 전송, 가격 업데이트, 특정 이벤트
- mempool에 새 트랜잭션이 떴을 때 (체인에 따라 의미 다름)

---

## 3. 어떻게 써요?

준비물: Mac/Linux, [Rust](https://rustup.rs/), [Foundry](https://book.getfoundry.sh/), [Claude Code](https://claude.ai/code).

### 1단계 — 코드 받고 빌드

```bash
git clone https://github.com/forrestkim-iotrust/onchain-strategy-mcp.git
cd onchain-strategy-mcp
cargo build --release --bin executor-mcp
```

### 2단계 — burner 지갑 하나 만들기

AI가 만질 지갑입니다. Base 같은 데에 몇 달러어치 ETH만 넣어두세요. **모아둔 돈을 여기 넣지 마세요.**

```bash
cast wallet new
# 주소랑 private key 어디 적어두고, 위 주소로 소액 ETH 보내기.
export EXECUTOR_PRIVATE_KEY=0x여기에키
```

### 3단계 — 규칙 쓰기

운영자 설정 예제를 복사해서 편집:

```bash
cp -R .local.example .local
# .local/config.toml — 지갑, RPC 정보
# .local/policy.toml — AI가 만질 수 있는 컨트랙트와 한도
```

policy는 짧은 텍스트 파일이고 기본은 "다 막힘". 허용할 거 있으면 그것만 한 줄씩 명시: 이 컨트랙트의 이 함수, 이만큼까지.

### 4단계 — Claude Code에 연결

```bash
claude mcp add osmcp \
  -e EXECUTOR_CONFIG=$PWD/.local/config.toml \
  -e EXECUTOR_PRIVATE_KEY=$EXECUTOR_PRIVATE_KEY \
  -- $PWD/target/release/executor-mcp
```

### 5단계 — Claude한테 시켜보기

Claude Code 안에서 이렇게 말합니다:

> 내 지갑 잔액 보여주고, `examples/strategies/yield-snapshot.js` 등록해서 한 번 돌려봐.

Claude가 이 프로젝트가 제공하는 도구로 지갑 읽고, strategy 등록하고, 실행합니다. 결과는 채팅창에 바로 나옵니다.

이게 기본 루프예요. 다음부터는 strategy 더 짜고, 트리거 붙이고, 다 말로 시키면 됩니다.

---

## 4. 실제로 굴려본 시나리오들

### A. 자동 입금 깔때기

한 번만 셋업해두면, burner 지갑에 들어오는 모든 ETH나 USDC가 자동으로 USDC가 되어 Aave에 들어갑니다. 가스용 ETH 약간만 남기고. 그 뒤로는 신경 안 써도 — 도착하는 모든 자금이 이자를 받기 시작합니다.

내가 할 일은 지갑 주소로 돈 보내는 거 그뿐.

### B. 수익률 비교기

Claude한테: *"지난 30일치 Aave / Compound / Moonwell의 USDC 이자율을 시간 단위로 비교해줘."* Claude가 작은 view 함수를 짜서 archive RPC로 과거 블록을 읽어 표로 즉시 줍니다. 기다림 없음 — 데이터는 이미 온체인에 있어요.

이어서: *"이제부터는 계속 보고 있다가 Moonwell이 5% 넘으면 알려줘."* 별도 명령. Claude가 주기적 체크나 온체인 이벤트 트리거를 붙입니다. 앞으로의 변동도 놓치지 않게.

같은 런타임 두 가지 모드: **과거는 한 번의 호출**, **실시간 감시는 트리거**. 필요한 거 쓰면 됩니다.

### C. 즉각 반응자

Claude한테: *"Aave 오라클이 ETH 가격 업데이트할 때 봐줘. 한 번에 2% 이상 빠지면 내 borrow 갚아."* Claude가 log 이벤트 트리거를 등록. 그 정확한 이벤트가 온체인에서 발화하는 순간 strategy가 초 단위로 돕니다.

### D. 원자적 멀티스텝

어떤 동작들은 같이 일어나거나 아예 일어나지 말아야 합니다 (approve 후 사용처럼). 보통은 두 개의 트랜잭션, 그 사이에 위험 구간이 있어요. 이 프로젝트는 EIP-7702(2025년 이더리움 기능)로 둘을 하나의 트랜잭션에 묶습니다. 둘 다 들어가거나 둘 다 안 들어가거나.

---

## 5. FAQ

**Q. 내 돈 안전한가요?**

에이전트는 policy 파일이 허락한 것만 할 수 있어요. "Aave에 USDC 예치"만 허용했다면 토큰 매도도 못 하고, 임의 컨트랙트 approve도 못 하고, ETH도 아무 데나 못 보내요. private key는 환경변수로 내 컴퓨터에만 있고, 에이전트는 그걸 못 봐요.

다만 — policy의 안전성은 내가 쓴 만큼이에요. 좁게 시작하세요. $5로 테스트한 다음 늘리세요.

**Q. AI가 잘못 판단해서 돈 잃을 수도 있나요?**

거래나 시장 상호작용을 허용하면 정상적인 시장 손실은 일어날 수 있어요. 이 프로젝트가 막아주는 건 *권한 밖의 행동* (이상한 주소로 송금, 환각된 악성 컨트랙트)이지 *시장 타이밍 실수*는 아니에요.

**Q. 어떤 체인에서 돼요?**

Base(L2)에서 빌드·테스트했어요. EVM 호환 체인이면 config 한두 줄 바꾸는 걸로 다 됩니다 — 이더리움 메인넷, Arbitrum, Optimism, Polygon 등.

**Q. 비용은 얼마나 들어요?**

소프트웨어는 무료. 온체인 트랜잭션은 가스비 듭니다. Base는 보통 액션당 $0.10 미만. 더 큰 비용은 burner 지갑 자체 — $5~10어치 ETH랑 운영할 자산 소액으로 시작하세요.

**Q. 프로그래밍 할 줄 알아야 해요?**

기본은: 조금. 명령어 복붙하고 config 파일 한 번 편집할 수 있어야 해요. Strategy 본체는 짧은 자바스크립트인데, Claude한테 뭐 하고 싶은지 말하면 알아서 짜줘요.

**Q. API 키 필요해요?**

- 기본 사용엔 필요 없어요. 공개 RPC로 충분.
- mempool 감시, 실시간 이벤트 듣기, 며칠 넘는 과거 데이터 조회는 [Alchemy](https://www.alchemy.com/) 키가 있는 게 좋아요. 무료 tier로 취미용 충분합니다.

**Q. "MCP"가 뭐예요?**

Model Context Protocol. Claude Code(나 비슷한 AI 클라이언트)가 외부 프로그램이랑 대화하는 표준 방식이에요. 이 프로젝트가 그런 외부 프로그램 중 하나고요. "Claude Code에 추가한다"는 건 *Claude야, 이 친구랑 얘기할 수 있어*라고 알려주는 거.

**Q. 그냥 봇 만들면 되지 않아요?**

봇은 사람이 미리 코드로 짜는 거예요. 이건 AI가 대화하면서 작은 strategy를 즉석에서 짜고 돌리는 동안, 단단한 정책이 권한 밖 행동을 막아주는 구조예요. 봇을 대체한다기보다는 "AI한테 온체인 일을 시키는 진입 장벽을 낮추는" 쪽에 가깝습니다.

**Q. 이거로 돈 벌어요?**

아니요. 런타임이지 alpha 생성기가 아니에요. 사용자(또는 사용자가 신뢰하는 AI)가 시킨 일을 합니다. 알아서 돈 버는 거래를 찾아내진 않아요. 그렇게 광고하는 건 다 사기.

**Q. 버그 있거나 궁금한 거 있으면요?**

[issue 등록](https://github.com/forrestkim-iotrust/onchain-strategy-mcp/issues). 보안 관련은 GitHub private advisory 기능 써주세요.

---

## 로드맵

지금 되는 건 위 §2에 있고, 앞으로 만들 것:

- **외부 오라클 트리거 + 데이터 소싱.** Chainlink / Pyth / Redstone 가격 업데이트, 오프체인 데이터 피드, 임의 HTTPS 웹훅으로 strategy 발화. 에이전트가 온체인 상태뿐 아니라 *현실 세계* 신호에도 반응할 수 있게.
- **AMM 아닌 거래소 통합.** Hyperliquid(perps), Polymarket(예측시장) 같은 오더북·전문 거래소 1급 지원. 에이전트가 주문 내고, 포지션 관리하고, 시장 결제까지 — 모두 같은 정책 게이트 아래.
- **non-EVM 체인.** Solana 먼저, 그 다음 다른 생태계(Move, CosmWasm, Stellar Soroban)를 위한 깨끗한 추상화. 같은 strategy / policy / journal 모델, 다른 signer + RPC 백엔드만.

큰 방향들이에요. 날짜보다 방향이 중요 — [issue](https://github.com/forrestkim-iotrust/onchain-strategy-mcp/issues)로 토론·PR 환영합니다.

---

## License & credits

Apache 2.0. [rmcp](https://github.com/modelcontextprotocol/rust-sdk), [alloy](https://github.com/alloy-rs/alloy), [rquickjs](https://github.com/DelSkayn/rquickjs), [foundry](https://github.com/foundry-rs/foundry) 위에 빌드했어요. 전부 로컬에서 도는 구조 — 우리가 운영하는 서버 없고, 만들어야 할 계정도 없어요.

아키텍처 자세한 내용 (크레이트 구조, 트리거 파이프라인, EIP-7702 사양)은 `crates/` 소스랑 `examples/contracts/BatchExec.sol` 보세요.
