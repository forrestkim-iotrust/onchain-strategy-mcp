<p align="right"><b>한국어</b> · <a href="./README.md">English</a></p>

# onchain-strategy-mcp

Claude 같은 AI 에이전트가 온체인에서 **직접 손발을 갖게** 해주는 로컬 런타임.

---

## 1. 뭐 하는 거예요?

지금 Claude한테 *"내 USDC를 Aave에 예치해줘"* 하면 어떻게 되죠? Python 코드 한 덩어리 던져주고 "이거 돌려보세요" 합니다. 그 뒤로는 다 사용자 몫이에요 — RPC 셋업, 키 관리, 가스, 시뮬레이션, 영수증 확인, 에러 처리. AI는 *설계자*고, 사용자는 *운영자*. 자동화 하나 굴리려고 셋업하는 데만 반나절씩.

이 런타임이 그 *운영자 자리*를 컴퓨터한테 넘깁니다. AI는 *뭘 시키고 싶은지*만 짧은 자바스크립트로 적어 넘기고, 런타임이 나머지를 다 처리해요:

- 체인 연결, 한 단계씩 시뮬레이션
- 로컬에서 서명 후 브로드캐스트
- 영수증 받고, 모든 결정을 로컬 DB에 기록
- 온체인 이벤트 구독해서 즉시 반응
- 여러 단계를 한 트랜잭션으로 묶기 (EIP-7702)

이제 AI는 *코드를 제안*하는 게 아니라 본인 컴퓨터 안에서 **코드를 굴립니다.** 대화로요.

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

준비물: Mac/Linux, [Rust](https://rustup.rs/), [Foundry](https://book.getfoundry.sh/), [Claude Code](https://claude.ai/code).

```bash
# 1. 코드 받고 빌드
git clone https://github.com/forrestkim-iotrust/onchain-strategy-mcp.git
cd onchain-strategy-mcp
cargo build --release --bin executor-mcp

# 2. 에이전트가 굴릴 새 지갑 생성 (소액만 넣기)
cast wallet new
export EXECUTOR_PRIVATE_KEY=0x여기에키

# 3. 운영자 설정
cp -R .local.example .local
$EDITOR .local/config.toml         # RPC와 signer env 변수명
$EDITOR .local/policy.toml         # 에이전트가 만질 수 있는 범위

# 4. Claude Code에 붙이기
claude mcp add osmcp \
  -e EXECUTOR_CONFIG=$PWD/.local/config.toml \
  -e EXECUTOR_PRIVATE_KEY=$EXECUTOR_PRIVATE_KEY \
  -- $PWD/target/release/executor-mcp
```

그 다음 Claude Code 안에서:

> `examples/strategies/yield-snapshot.js` 등록해서 한 번 돌리고 결과 보여줘.

Claude가 MCP 도구로 strategy 등록·실행하고, journal에 남은 결과가 채팅에 그대로 나옵니다. 여기서부터는 strategy 더 짜고, 트리거 붙이고, 흐름을 말로 다 조립하면 됩니다.

---

## 4. 실제로 굴려본 시나리오들

### A. 자동 입금 깔때기

한 번 셋업해두면, 지갑에 들어오는 모든 ETH나 USDC가 자동으로 USDC가 되어 Aave에 들어갑니다. 가스용 ETH 약간만 남겨두고. 그 뒤로는 손 댈 일 없음 — 도착하는 모든 자금이 이자를 받기 시작해요. 신경 쓸 건 지갑 주소로 돈 보내는 거 하나뿐.

### B. 수익률 비교기

Claude한테: *"지난 30일 Aave / Compound / Moonwell USDC 이자율 시간 단위로 비교해줘."* Claude가 짧은 view 함수를 짜서 archive RPC로 과거 블록을 훑어 표로 즉시 줍니다. 기다림 없음 — 데이터는 이미 온체인에 있어요.

이어서: *"이제부터 계속 보다가 Moonwell이 5% 넘으면 알려줘."* 별도 명령 — Claude가 주기적 체크나 이벤트 트리거를 붙입니다.

같은 런타임의 두 모드: **과거는 호출 한 번, 실시간 감시는 트리거.**

### C. 즉각 반응자

Claude한테: *"Aave 오라클이 ETH 가격 업데이트하는 걸 봐. 한 번에 2% 이상 떨어지면 내 borrow 갚아."* Claude가 로그 이벤트 트리거를 등록해두면, 그 정확한 이벤트가 발화하는 순간 strategy가 초 단위로 돕니다.

### D. 원자적 멀티스텝

같이 일어나거나 둘 다 안 일어나야 하는 행동들이 있습니다 (approve 후 사용 같은). 보통은 두 트랜잭션이고 사이에 위험 구간이 생기죠. EIP-7702 (이더리움, 2025) 덕분에 이 런타임은 둘을 한 트랜잭션에 묶습니다. 둘 다 들어가거나 둘 다 안 들어가거나.

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

지금 되는 건 위 §2에 있고, 앞으로 만들 것:

- **외부 오라클 트리거 + 데이터 소싱.** Chainlink / Pyth / Redstone 가격 업데이트, 오프체인 데이터 피드, 임의 HTTPS 웹훅으로 strategy 발화. 에이전트가 온체인 상태뿐 아니라 *현실 세계* 신호에도 반응할 수 있게.
- **WaaS (Wallet-as-a-Service) 통합.** Privy, Turnkey, Coinbase MPC 같은 에이전트 전용 지갑 솔루션을 1급으로 연결. 권한 분리(세션 키, 계정별 정책, 키 로테이션, 복구)를 지갑 레이어가 책임지게 — burner + 로컬 정책은 1인 운영자용, WaaS는 팀·프로덕션·멀티테넌트용.
- **AMM 아닌 거래소 통합.** Hyperliquid (perps), Polymarket (예측시장) 같은 오더북·전문 거래소 1급 지원. 에이전트가 주문, 포지션 관리, 시장 결제까지.
- **non-EVM 체인.** Solana 먼저, 다른 생태계 (Move, CosmWasm, Stellar Soroban)를 위한 추상화. 같은 strategy / policy / journal 모델, signer와 RPC 백엔드만 다르게.

날짜보다 방향이 중요한 과제들이에요. [issue](https://github.com/forrestkim-iotrust/onchain-strategy-mcp/issues)로 토론·PR 환영합니다.

---

## License & credits

Apache 2.0. [rmcp](https://github.com/modelcontextprotocol/rust-sdk), [alloy](https://github.com/alloy-rs/alloy), [rquickjs](https://github.com/DelSkayn/rquickjs), [foundry](https://github.com/foundry-rs/foundry) 위에 만들었어요. 로컬 우선 — 운영하는 서버 없고, 만들 계정도 없습니다.

아키텍처 자세한 내용 (크레이트 구조, 트리거 파이프라인, EIP-7702 사양)은 `crates/` 소스랑 `examples/contracts/BatchExec.sol` 보세요.
