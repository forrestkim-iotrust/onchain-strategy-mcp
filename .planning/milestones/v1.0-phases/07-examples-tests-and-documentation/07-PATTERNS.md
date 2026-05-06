# Phase 7: Examples, Tests, and Documentation - Pattern Map

**Mapped:** 2026-04-29
**Files analyzed:** 12
**Analogs found:** 12 / 12

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `examples/README.md` | documentation | request-response | `README.md` | role-match |
| `examples/local-erc20-transfer-or-approve/strategy.js` | example fixture | transform | `crates/strategy-js/tests/ctx_actions_builders.rs` | exact |
| `examples/local-erc20-transfer-or-approve/policy.toml` | config fixture | request-response | `crates/executor-policy/tests/fixtures/policy.permissive.toml` | exact |
| `examples/local-erc20-transfer-or-approve/config.example.toml` | config fixture | request-response | `config.example.toml` + stdio config helpers | role-match |
| `examples/generic-contract-call/strategy.js` | example fixture | transform | `crates/strategy-js/tests/ctx_actions_builders.rs` | exact |
| `examples/generic-contract-call/abi.json` | fixture | transform | inline ABI fixtures in `ctx_actions_builders.rs` and `stdio_handshake.rs` | exact |
| `examples/generic-contract-call/policy.toml` | config fixture | request-response | `crates/executor-policy/tests/fixtures/policy.permissive.toml` | exact |
| `examples/generic-contract-call/config.example.toml` | config fixture | request-response | `config.example.toml` + stdio config helpers | role-match |
| `crates/executor-mcp/tests/stdio_handshake.rs` | test | request-response | same file existing stdio/anvil sections | exact |
| `crates/strategy-js/tests/sandbox_host_globals.rs` | test | transform | same file existing forbidden-host suite | exact |
| `README.md` | documentation | request-response | existing `README.md` structure + `AGENTS.md` constraints | exact |
| `AGENTS.md` | documentation | request-response | existing `AGENTS.md` architecture/boundaries sections | exact |

## Pattern Assignments

### `examples/README.md` (documentation, request-response)

**Analog:** `README.md`

**Docs scope/non-goal pattern** (`README.md` lines 4-24):
```markdown
## What It Is

`onchain-strategy-mcp` is a strategy runtime, not a product surface.

It exists to let an agent:

- create a strategy
- register it against an account boundary
- run it on a schedule or event loop
- produce auditable action graphs
- execute those actions safely or externalize them
- inspect state, logs, reports, and receipts later

This repository is not:

- a trading app
- a wallet UI
- a dashboard
- a cloud deployment system
- a strategy marketplace
```

**Apply:** Keep examples index local-runtime focused; do not introduce hosted dashboard/product language.

**Runtime lifecycle pattern** (`README.md` lines 159-179):
```markdown
## Execution Lifecycle

Every action should move through a consistent pipeline:

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

The runtime should prefer structured reports over prose-only logs.
```

**Apply:** Example README should explain commands in this order: register strategy, run strategy, policy/simulation/signing, `execution_get`, `journal://{run_id}`.

---

### `examples/local-erc20-transfer-or-approve/strategy.js` (example fixture, transform)

**Analog:** `crates/strategy-js/tests/ctx_actions_builders.rs`

**Builder import/test harness pattern** (lines 10-21):
```rust
use executor_core::schema::action::Action;
use serde_json::{Value, json};
use strategy_js::{CtxStub, RuntimeError, Sandbox};

fn run(source: &str) -> Result<Value, RuntimeError> {
    let mut host = CtxStub {
        strategy_id: "0".repeat(64),
        run_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        ..CtxStub::default()
    };
    Sandbox::execute(source, &mut host)
}
```

**ERC20 transfer strategy shape** (lines 208-224):
```rust
#[test]
fn erc20_transfer_builder_returns_valid_json() {
    let src = format!(
        r#"(ctx) => [ctx.actions.erc20Transfer({{
            token: "{ADDR1}",
            to:    "{ADDR2}",
            amount: "1000"
        }})]"#
    );
    let r = run(&src).expect("must succeed");
    let arr = r.as_array().expect("array");
    assert_eq!(arr[0]["kind"].as_str(), Some("erc20_transfer"));
    assert_eq!(arr[0]["amount"].as_str(), Some("1000"));
    let action: Action =
        serde_json::from_value(arr[0].clone()).expect("Action deserialize");
    assert!(matches!(action, Action::Erc20Transfer(_)));
}
```

