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
use common::{
    call_tool, extract_json_result, initialize, spawn_server_with_config_text_and_env,
    spawn_server_with_state,
};
use serde_json::{Value, json};

#[cfg(feature = "anvil-tests")]
const REVERT_BYTECODE: &str = include_str!("../../executor-evm/tests/fixtures/revert_counter.hex");

const TEST_PRIVATE_KEY_ENV: &str = "EXECUTOR_VERIFICATION_SAFETY_PRIVATE_KEY";
const ANVIL_PRIVATE_KEY: &str = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

fn write_policy(policy_toml: &str) -> Result<tempfile::NamedTempFile> {
    let policy = tempfile::NamedTempFile::new()?;
    std::fs::write(policy.path(), policy_toml)?;
    Ok(policy)
}

async fn spawn_server_with_policy_rpc_and_signer(
    db_path: &std::path::Path,
    policy_path: &std::path::Path,
    rpc_url: &str,
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
            TEST_PRIVATE_KEY_ENV,
        ),
        &[(TEST_PRIVATE_KEY_ENV, ANVIL_PRIVATE_KEY)],
    )
    .await
}

async fn ensure_anvil_8545() -> Result<()> {
    let reachable = tokio::time::timeout(
        std::time::Duration::from_millis(300),
        tokio::net::TcpStream::connect("127.0.0.1:8545"),
    )
    .await
    .is_ok_and(|r| r.is_ok());
    if reachable {
        return Ok(());
    }

    let child = tokio::process::Command::new("anvil")
        .args(["--host", "127.0.0.1", "--port", "8545", "--chain-id", "31337"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    std::mem::forget(child);
    for _ in 0..20 {
        let reachable = tokio::time::timeout(
            std::time::Duration::from_millis(300),
            tokio::net::TcpStream::connect("127.0.0.1:8545"),
        )
        .await
        .is_ok_and(|r| r.is_ok());
        if reachable {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    anyhow::bail!("anvil did not start on 127.0.0.1:8545")
}

fn policy_with_contracts(
    chains_allow: &[u64],
    contracts: &[&str],
    selector_entries: &[(&str, &[&str])],
    raw_entries: &[(&str, &str)],
    native_cap: &str,
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
    let selectors_toml = selector_entries
        .iter()
        .map(|(addr, selectors)| {
            let values = selectors
                .iter()
                .map(|sel| format!("\"{sel}\""))
                .collect::<Vec<_>>()
                .join(", ");
            format!("[selectors.\"31337:{addr}\"]\nallow = [{values}]\n")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let raw_toml = raw_entries
        .iter()
        .map(|(addr, selector)| {
            format!("    {{ chain = 31337, contract = \"{addr}\", selector = \"{selector}\" }},")
        })
        .collect::<Vec<_>>()
        .join("\n");
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

[raw_call]
allow_global = false
allow = [
{raw_toml}
]
"#
    )
}

fn assert_policy_violation(err: &Value, rule: &str) -> String {
    assert_eq!(err["code"].as_i64(), Some(-32017));
    assert_eq!(err["data"]["code"].as_str(), Some("strategy_runtime_error"));
    assert_eq!(err["data"]["kind"].as_str(), Some("policy_violation"), "unexpected error: {err}");
    assert_eq!(err["data"]["rule"].as_str(), Some(rule));
    err["data"]["run_id"].as_str().expect("run_id").to_string()
}

fn assert_no_tx_hash(db_path: &std::path::Path, run_id: &str) -> Result<()> {
    let store = executor_state::StateStore::open(db_path)?;
    let executions = store.list_executions_for_run(run_id)?;
    assert!(
        executions.iter().all(|entry| entry.tx_hash.is_none()),
        "unsafe path recorded a tx hash: {executions:?}"
    );
    Ok(())
}

#[tokio::test]
async fn policy_blocks_disallowed_chain_contract_and_selector_before_signing() -> Result<()> {
    ensure_anvil_8545().await?;

    let target = "0x0000000000000000000000000000000000000002";
    let abi = r#"[{"type":"function","name":"transfer","inputs":[{"name":"to","type":"address"},{"name":"amount","type":"uint256"}],"outputs":[{"name":"","type":"bool"}],"stateMutability":"nonpayable"}]"#;
    let abi_lit = serde_json::to_string(abi)?;
    let cases = [
        (
            "policy_chain_not_allowed",
            format!(
                r#"[chains]
allow = [1]

[contracts.1]
allow = ["{target}"]

[native_value.31337]
max_per_action = "1000000000000000000"

[raw_call]
allow_global = false
allow = []
"#
            ),
            format!(
                r#"(ctx) => [{{
                    kind: "native_transfer",
                    to: "{target}",
                    value: "0"
                }}]"#
            ),
            "chain_not_allowed",
        ),
        (
            "policy_contract_not_allowed",
            policy_with_contracts(&[31337], &[], &[], &[], "1000000000000000000"),
            format!(
                r#"(ctx) => [{{
                    kind: "native_transfer",
                    to: "{target}",
                    value: "0"
                }}]"#
            ),
            "contract_not_allowed",
        ),
        (
            "policy_selector_not_allowed",
            policy_with_contracts(&[31337], &[target], &[(target, &["0xaaaaaaaa"])], &[], "0"),
            format!(
                r#"(ctx) => [ctx.actions.contractCall({{
                    address: "{target}",
                    abi: {abi_lit},
                    function: "transfer",
                    args: ["0x0000000000000000000000000000000000000003", "1"],
                    value: "0"
                }})]"#
            ),
            "selector_not_allowed",
        ),
    ];

    for (name, policy_toml, source, rule) in cases {
        let dir = tempfile::tempdir()?;
        let db_path = dir.path().join("state.db");
        let policy = write_policy(&policy_toml)?;
        executor_policy::load_policy_from_path(policy.path())?;
        let strategy_id = seed_strategy(&db_path, name, &source)?;
        let mut proc = spawn_server_with_policy_rpc_and_signer(
            &db_path,
            policy.path(),
            "http://127.0.0.1:8545",
        )
        .await?;
        let _ = initialize(&mut proc).await?;
        let r = call_tool(
            &mut proc,
            2,
            "strategy_run",
            json!({ "strategy_id": strategy_id }),
        )
        .await?;
        if let Some(err) = r.get("error") {
            let run_id = assert_policy_violation(err, rule);
            assert_no_tx_hash(&db_path, &run_id)?;
        } else {
            panic!("expected policy error for {rule}, got success: {}", extract_json_result(&r));
        }
        proc.child.kill().await?;
    }

    Ok(())
}

