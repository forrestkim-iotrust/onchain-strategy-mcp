<p align="right"><b>한국어</b> · <a href="./README.md">English</a></p>

# onchain-strategy-mcp

Claude 같은 AI 에이전트가 온체인에서 **직접 손발을 갖게** 해주는 로컬 런타임.

---

## 1. 뭐 하는 거예요?

지금 Claude한테 *"내 USDC를 Aave에 예치해줘"* 하면 어떻게 되죠? Python 코드 한 덩어리 던져주고 "이거 돌려보세요" 합니다. 그 뒤로는 다 사용자 몫이에요 — RPC 셋업, 키 관리, 가스, 시뮬레이션, 영수증 확인, 에러 처리. AI는 *설계자*고, 사용자는 *운영자*. 자동화 하나 굴리려고 셋업하는 데만 반나절씩.

이 런타임이 그 *운영자 자리*를 컴퓨터한테 맡깁니다. AI는 *원하는 결과*만 짧은 자바스크립트로 표현하면, 런타임이 나머지를 다 처리해요:

- 체인 연결, 한 단계씩 시뮬레이션
- 로컬에서 서명 후 브로드캐스트
- 영수증 받고, 모든 결정을 로컬 DB에 기록
- 온체인 이벤트 구독해서 즉시 반응
- 여러 단계를 한 트랜잭션으로 묶기 (EIP-7702)

이제 AI는 *코드를 제안만 하던 자리*에서 **사용자 컴퓨터 안에서 직접 실행되는 코드**를 다루는 위치로 옮겨갑니다. 그리고 그 흐름이 전부 대화로 이뤄집니다.

---

## 2. 뭘 만들 수 있어요?

짧은 자바스크립트로 표현되는 거라면 뭐든 — 사용자가 허락한 범위 안에서. 실제로 굴려본 것들:

- **자동 입금 깔때기** — 지갑에 들어오는 모든 ETH/USDC가 자동으로 USDC가 되어 대출 시장에 예치됨.
- **수익률 로테이터** — 이자율 가장 높은 시장으로 스테이블 이동, 단 가스비보다 이득이 클 때만.
- **이벤트 반응자** — 로그 이벤트, 가격 오라클 업데이트, 토큰 전송, mempool 신호에 strategy가 발화.
- **멀티 프로토콜 가격/APY 비교기** — 임의 컨트랙트를 임의 과거 블록에서 읽어서 즉시 데이터셋 구성.
- **원자적 멀티스텝** — approve + supply, swap + LP 같은 거 한 트랜잭션에 묶기 (EIP-7702).

기본 제공 트리거:

| 모드 | 언제 발화하는가 |
|---|---|
| `manual` | 사용자(또는 Claude)가 시킬 때 |
| `interval` | N ms마다 — cron 스타일 |
| `log` | 확정된 블록에서 매칭 로그가 나올 때 |
| `mempool` | mempool에 매칭되는 pending tx가 뜰 때 (Alchemy WSS) |
| 예약 | `block`, `webhook` — v1.3에서 연결 |

---

## 3. 어떻게 써요?