**ERC20 approve strategy shape** (lines 277-293):
```rust
#[test]
fn erc20_approve_builder_returns_valid_json() {
    let src = format!(
        r#"(ctx) => [ctx.actions.erc20Approve({{
            token:   "{ADDR1}",
            spender: "{ADDR3}",
            amount:  "0"
        }})]"#
    );
    let r = run(&src).expect("must succeed");
    let arr = r.as_array().expect("array");
    assert_eq!(arr[0]["kind"].as_str(), Some("erc20_approve"));
    assert_eq!(arr[0]["spender"].as_str(), Some(ADDR3));
    let action: Action =
        serde_json::from_value(arr[0].clone()).expect("Action deserialize");
    assert!(matches!(action, Action::Erc20Approve(_)));
}
```

**Apply:** Example JS should export or contain a plain `(ctx) => [...]` strategy using `ctx.actions.erc20Transfer` or `ctx.actions.erc20Approve`; use decimal strings for token amounts.

---

### `examples/local-erc20-transfer-or-approve/policy.toml` (config fixture, request-response)

**Analog:** `crates/executor-policy/tests/fixtures/policy.permissive.toml`

**Policy TOML pattern** (lines 0-25):
```toml
[chains]
allow = [31337]

[contracts.31337]
allow = [
    "0x5fbdb2315678afecb367f032d93f642f64180aa3",
    "0xe7f1725e7734ce288f8367e1bb143e90bb3f0512",
]

[selectors."31337:0x5fbdb2315678afecb367f032d93f642f64180aa3"]
allow = ["any"]

[selectors."31337:0xe7f1725e7734ce288f8367e1bb143e90bb3f0512"]
allow = ["0xa9059cbb", "0x095ea7b3", "0x70a08231"]

[native_value.31337]
max_per_action = "1000000000000000000000"

[erc20_spend."31337:0xe7f1725e7734ce288f8367e1bb143e90bb3f0512"]
max_per_run = "1000000000000000000000000"

[raw_call]
allow_global = false
allow = [
    { chain = 31337, contract = "0x5fbdb2315678afecb367f032d93f642f64180aa3", selector = "any" },
]
```

**Apply:** Use chain `31337`, include deployed token contract in `[contracts.31337]`, allow ERC20 transfer selector `0xa9059cbb` and/or approve selector `0x095ea7b3`, and set explicit ERC20 spend cap.

---

### `examples/local-erc20-transfer-or-approve/config.example.toml` (config fixture, request-response)

**Analog:** `config.example.toml`; signer shape from `crates/executor-mcp/tests/stdio_handshake.rs`

**Base config pattern** (`config.example.toml` lines 5-27):
```toml
[logging]
level = "info"   # trace | debug | info | warn | error

[state]
# Path to the SQLite database file. Use ":memory:" for ephemeral testing,
# or an absolute path for fixed-location deployments. Relative paths are
# resolved against the process current working directory (Phase 2 / D-03a).
path = "./state.db"

[evm]
# Phase 4 D-04: EVM RPC endpoint and per-call timeout.
rpc_url = "http://127.0.0.1:8545"
call_timeout_ms = 1000             # 50..=30_000

# Phase 5 D-14: `from` address used by the simulation adapter
simulation_from = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
```

**Signer config pattern** (`crates/executor-mcp/tests/stdio_handshake.rs` lines 1265-1288):
```rust
spawn_server_with_config_text_and_env(
    &format!(
        r#"[state]
path = "{}"

[policy]
path = "{}"

[evm]
rpc_url = "{}"
call_timeout_ms = 1000

[signer]
private_key_env = "{}"
receipt_timeout_ms = 120000
"#,
        db_path.display(),
        policy_path.display(),
        rpc_url,
        private_key_env,
    ),
    &[(private_key_env, private_key)],
)
```

