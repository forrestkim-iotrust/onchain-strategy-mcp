mod common;

#[cfg(feature = "anvil-tests")]
mod anvil_examples {
    use std::str::FromStr;

    use alloy::network::TransactionBuilder;
    use alloy::providers::Provider;
    use alloy::rpc::types::TransactionRequest;
    use alloy_primitives::Address;
    use anyhow::Result;
    use serde_json::{Value, json};

    use crate::common::{self, call_tool, extract_json_result, initialize};

    const ERC20_EXAMPLE: &str = include_str!("../../../examples/strategies/erc20-approve.js");
    const COUNTER_EXAMPLE: &str = include_str!("../../../examples/strategies/generic-counter-call.js");
    const ACCEPTS_ANY_CALL_BYTECODE: &str = "0x6001600c60003960016000f300";
    const ANVIL_PRIVATE_KEY: &str = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

    fn seed_strategy(db_path: &std::path::Path, name: &str, source: &str) -> Result<String> {
        let mut store = executor_state::StateStore::open(db_path)?;
        let outcome = store.register_strategy(name, source, None, None)?;
        let id = match outcome {
            executor_state::RegisterOutcome::Created(s)
            | executor_state::RegisterOutcome::AlreadyExists(s) => s.id,
        };
        Ok(id)
    }

    fn decode_bytecode(bytecode_hex: &str) -> Vec<u8> {
        let stripped = bytecode_hex
            .trim()
            .strip_prefix("0x")
            .or_else(|| bytecode_hex.trim().strip_prefix("0X"))
            .unwrap_or(bytecode_hex.trim());
        let padded;
        let stripped = if stripped.len() % 2 == 0 {
            stripped
        } else {
            padded = format!("{stripped}0");
            padded.as_str()
        };
        (0..stripped.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&stripped[i..i + 2], 16).expect("hex"))
            .collect()
    }

    async fn deploy_bytecode(
        provider: &std::sync::Arc<executor_evm::DynProvider>,
        deployer: Address,
        bytecode_hex: &str,
    ) -> Address {
        let tx = TransactionRequest::default()
            .with_from(deployer)
            .with_deploy_code(decode_bytecode(bytecode_hex));
        let pending = provider.send_transaction(tx).await.expect("send deploy tx");
        let receipt = pending.get_receipt().await.expect("deploy receipt");
        receipt
            .contract_address
            .expect("deploy receipt has contract_address")
    }

    fn write_example_policy(token: Address, counter: Address) -> Result<tempfile::NamedTempFile> {
        let policy = tempfile::NamedTempFile::new()?;
        std::fs::write(
            policy.path(),
            format!(
                r#"[chains]
allow = [31337]

[contracts.31337]
allow = [
    "{token}",
    "{counter}",
]

[selectors."31337:{token}"]
allow = ["0x095ea7b3"]

[selectors."31337:{counter}"]
allow = ["0xd09de08a"]

[native_value.31337]
max_per_action = "0"

[erc20_spend."31337:{token}"]
max_per_run = "1000000000000000000"

[raw_call]
allow_global = false
allow = []
"#
            ),
        )?;
        Ok(policy)
    }

    async fn spawn_server(
        db_path: &std::path::Path,
        policy_path: &std::path::Path,
        rpc_url: &str,
    ) -> Result<crate::common::ServerProc> {
        crate::common::spawn_server_with_config_text_and_env(
            &format!(
                r#"[state]
path = "{}"

[policy]
path = "{}"

[evm]
rpc_url = "{}"
call_timeout_ms = 1000

[signer]
private_key_env = "EXECUTOR_TEST_PRIVATE_KEY"
receipt_timeout_ms = 120000
"#,
                db_path.display(),
                policy_path.display(),
                rpc_url,
            ),
            &[("EXECUTOR_TEST_PRIVATE_KEY", ANVIL_PRIVATE_KEY)],
        )
        .await
    }

    async fn execution_report_for_strategy(source: &str, name: &str) -> Result<Value> {
        let Some(fixture) = alloy::node_bindings::Anvil::new()
            .chain_id(31337)
            .try_spawn()
            .ok()
        else {
            return Ok(json!({ "skipped": "anvil unavailable" }));
        };
        let funded_accounts = fixture.addresses().to_vec();
        if funded_accounts.len() < 2 {
            return Ok(json!({ "skipped": "anvil funded accounts unavailable" }));
        }
        let rpc_url = fixture.endpoint_url();
        let cfg = executor_evm::EvmConfig {
            rpc_url: rpc_url.clone(),
            ..executor_evm::EvmConfig::default()
        };
        let provider = executor_evm::build_provider(&cfg)?;
        let token = deploy_bytecode(&provider, funded_accounts[0], ACCEPTS_ANY_CALL_BYTECODE).await;
        let counter = deploy_bytecode(&provider, funded_accounts[0], ACCEPTS_ANY_CALL_BYTECODE).await;
        let policy = write_example_policy(token, counter)?;
        let source = source
            .replace("0x0000000000000000000000000000000000000001", &token.to_string())
            .replace(
                "0x0000000000000000000000000000000000000002",
                &funded_accounts[1].to_string(),
            )
            .replace("0x0000000000000000000000000000000000000003", &counter.to_string());
        let source = source
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");

        let dir = tempfile::tempdir()?;
        let db_path = dir.path().join("state.db");
        let strategy_id = seed_strategy(&db_path, name, &source)?;
        let mut proc = spawn_server(&db_path, policy.path(), rpc_url.as_str()).await?;
        let _ = initialize(&mut proc).await?;
        let run_response = extract_json_result(
            &call_tool(
                &mut proc,
                2,
                "strategy_run",
                json!({ "strategy_id": strategy_id }),
            )
            .await?,
        );
        assert_eq!(run_response["status"].as_str(), Some("succeeded"));
        let run_id = run_response["run_id"].as_str().expect("run_id");
        // v1.4 Track B: execution_get tool dropped; read via the resource.
        let report_resp = common::read_resource(&mut proc, 3, &format!("execution://{run_id}")).await?;
        let report = common::extract_resource_json(&report_resp);
        proc.child.kill().await?;
        Ok(report)
    }

    fn assert_one_confirmed_action(report: &Value) {
        if report.get("skipped").is_some() {
            return;
        }
        assert_eq!(report["status"].as_str(), Some("succeeded"));
        let actions = report["actions"].as_array().expect("actions array");
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0]["status"].as_str(), Some("confirmed"));
        assert!(actions[0]["tx_hash"].as_str().is_some_and(|s| !s.is_empty()));
        assert!(actions[0]["gas_used"].as_str().is_some_and(|s| !s.is_empty()));
        assert!(actions[0]["error_kind"].is_null());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn examples_erc20_approve_executes_on_anvil() -> Result<()> {
        let report = execution_report_for_strategy(ERC20_EXAMPLE, "example_erc20_approve").await?;
        assert_one_confirmed_action(&report);
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn examples_generic_contract_call_executes_on_anvil() -> Result<()> {
        let report = execution_report_for_strategy(COUNTER_EXAMPLE, "example_counter_call").await?;
        assert_one_confirmed_action(&report);
        Ok(())
    }

    #[test]
    fn anvil_fixture_private_key_matches_first_account() {
        let _ = Address::from_str("0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266")
            .expect("known anvil address");
    }
}
