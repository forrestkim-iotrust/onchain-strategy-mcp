# Design System — onchain-strategy-mcp

## Product Context

- **What this is:** A bounded execution runtime for agent-authored DeFi strategies. External agents choose strategy, venues, bridges, and routes; the runtime validates explicit plans, enforces burner/session wallet and risk boundaries, executes approved actions, monitors state, and emits receipts/reports.
- **Who it's for:** Individual DeFi hunters who want to run agent-generated strategies without risking a main wallet, and agent builders who need a runtime that can say no to unsafe execution.
- **Space/industry:** DeFi automation, AI agents, local-first crypto execution infrastructure, MCP developer tooling.
- **Project type:** Product intro / marketing site first, with future app/dashboard surfaces for strategy runs, policy decisions, execution reports, and receipts.

## Aesthetic Direction

- **Direction:** Flight recorder for agentic DeFi execution.
- **Decoration level:** Minimal but instrumented. Use execution traces, ledger rules, receipt strips, route lines, policy stamps, and state markers instead of decorative blobs or generic gradients.
- **Mood:** Serious, bounded, operational, and slightly severe. It should feel like a black-box trading cockpit where agent intent becomes constrained onchain execution.
- **Reference sites:** Privy, Turnkey, Safe, and thirdweb define the category baseline with wallet infrastructure, security claims, programmable wallets, and proof sections. This product should deliberately avoid looking like generic wallet infrastructure.

## Brand Thesis

The product is not a wallet SDK, AI copilot, trading bot, or hosted custody surface.

It is the hard boundary between agent-generated DeFi plans and irreversible chain state.

Core message:

> Your agent can find the trade. It cannot take the wallet.

Supporting message:

> Agents decide strategy. Runtime decides whether execution is allowed.

The hero should make the user believe the product has boundaries before they read a single technical detail.

## Typography

- **Display/Hero:** ABC Diatype Condensed or Söhne Schmal — condensed, severe, operational display type for hero statements and section openers.
- **Body:** IBM Plex Sans or Untitled Sans — neutral supporting copy that does not soften the product too much.
- **UI/Labels:** IBM Plex Mono — labels, policy keys, trace states, run metadata, and command-like UI.
- **Data/Tables:** Berkeley Mono or Commit Mono — receipts, balances, timestamps, hashes, policy decisions, and execution rows. Must use tabular numerals.
- **Code:** Berkeley Mono, Commit Mono, or IBM Plex Mono.
- **Loading:** Prefer self-hosted licensed fonts for production. If unavailable, use IBM Plex Sans/Mono and Barlow Condensed for previews only.
- **Scale:**
  - Hero: 72-138px, condensed, uppercase, line-height 0.82-0.9
  - Section title: 44-82px, condensed, uppercase, line-height 0.9
  - Lead: 20-24px, line-height 1.35-1.45
  - Body: 16-18px, line-height 1.45-1.6
  - Mono UI: 11-13px, letter spacing 0.06-0.14em for labels

Do not use Inter, Roboto, Arial, Helvetica, Open Sans, Lato, Montserrat, or Poppins as primary fonts.

## Color

- **Approach:** Restrained dark technical surface with functional signal colors. No purple-gradient AI branding.
- **Background:** `#0B0E0D` — near-black green graphite.
- **Surface:** `#111714` — panel-depth black-green.
- **Surface raised:** `#151D19` — secondary panel.
- **Primary text:** `#E7E1D2` — aged receipt paper.
- **Muted text:** `#B8B09F` — quiet supporting copy.
- **Disabled / metadata:** `#6F786F`.
- **Rules:** `#26312B` primary linework, `#3C493F` emphasized dividers.
- **Primary accent:** `#D7FF5F` — approved/live execution. Use for CTA, active route, and policy-approved states.
- **Error / denied:** `#FF5C39` — policy breach, rejected action, blocked signature.
- **Warning:** `#F2B84B` — simulation warning, slippage, gas risk.
- **Info / receipt:** `#58C7E8` — confirmation, tx receipt, RPC confirmation.
- **White:** `#FFFFFF` only for rare high-contrast anchors.
- **Dark mode:** Default mode. It should feel native, not inverted.
- **Light mode:** Optional preview/support mode using receipt paper as the background and graphite as text, but product marketing should lead dark.

Gradients are not a primary visual language. If used, they must be functional, such as a thin heat strip showing risk escalation from simulation to broadcast.

## Spacing

- **Base unit:** 4px.
- **Density:** Dense but breathable. Information should feel inspectable, not cramped.
- **Scale:**
  - 2xs: 2px
  - xs: 4px
  - sm: 8px
  - md: 16px
  - lg: 24px
  - xl: 32px
  - 2xl: 48px
  - 3xl: 64px
  - 4xl: 96px

Marketing pages should avoid generic SaaS whitespace that makes the product feel fluffy. App/report screens should prioritize scan speed and row-level clarity.

## Layout