**Apply:** Example config should include `[policy].path`, `[evm].rpc_url`, `[signer].private_key_env`, and must reference env vars, not committed private keys.

---

### `examples/generic-contract-call/strategy.js` (example fixture, transform)

**Analog:** `crates/strategy-js/tests/ctx_actions_builders.rs`

**Generic contract call builder pattern** (lines 27-58):
```rust
const TRANSFER_ABI: &str = r#"[
    {"type":"function","name":"transfer","inputs":[
        {"name":"to","type":"address"},
        {"name":"amount","type":"uint256"}
    ],"outputs":[{"name":"","type":"bool"}],"stateMutability":"nonpayable"}
]"#;

#[test]
fn contract_call_builder_returns_valid_json() {
    let src = format!(
        r#"(ctx) => [ctx.actions.contractCall({{
            address: "{ADDR1}",
            abi: {abi},
            function: "transfer",
            args: ["{ADDR2}", "1000"]
        }})]"#,
        abi = serde_json::to_string(TRANSFER_ABI).unwrap()
    );
    let r = run(&src).expect("must succeed");
    let arr = r.as_array().expect("array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["kind"].as_str(), Some("contract_call"));
    assert_eq!(arr[0]["address"].as_str(), Some(ADDR1));
    assert_eq!(arr[0]["function"].as_str(), Some("transfer"));
    assert_eq!(arr[0]["value"].as_str(), Some("0"));
}
```

**ABI array form pattern** (lines 60-82):
```rust
#[test]
fn contract_call_builder_accepts_abi_array_form() {
    // abi as JS array of fragments (no JSON.stringify on the JS side).
    let src = format!(
        r#"(ctx) => [ctx.actions.contractCall({{
            address: "{ADDR1}",
            abi: [{{
                type: "function",
                name: "f",
                inputs: [],
                outputs: [],
                stateMutability: "nonpayable"
            }}],
            function: "f",
            args: []
        }})]"#
    );
    let r = run(&src).expect("must succeed");
    let arr = r.as_array().expect("array");
    assert_eq!(arr[0]["kind"].as_str(), Some("contract_call"));
    // abi field carries the serialized JSON string
    assert!(arr[0]["abi"].as_str().expect("abi string").contains("\"f\""));
}
```

**Apply:** Generic example should use `ctx.actions.contractCall({ address, abi, function, args })`; large integers must be decimal strings.

---

### `examples/generic-contract-call/abi.json` (fixture, transform)

**Analog:** ABI fragments in `crates/strategy-js/tests/ctx_actions_builders.rs` and `crates/executor-mcp/tests/stdio_handshake.rs`

**Minimal ABI pattern** (`ctx_actions_builders.rs` lines 27-32):
```rust
const TRANSFER_ABI: &str = r#"[
    {"type":"function","name":"transfer","inputs":[
        {"name":"to","type":"address"},
        {"name":"amount","type":"uint256"}
    ],"outputs":[{"name":"","type":"bool"}],"stateMutability":"nonpayable"}
]"#;
```

**Selector-denial ABI pattern** (`stdio_handshake.rs` lines 3008-3019):
```rust
let abi = r#"[{"type":"function","name":"transfer","inputs":[{"name":"to","type":"address"},{"name":"amount","type":"uint256"}],"outputs":[{"name":"","type":"bool"}],"stateMutability":"nonpayable"}]"#;
let source = format!(
    r#"(ctx) => [{
        kind: "contract_call",
        address: "{target}",
        abi: {},
        function: "transfer",
        args: ["0x0000000000000000000000000000000000000003", "1"],
        value: "0"
    }]"#,
    serde_json::to_string(abi)?
);
```

**Apply:** Keep checked-in ABI JSON minimal and aligned with the deployed fixture function(s); do not hand-compute calldata.

---

### `examples/generic-contract-call/policy.toml` (config fixture, request-response)

**Analog:** `crates/executor-mcp/tests/stdio_handshake.rs` policy generation helpers