#[cfg(feature = "anvil-tests")]
async fn deploy_bytecode(provider: &executor_evm::DynProvider, deployer: Address, bytecode_hex: &str) -> Address {
    let stripped = bytecode_hex
        .trim()
        .strip_prefix("0x")
        .or_else(|| bytecode_hex.trim().strip_prefix("0X"))
        .unwrap_or(bytecode_hex.trim());
    let padded;
    let stripped = if stripped.len() % 2 == 0 {
        stripped
    } else {
        padded = format!("0{stripped}");
        padded.as_str()
    };
    let bytecode = hex::decode(stripped).expect("hex bytecode");
    let tx = TransactionRequest::default()
        .with_from(deployer)
        .with_deploy_code(bytecode);
    let pending = provider.send_transaction(tx).await.expect("send deploy tx");
    let receipt = pending.get_receipt().await.expect("deploy receipt");
    receipt
        .contract_address
        .expect("deploy receipt has contract_address")
}

#[cfg(feature = "anvil-tests")]
#[tokio::test(flavor = "multi_thread")]
async fn simulation_failure_prevents_signing_and_records_no_tx_hash() -> Result<()> {
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
    let rpc_url = fixture.endpoint_url();
    let cfg = executor_evm::EvmConfig {
        rpc_url: rpc_url.clone(),
        ..executor_evm::EvmConfig::default()
    };
    let provider = executor_evm::build_provider(&cfg)?;
    let revert_addr = deploy_bytecode(&provider, funded_accounts[0], REVERT_BYTECODE).await;
    let revert_addr_s = revert_addr.to_string();

    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let policy_toml = policy_with_contracts(
        &[31337],
        &[revert_addr_s.as_str()],
        &[],
        &[(revert_addr_s.as_str(), "0x00000000")],
        "0",
    );
    let policy = write_policy(&policy_toml)?;
    let source = format!(
        r#"(ctx) => [{{
            kind: "raw_call",
            address: "{revert_addr_s}",
            data: "0x00000000",
            value: "0"
        }}]"#
    );
    let strategy_id = seed_strategy(&db_path, "verification_sim_revert", &source)?;
    let mut proc = spawn_server_with_policy_rpc_and_signer(&db_path, policy.path(), rpc_url.as_str()).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let err = r.get("error").expect("error envelope");
    assert_eq!(err["code"].as_i64(), Some(-32017));
    assert_eq!(err["data"]["kind"].as_str(), Some("simulation_failure"));
    assert_eq!(err["data"]["fail_reason"].as_str(), Some("revert"));
    let run_id = err["data"]["run_id"].as_str().expect("run_id");
    assert_no_tx_hash(&db_path, run_id)?;
    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn sandbox_blocks_forbidden_host_access_through_strategy_run() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let forbidden_source = r#"
        (ctx) => {
            if (typeof globalThis.process !== "undefined") return "BAD process";
            if (typeof fetch !== "undefined") return "BAD fetch";
            try { require("fs"); return "BAD require"; } catch (e) {}
            return "noop";
        }
    "#;
    let strategy_id = seed_strategy(&db_path, "sandbox_host_access", forbidden_source)?;
    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let body = extract_json_result(&r);
    assert_eq!(body["status"].as_str(), Some("succeeded"));
    assert_eq!(body["outcome"]["kind"].as_str(), Some("noop"));
    proc.child.kill().await?;

    let dynamic_import_id = seed_strategy(
        &db_path,
        "sandbox_dynamic_import",
        r#"(ctx) => import("fs")"#,
    )?;
    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        3,
        "strategy_run",
        json!({ "strategy_id": dynamic_import_id }),
    )
    .await?;
    let err = r.get("error").expect("dynamic import must not load a module");
    assert_eq!(err["data"]["code"].as_str(), Some("strategy_invalid_output"));
    let detail = err["data"]["detail"].as_str().unwrap_or_default().to_lowercase();
    assert!(
        detail.contains("promise") || detail.contains("import"),
        "unexpected dynamic import block detail: {detail}"
    );
    proc.child.kill().await?;
    Ok(())
}

fn seed_strategy(db_path: &std::path::Path, name: &str, source: &str) -> Result<String> {
    let mut store = executor_state::StateStore::open(db_path)?;
    let outcome = store.register_strategy(name, source, None, None)?;
    let id = match outcome {
        executor_state::RegisterOutcome::Created(s)
        | executor_state::RegisterOutcome::AlreadyExists(s) => s.id,
    };
    Ok(id)
}