준비물: Mac 또는 Linux, [Node.js 18+](https://nodejs.org/), [Claude Code](https://claude.ai/code). Rust도 Foundry도 필요 없음.

```bash
# 1. 한 줄 설치 (prebuilt 바이너리 다운로드 + burner 지갑 OS 키체인에 생성
#    + .local/config.toml + .local/policy.toml 스캐폴드까지)
npx onchain-strategy-mcp init

# 2. Claude Code에 붙이기 (init이 이 명령어를 그대로 출력해줍니다)
claude mcp add osmcp -- npx onchain-strategy-mcp serve
```

끝. Claude Code 열고:

> `examples/strategies/yield-snapshot.js` 등록해서 한 번 돌리고 결과 보여줘.

Claude가 MCP 도구로 strategy 등록·실행하고, journal에 남은 결과가 채팅에 그대로 나옵니다. 여기서부터는 strategy 더 짜고, 트리거 붙이고, 흐름을 말로 다 조립하면 됩니다.

<details>
<summary>소스에서 직접 빌드 (고급)</summary>

prebuilt 바이너리 대신 Rust 소스에서 직접 빌드하고 싶다면:

```bash
git clone https://github.com/forrestkim-iotrust/onchain-strategy-mcp.git
cd onchain-strategy-mcp
cargo build --release --bin executor-mcp

cast wallet new                       # burner 지갑 생성, 소액만
export EXECUTOR_PRIVATE_KEY=0x여기에키

cp -R .local.example .local
$EDITOR .local/config.toml            # RPC와 signer env 변수명
$EDITOR .local/policy.toml            # 에이전트 권한

claude mcp add osmcp \
  -e EXECUTOR_CONFIG=$PWD/.local/config.toml \
  -e EXECUTOR_PRIVATE_KEY=$EXECUTOR_PRIVATE_KEY \
  -- $PWD/target/release/executor-mcp
```

[Rust](https://rustup.rs/) 필요. `cast` 쓰려면 [Foundry](https://book.getfoundry.sh/)도.
</details>

---

## 4. 실제로 굴려본 시나리오들

### A. 자동 입금 깔때기

Claude한테: *"내 burner에 ETH나 USDC 들어오면 자동으로 USDC로 바꿔서 Aave에 예치해줘. 가스용 ETH는 $0.10어치만 남기고."* Claude가 strategy랑 log 트리거 두 개를 등록하면 깔때기가 알아서 돌아갑니다. 지갑에 도착하는 모든 자금이 이자를 받기 시작해요. 신경 쓸 건 주소로 돈 보내는 거 하나뿐.

### B. 수익률 비교기

Claude한테: *"지난 30일 Aave / Compound / Moonwell USDC 이자율 시간 단위로 비교해줘."* Claude가 짧은 view 함수를 짜서 archive RPC로 과거 블록을 훑어 표로 즉시 줍니다. 기다림 없음 — 데이터는 이미 온체인에 있어요.

이어서: *"이제부터 계속 보다가 Moonwell이 5% 넘으면 알려줘."* 별도 명령 — Claude가 주기적 체크나 이벤트 트리거를 붙입니다.

같은 런타임의 두 모드: **과거는 호출 한 번, 실시간 감시는 트리거.**

### C. 즉각 반응자

Claude한테: *"Aave 오라클이 ETH 가격 업데이트하는 걸 봐. 한 번에 2% 이상 떨어지면 내 borrow 갚아."* Claude가 로그 이벤트 트리거를 등록해두면, 그 정확한 이벤트가 발화하는 순간 strategy가 초 단위로 돕니다.

### D. 원자적 멀티스텝

Claude한테: *"내 burner USDC 0.1을 Aave에 예치해줘."* Claude가 `[approve, supply]` 두 action을 반환하면, 런타임이 멀티스텝임을 감지해서 자동으로 EIP-7702 한 트랜잭션에 묶어 broadcast합니다. 둘 다 한 번에 들어가거나 둘 다 안 들어가요 — approve와 사용 사이의 위험 구간이 사라집니다.

---

## 5. FAQ

**Q. 어떤 체인에서 돼요?**
Base(L2)에서 빌드·테스트했습니다. EVM 호환 체인이면 config 한두 줄 바꾸는 걸로 다 됩니다 — 이더리움 메인넷, Arbitrum, Optimism, Polygon 등. Solana나 다른 non-EVM은 로드맵에 있어요.

**Q. 비용은 얼마나 들어요?**
소프트웨어는 무료. 온체인 트랜잭션은 가스비가 듭니다. Base는 보통 액션당 $0.10 미만. 더 큰 비용은 지갑 자체 — $5~10어치 ETH랑 운영할 자산 소액으로 시작하세요.

**Q. 프로그래밍 할 줄 알아야 해요?**
조금. 명령어 복붙이랑 config 파일 한 번 편집할 정도. Strategy 본체는 짧은 자바스크립트인데, Claude한테 뭐 하고 싶은지 말로 설명하면 알아서 짜줍니다.

**Q. API 키 필요해요?**
- 기본 사용은 필요 없음. 공개 RPC로 OK.
- mempool 감시, 실시간 이벤트 듣기, 며칠 넘는 과거 데이터 조회는 [Alchemy](https://www.alchemy.com/) 키가 있으면 좋아요. 무료 tier로 취미용 충분.

**Q. "MCP"가 뭐예요?**
Model Context Protocol — Claude Code (랑 비슷한 AI 클라이언트)가 외부 프로그램이랑 대화하는 표준 방식. 이 프로젝트가 그런 외부 프로그램 중 하나고, "Claude Code에 추가한다"는 건 *Claude야, 이 친구랑 얘기할 수 있어*라고 알려주는 거예요.

**Q. 권한·정책 모델은 어떻게 동작해요?**
런타임은 deny-by-default 정책 DSL이 기본으로 들어 있어요 (허용 체인, 컨트랙트, 함수 셀렉터, native value 한도, ERC20 지출 한도). 정책에 없는 건 서명 전에 거부됩니다. 단순한 baseline이고, 더 정교한 권한 분리 (세션 키, 에이전트 전용 지갑)는 아래 로드맵에 있어요.

**Q. 버그 있거나 궁금한 게 있으면요?**
[issue 등록](https://github.com/forrestkim-iotrust/onchain-strategy-mcp/issues). 보안 관련은 GitHub private security advisory 기능 쓰세요.

---

## 로드맵

지금 되는 건 위 §2에 있고, 최근 릴리즈:

- ✅ **Claude Code 한 줄 설치** *(v1.3)* — `npx onchain-strategy-mcp init` 한 번으로 burner 지갑(OS 키체인) + config + policy 스캐폴드. CREATE2 deterministic BatchExec 주소라 7702 batching이 어느 체인에서든 작동 (필요시 `deploy-delegate` 한 번만). `cargo build`도, `cast`도, 수동 `claude mcp add`도 없음.

앞으로 만들 것:

- **Self-documenting MCP 세션.** 서버의 `instructions`, prompts, resource template를 풍부하게 채워서 — 예시, 트리거 패턴, action shape, 흔한 함정까지 — 새 Claude Code 세션이 *처음부터* 뭘 할 수 있는지 자기가 압니다. "기능 있는 줄 몰라서 못 쓰는" 게 없게.
- **제품 홈페이지.** 이게 뭔지, 대표 유즈케이스가 뭔지, 설치를 어떻게 하는지를 브라우저에서 바로 안내하는 단순한 랜딩. 복붙 가능한 커맨드, 실제 Claude Code 세션 스크린샷, 예시 링크까지. 터미널 열기 전에 "이거 뭐 하는 거지?" 단계를 먼저 해결.
- **외부 오라클 트리거 + 데이터 소싱.** Chainlink / Pyth / Redstone 가격 업데이트, 오프체인 데이터 피드, 임의 HTTPS 웹훅으로 strategy 발화. 에이전트가 온체인 상태뿐 아니라 *현실 세계* 신호에도 반응할 수 있게.
- **WaaS (Wallet-as-a-Service) 통합.** Privy, Turnkey, Coinbase MPC 같은 에이전트 전용 지갑 솔루션을 1급으로 연결. 권한 분리(세션 키, 계정별 정책, 키 로테이션, 복구)를 지갑 레이어가 책임지게 — burner + 로컬 정책은 1인 운영자용, WaaS는 팀·프로덕션·멀티테넌트용.
- **AMM 아닌 거래소 통합.** Hyperliquid (perps), Polymarket (예측시장) 같은 오더북·전문 거래소 1급 지원. 에이전트가 주문, 포지션 관리, 시장 결제까지.
- **크로스체인 실행 + 브릿지 통합.** Across 같은 canonical 브릿지 어댑터, 그리고 strategy 파일 하나로 여러 체인을 가로지르는 흐름 표현 가능 (예: *Base Aave에서 인출 → USDC를 Arbitrum으로 브릿지 → 거기 Aave에 다시 예치*). EIP-7702 batch처럼 원자적이지 않은 만큼, 런타임이 각 단계를 분리된 커밋 단위로 처리하면서 매 경계마다 명시적 fallback 의미(재시도, 환불 경로, 중단)를 부여.
- **non-EVM 체인.** Solana 먼저, 다른 생태계 (Move, CosmWasm, Stellar Soroban)를 위한 추상화. 같은 strategy / policy / journal 모델, signer와 RPC 백엔드만 다르게.

날짜보다 방향이 중요한 과제들이에요. [issue](https://github.com/forrestkim-iotrust/onchain-strategy-mcp/issues)로 토론·PR 환영합니다.

---

## License & credits

Apache 2.0. [rmcp](https://github.com/modelcontextprotocol/rust-sdk), [alloy](https://github.com/alloy-rs/alloy), [rquickjs](https://github.com/DelSkayn/rquickjs), [foundry](https://github.com/foundry-rs/foundry) 위에 만들었어요. 로컬 우선 — 운영하는 서버 없고, 만들 계정도 없습니다.

아키텍처 자세한 내용 (크레이트 구조, 트리거 파이프라인, EIP-7702 사양)은 `crates/` 소스랑 `examples/contracts/BatchExec.sol` 보세요.