**Permissive policy generation pattern** (lines 1191-1233):
```rust
#[cfg(feature = "anvil-tests")]
fn write_permissive_policy(contracts: &[&str]) -> Result<tempfile::NamedTempFile> {
    let policy = tempfile::NamedTempFile::new()?;
    let contracts_toml = contracts
        .iter()
        .map(|addr| format!("    \"{addr}\","))
        .collect::<Vec<_>>()
        .join("\n");
    let selector_entries = contracts
        .iter()
        .map(|addr| format!("[selectors.\"31337:{addr}\"]\nallow = [\"any\"]\n"))
        .collect::<Vec<_>>()
        .join("\n");
    let raw_allow = contracts
        .iter()
        .map(|addr| format!("    {{ chain = 31337, contract = \"{addr}\", selector = \"any\" }},"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(
        policy.path(),
        format!(
            r#"[chains]
allow = [31337]

[contracts.31337]
allow = [
{contracts_toml}
]

{selector_entries}
[native_value.31337]
max_per_action = "1000000000000000000000000"

[raw_call]
allow_global = false
allow = [
{raw_allow}
]
"#
        ),
    )?;
    Ok(policy)
}
```

**Apply:** For the generic ABI example, allow the deployed fixture contract and either the exact selector(s) or `any` for the local-only example contract.

---

### `examples/generic-contract-call/config.example.toml` (config fixture, request-response)

**Analog:** `crates/executor-mcp/tests/stdio_handshake.rs`

**MCP config with policy/RPC/signer pattern** (lines 1258-1289):
```rust
async fn spawn_server_with_policy_rpc_and_signer(
    db_path: &std::path::Path,
    policy_path: &std::path::Path,
    rpc_url: &str,
    private_key_env: &str,
    private_key: &str,
) -> Result<common::ServerProc> {
    spawn_server_with_config_text_and_env(
        &format!(
            r#"[state]
path = "{}"

[policy]
path = "{}"

[evm]
rpc_url = "{}"
call_timeout_ms = 1000

[signer]
private_key_env = "{}"
receipt_timeout_ms = 120000
"#,
            db_path.display(),
            policy_path.display(),
            rpc_url,
            private_key_env,
        ),
        &[(private_key_env, private_key)],
    )
    .await
}
```

**Apply:** Mirror the ERC20 example config shape; keep signer private key outside TOML via env var.

---

### `crates/executor-mcp/tests/stdio_handshake.rs` (test, request-response)

**Analog:** same file existing stdio/anvil safety sections and `crates/executor-mcp/tests/common/mod.rs`

**Imports and feature-gated Anvil imports** (lines 16-29):
```rust
mod common;

use anyhow::Result;
#[cfg(feature = "anvil-tests")]
use alloy::network::TransactionBuilder;
#[cfg(feature = "anvil-tests")]
use alloy::providers::Provider;
#[cfg(feature = "anvil-tests")]
use alloy::rpc::types::TransactionRequest;
#[cfg(feature = "anvil-tests")]
use alloy_primitives::Address;
use serde_json::{Value, json};

use common::{initialize, recv, send, spawn_server};
```

**Stdio harness pattern** (`crates/executor-mcp/tests/common/mod.rs` lines 130-156):
```rust
pub async fn call_tool(
    proc: &mut ServerProc,
    id: u64,
    tool: &str,
    args: Value,
) -> Result<Value> {
    send(
        proc,
        json!({
            "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": { "name": tool, "arguments": args }
        }),
    )
    .await?;
    recv(proc).await
}

pub fn extract_json_result(r: &Value) -> Value {
    let text = r["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("tools/call result missing content[0].text: {r}"));
    serde_json::from_str(text)
        .unwrap_or_else(|e| panic!("content text is not JSON: {e} — text={text}"))
}
```

**Server spawn/config/env pattern** (`crates/executor-mcp/tests/common/mod.rs` lines 87-128):
```rust
pub async fn spawn_server_with_config_text_and_env(
    config_text: &str,
    envs: &[(&str, &str)],
) -> Result<ServerProc> {
    let bin = env!("CARGO_BIN_EXE_executor-mcp");
    let tmp = tempfile::NamedTempFile::new()?;
    let config_path = tmp.path().to_path_buf();
    std::fs::write(&config_path, config_text)?;
    let _ = tmp.into_temp_path().keep()?;

    let mut command = Command::new(bin);
    command
        .env("RUST_LOG", "error")
        .env("EXECUTOR_CONFIG", config_path.as_os_str())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for (key, value) in envs {
        command.env(key, value);
    }
    let mut child = command.spawn()?;
```