- **Approach:** Poster-first for marketing, grid-disciplined for runtime/report UI.
- **Hero:** Full-screen or near-full-screen execution boundary. Left side contains product mark, blunt headline, and CTA. Right or full-width area shows a live-looking execution ledger.
- **Grid:** 12 columns desktop, 6 columns tablet, 1 column mobile.
- **Max content width:** 1440px for landing pages, 1180px for readable documentation sections.
- **Border radius:** Use almost none.
  - sm: 2px
  - md: 4px
  - lg: 6px
  - full: avoid unless required for tiny status dots
- **Linework:** Thin rules and ledger separators are part of the identity. Use borders more than shadows.
- **Cards:** Avoid decorative card grids. If a panel exists, it should look like an execution surface, receipt, policy stack, or trace artifact.

## Motion

- **Approach:** Minimal-functional and mechanical.
- **States:** proposed → validated → simulated → approved/denied → signed → broadcast → receipt → journal sealed.
- **Easing:**
  - enter: ease-out
  - exit: ease-in
  - move: ease-in-out
- **Duration:**
  - micro: 50-100ms
  - short: 150-250ms
  - medium: 250-400ms
  - long: 400-700ms only for section-level trace reveals

Motion should feel like state transitions in a machine, not magic. No floating particles, orbiting chains, animated mascots, glowing brains, or abstract neural meshes.

## Product UI Patterns

### Execution Ledger

Use the execution ledger as the primary visual proof.

Example rows:

```text
strategy.reviewed      valid
route.locked           deBridge -> Jupiter -> Hyperliquid
simulation.passed      max slippage 42bps
policy.approved        max_notional_usd: 500
signed.session_wallet  no main wallet access
broadcast.tx           0x9c41...77ae
receipt.confirmed      gas_used: 118230
journal.sealed         run_7F92A1
```

### Denied Path

Always show at least one denied path in trust-building surfaces.

```text
policy.denied
reason: selector_not_allowed
signer: not_requested
broadcast: skipped
journal: written
agent_plan: preserved
wallet_boundary: intact
```

Safety should be visible as refusal, not claimed as marketing copy.

### Runtime Contract Split

Use a hard visual split:

- **Agent planner:** strategy, venue, bridge, route, timing, entry/exit, unwind.
- **Execution boundary:** validation, simulation, policy, signer boundary, broadcast, receipts, journal.

This makes the product impossible to confuse with wallet infrastructure.

### Receipt Surface

Receipt views should look like printouts or sealed journal artifacts.

Must include:

- run ID
- strategy ID
- policy decision
- simulation result
- signer/session wallet
- tx hash
- gas used
- receipt status
- stable error kind if failed
- journal status

## Copy Rules

Use blunt operational copy.

Good:

- “Your agent can find the trade. It cannot take the wallet.”
- “Agents decide strategy. Runtime decides whether execution is allowed.”
- “Receipts or it didn’t happen.”
- “Policy before signature.”
- “Burner and session wallets are blast-radius controls.”
- “No private keys in prompts. No hidden execution. No unsigned action drift.”

Avoid:

- “AI-powered DeFi automation”
- “Secure wallet infrastructure for the future”
- “Seamless onchain experiences”
- “Unlock the power of autonomous finance”
- “Built for developers, designed for everyone”

The product should sound like it was built by someone who has watched agents hallucinate and transactions fail.

## Safe Choices

These preserve category literacy:

- Dark technical surface for DeFi/devtool familiarity.
- Code, receipts, hashes, and policy rows as proof artifacts.
- Clear execution lifecycle instead of abstract security claims.
- Developer-readable structure for MCP/agent-builder adoption.

## Deliberate Risks

These make the product memorable:

- Treat agents as untrusted planners, not friendly copilots.
- Do not lead with wallet infrastructure, MPC, embedded wallets, or account abstraction.
- Use denied execution paths as trust proof instead of logo walls.
- Use severe, condensed, terminal-adjacent typography rather than soft SaaS fonts.
- Keep the visual system cold and bounded. Some users may find it less friendly; that is acceptable.

## Anti-Slop Rules

Never use:

- Purple or blue-purple gradients as brand center.
- Mascots, robots, or AI orb imagery.
- 3-column feature grids with icon circles.
- Centered everything with uniform spacing.
- Decorative blob backgrounds.
- Stock dashboards tilted in perspective.
- Trust-logo strips as the main proof.
- Vague non-custodial security claims without policy and receipt artifacts.
- Rounded pill overload.
- Generic “AI agent platform” language.

## Decisions Log

| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-04-29 | Initial design system created | Created by `/design-consultation` after product positioning discussion, category review of Privy/Turnkey/Safe/thirdweb patterns, and outside design review from Codex + Claude subagent. |
| 2026-04-29 | Lead with execution boundary, not wallet infrastructure | The product differentiates by containing agent-authored execution, not by managing wallets more conveniently. |
| 2026-04-29 | Use denied paths and receipts as trust proof | Concrete refusal and receipt artifacts communicate safety better than abstract security claims. |