**Policy helper pattern** (lines 2704-2766):
```rust
fn policy_with_contracts(
    chains_allow: &[u64],
    contracts: &[&str],
    selector_entries: &[(&str, &[&str])],
    raw_entries: &[(&str, &str)],
    native_cap: &str,
    erc20_caps: &[(&str, &str)],
) -> String {
    let chains = chains_allow
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let contracts_toml = contracts
        .iter()
        .map(|addr| format!("    \"{addr}\","))
        .collect::<Vec<_>>()
        .join("\n");
    // ... selectors/raw/erc20 entries ...
    format!(
        r#"[chains]
allow = [{chains}]

[contracts.31337]
allow = [
{contracts_toml}
]

{selectors_toml}
[native_value.31337]
max_per_action = "{native_cap}"

{erc20_toml}[raw_call]
allow_global = false
allow = [
{raw_toml}
]
"#
    )
}
```

**Policy denial assertion pattern** (lines 2624-2629):
```rust
fn assert_policy_violation(err: &Value, rule: &str) {
    assert_eq!(err["code"].as_i64(), Some(-32017));
    assert_eq!(err["data"]["code"].as_str(), Some("strategy_runtime_error"));
    assert_eq!(err["data"]["kind"].as_str(), Some("policy_violation"));
    assert_eq!(err["data"]["rule"].as_str(), Some(rule));
}
```

**Journal resource and decision assertion pattern** (lines 2666-2702):
```rust
async fn read_journal_resource(proc: &mut common::ServerProc, id: u64, run_id: &str) -> Result<Value> {
    send(
        proc,
        json!({
            "jsonrpc": "2.0", "id": id, "method": "resources/read",
            "params": { "uri": format!("journal://{run_id}") }
        }),
    )
    .await?;
    let resp = recv(proc).await?;
    let text = resp["result"]["contents"][0]["text"]
        .as_str()
        .expect("journal contents text");
    Ok(serde_json::from_str(text)?)
}

fn assert_decision_row(
    journal: &Value,
    action_index: i64,
    gate: &str,
    verdict: &str,
    rule: Option<&str>,
) {
    let rows = journal["decisions"].as_array().expect("decisions array");
    assert!(
        rows.iter().any(|row| {
            row["action_index"].as_i64() == Some(action_index)
                && row["gate"].as_str() == Some(gate)
                && row["verdict"].as_str() == Some(verdict)
                && match rule {
                    Some(expected) => row["rule"].as_str() == Some(expected),
                    None => row["rule"].is_null(),
                }
        }),
        "missing decision row action_index={action_index} gate={gate} verdict={verdict} rule={rule:?}; rows={rows:?}"
    );
}
```

**Simulation failure pattern** (lines 2768-2822):
```rust
#[cfg(feature = "anvil-tests")]
#[tokio::test(flavor = "multi_thread")]
async fn strategy_run_returns_simulation_failed_when_revert() -> Result<()> {
    let Some(fixture) = alloy::node_bindings::Anvil::new()
        .chain_id(31337)
        .try_spawn()
        .ok()
    else {
        return Ok(());
    };
    let funded_accounts = fixture.addresses().to_vec();
    if funded_accounts.is_empty() {
        return Ok(());
    }
    // deploy revert fixture, run strategy_run, assert simulation_failure
    let err = r.get("error").expect("error envelope present");
    assert_eq!(err["code"].as_i64(), Some(-32017));
    assert_eq!(err["data"]["code"].as_str(), Some("strategy_runtime_error"));
    assert_eq!(err["data"]["kind"].as_str(), Some("simulation_failure"));
    assert_eq!(err["data"]["action_index"].as_i64(), Some(0));
    assert_eq!(err["data"]["fail_reason"].as_str(), Some("revert"));
}
```

**Execution status surface pattern** (lines 886-949):
```rust
#[tokio::test]
async fn execution_status_surfaces_match() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();

    let (strategy_id, run_id) = {
        let mut store = executor_state::StateStore::open(&db_path)?;
        let outcome = store.register_strategy("exec_status", "(ctx) => []", None, None)?;
        let sid = match outcome {
            executor_state::RegisterOutcome::Created(s)
            | executor_state::RegisterOutcome::AlreadyExists(s) => s.id,
        };
        let rid = store.insert_run(&sid, executor_core::schema::execution::RunStatus::Running)?;
        store.record_execution_broadcast(
            &rid,
            1,
            "0x1111111111111111111111111111111111111111",
            "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )?;
        store.record_execution_receipt_success(&rid, 1, "success", "21000")?;
        (sid, rid)
    };
    // assert tool execution_get and execution:// resource match
}
```

**Apply:** Add Phase 7 example smoke tests here if they drive public MCP runtime. Reuse existing helpers; do not create a parallel process/JSON-RPC harness.

---

### `crates/strategy-js/tests/sandbox_host_globals.rs` (test, transform)

**Analog:** same file

**Imports and sandbox harness pattern** (lines 11-21):
```rust
use serde_json::json;
use strategy_js::{CtxStub, RuntimeError, Sandbox};

fn run(source: &str) -> Result<serde_json::Value, RuntimeError> {
    let mut host = CtxStub {
        strategy_id: "0".repeat(64),
        run_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        ..CtxStub::default()
    };
    Sandbox::execute(source, &mut host)
}
```

**Forbidden globals pattern** (lines 23-45):
```rust
#[test]
fn sandbox_blocks_host_globals() {
    // Names verified in 03-CONTEXT.md D-11.
    let source = r#"
        (ctx) => {
            const names = [
                "console", "fetch",
                "setTimeout", "setInterval", "setImmediate", "queueMicrotask",
                "XMLHttpRequest", "WebSocket",
                "process", "Worker",
                "child_process", "fs",
            ];
            for (const n of names) {
                if (typeof globalThis[n] !== "undefined") {
                    return "FOUND: " + n;
                }
            }
            return "noop";
        }
    "#;
    let r = run(source).expect("must succeed");
    assert_eq!(r, json!("noop"), "a forbidden global was reachable: {r:?}");
}
```

**Module/dynamic import denial pattern** (lines 47-92):
```rust
#[test]
fn sandbox_blocks_node_fs_module() {
    let r = run(
        r#"(ctx) => { try { const fs = require("fs"); return "BAD"; } catch(e) { return "noop"; } }"#,
    )
    .expect("must succeed");
    assert_eq!(r, json!("noop"));
}

#[test]
fn sandbox_blocks_dynamic_import() {
    let r = run(r#"(ctx) => import("./foo.so")"#);
    match r {
        Err(RuntimeError::InvalidOutput { detail }) => {
            assert!(
                detail.to_lowercase().contains("promise"),
                "expected promise-reject detail, got: {detail}"
            );
        }
        Err(RuntimeError::Exception(_)) => {}
        Ok(v) => panic!("dynamic import unexpectedly resolved: {v:?}"),
        other => panic!("unexpected error mode: {other:?}"),
    }
}
```

**Apply:** Keep VER-05 canonical coverage in this crate. If adding cases, assert from inside JS and return `"noop"` on safe/blocked behavior.

---

### `README.md` (documentation, request-response)

**Analog:** existing `README.md` plus implementation patterns from tests

**Strategy code boundary pattern** (lines 45-86):
```markdown
### 2. JavaScript Is a Strategy DSL

Strategies may be authored as sandboxed JavaScript functions.

But JavaScript is not the execution authority. A strategy function reads through `ctx`, makes decisions, and returns a structured action graph. The runtime validates and executes that graph.

The important boundary is simple:

- strategy code may propose actions
- strategy code may not directly sign or broadcast transactions
- action graphs must be compiled into normalized actions before policy or execution
```

**Update needed:** Existing README examples use conceptual `ctx.source.*` / `ctx.action.*`. Replace examples with implemented `ctx.evm.*` and `ctx.actions.*` shapes, using patterns from `ctx_actions_builders.rs` lines 39-45, 210-215, and 280-284.

**MCP surface caution pattern** (lines 209-221):
```markdown
## MCP Surface

The public MCP surface should stay narrow and stable.

Current conceptual groups:

- `account.*`
- `strategy.*`
- `execution.*`
- `policy.*`
- `opcode.*`

The runtime should expose durable contracts first, then higher-level recipes later.
```

**Apply:** Document actual v1 tool names (`strategy_register`, `strategy_run`, `execution_get`, journal resource) and avoid adding deferred external signer/dashboard semantics.

---

### `AGENTS.md` (documentation, request-response)

**Analog:** existing `AGENTS.md`

**Runtime flow pattern** (lines 4-7):
```markdown
`onchain-strategy-mcp` is an MCP runtime that lets an AI agent code, run, and manage EVM automation strategies.

v1 uses sandboxed JavaScript over a small `ctx` API. Strategies return `Action[]`; the runtime validates, simulates, policy-checks, signs with a local signer, broadcasts, waits for receipts, and records a journal.
```

**Hard boundaries pattern** (lines 55-64):
```markdown
## Hard Boundaries

- Do not add a dashboard, landing page, marketplace, hosted platform, or protocol recipe catalog in v1.
- Do not add a TypeScript compiler, custom DSL, opcode VM, or workflow DAG in v1.
- Strategy code must not access private keys, filesystem, process APIs, arbitrary network, or direct RPC clients.
- Strategy code returns `Action[]`; it does not sign or broadcast.
- Simulation and policy must run before signing.
- Local signer is v1-only hot-wallet custody; keep signer behind an interface for later external signers.
- Stdio MCP servers must not write logs to stdout. Use stderr/tracing.
```

**Apply:** Refresh AGENTS docs around final local runtime loop and local hot-wallet assumptions; keep these hard boundaries intact.

## Shared Patterns

### Public MCP stdio harness
**Source:** `crates/executor-mcp/tests/common/mod.rs` lines 47-67, 130-156
**Apply to:** Example smoke tests, policy/simulation regression tests
```rust
pub async fn send(proc: &mut ServerProc, msg: Value) -> Result<()> {
    let line = serde_json::to_string(&msg)? + "\n";
    proc.stdin.write_all(line.as_bytes()).await?;
    proc.stdin.flush().await?;
    Ok(())
}

pub async fn recv(proc: &mut ServerProc) -> Result<Value> {
    let mut line = String::new();
    timeout(Duration::from_secs(5), proc.stdout.read_line(&mut line)).await??;
    let v: Value = serde_json::from_str(line.trim_end()).map_err(|e| {
        anyhow::anyhow!("stdout line is not JSON-RPC: {:?} — line={:?}", e, line)
    })?;
    assert_eq!(
        v.get("jsonrpc").and_then(Value::as_str),
        Some("2.0"),
        "message missing jsonrpc: 2.0"
    );
    Ok(v)
}
```

### Anvil skip-cleanly behavior
**Source:** `crates/executor-evm/tests/common/anvil_fixture.rs` lines 21-52
**Apply to:** Live local-chain example tests
```rust
impl AnvilFixture {
    pub fn try_spawn() -> Option<Self> {
        if let Ok(url) = std::env::var("ANVIL_RPC_URL") {
            let rpc_url: Url = url.parse().ok()?;
            return Some(Self {
                instance: None,
                rpc_url,
                funded_accounts: vec![],
            });
        }
        match Anvil::new().chain_id(31337).try_spawn() {
            Ok(instance) => {
                let rpc_url = instance.endpoint_url();
                let funded = instance.addresses().to_vec();
                Some(Self {
                    instance: Some(instance),
                    rpc_url,
                    funded_accounts: funded,
                })
            }
            Err(_e) => {
                eprintln!(
                    "[skip] anvil binary not on PATH; install foundry to run anvil-tests"
                );
                None
            }
        }
    }
}
```

### Fixture bytecode deployment
**Source:** `crates/executor-evm/tests/erc20_helpers_anvil.rs` lines 32-59 and `crates/executor-mcp/tests/stdio_handshake.rs` lines 2592-2622
**Apply to:** ERC20 and generic contract local examples
```rust
const ERC20_BYTECODE: &str = include_str!("fixtures/erc20.hex");

async fn deploy_erc20(
    provider: &Arc<alloy::providers::DynProvider>,
    deployer: Address,
) -> Address {
    let bytecode_hex = ERC20_BYTECODE.trim();
    let stripped = bytecode_hex
        .strip_prefix("0x")
        .or_else(|| bytecode_hex.strip_prefix("0X"))
        .unwrap_or(bytecode_hex);
    let mut bytecode: Vec<u8> = (0..stripped.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&stripped[i..i + 2], 16).expect("hex"))
        .collect();
    let supply: U256 = INITIAL_SUPPLY_DECIMAL.parse().unwrap();
    bytecode.extend_from_slice(&supply.to_be_bytes::<32>());

    let tx = TransactionRequest::default()
        .with_from(deployer)
        .with_deploy_code(bytecode);
    let pending = provider.send_transaction(tx).await.expect("deploy send");
    let receipt = pending.get_receipt().await.expect("deploy receipt");
    receipt.contract_address.expect("deploy receipt has contract_address")
}
```

### Policy/simulation/journal safety evidence
**Source:** `crates/executor-mcp/tests/stdio_handshake.rs` lines 2872-2919
**Apply to:** VER-03 and VER-04 assertions
```rust
async fn assert_policy_denied_journal(name: &str, policy_toml: String, source: String) -> Result<()> {
    ensure_anvil_8545().await?;
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let policy = write_policy(&policy_toml)?;
    let strategy_id = seed_strategy(&db_path, name, &source)?;
    let mut proc = spawn_server_with_policy_and_rpc(&db_path, policy.path(), "http://127.0.0.1:8545").await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let err = r.get("error").expect("error envelope");
    assert_policy_violation(err, "contract_not_allowed");
    let run_id = err["data"]["run_id"].as_str().expect("run_id");
    let journal = read_journal_resource(&mut proc, 3, run_id).await?;
    assert_decision_row(&journal, 0, "policy", "fail", Some("contract_not_allowed"));
    assert_decision_row(&journal, 0, "simulation", "skipped", None);
    proc.child.kill().await?;
    Ok(())
}
```

### Sandbox forbidden-host tests
**Source:** `crates/strategy-js/tests/sandbox_host_globals.rs` lines 23-45, 47-92
**Apply to:** VER-05
```rust
let source = r#"
    (ctx) => {
        const names = [
            "console", "fetch",
            "setTimeout", "setInterval", "setImmediate", "queueMicrotask",
            "XMLHttpRequest", "WebSocket",
            "process", "Worker",
            "child_process", "fs",
        ];
        for (const n of names) {
            if (typeof globalThis[n] !== "undefined") {
                return "FOUND: " + n;
            }
        }
        return "noop";
    }
"#;
let r = run(source).expect("must succeed");
assert_eq!(r, json!("noop"), "a forbidden global was reachable: {r:?}");
```

### Local signer and private key documentation
**Source:** `AGENTS.md` lines 55-64 and `stdio_handshake.rs` lines 2847-2853
**Apply to:** README, AGENTS, config examples
```rust
let mut proc = spawn_server_with_policy_rpc_and_signer(
    &db_path,
    policy.path(),
    "http://127.0.0.1:8545",
    "EXECUTOR_TEST_PRIVATE_KEY",
    "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
)
.await?;
```

**Docs rule:** Show env var names and Anvil dev keys only. Do not imply production custody; local signer is v1-only hot-wallet custody.

## No Analog Found

All likely Phase 7 files have close analogs in the current repository.

| File | Role | Data Flow | Reason |
|------|------|-----------|--------|

## Metadata

**Analog search scope:** root docs, `config.example.toml`, `crates/executor-mcp/tests`, `crates/executor-evm/tests`, `crates/executor-policy/tests/fixtures`, `crates/strategy-js/tests`
**Files scanned:** 16 primary files/fixtures plus repository listing
**Pattern extraction date:** 2026-04-29
