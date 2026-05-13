use std::sync::Arc;

use executor_evm::{NormalizedAction, NormalizedActionKind};
use executor_mcp::tools::{execute_approved_actions, fail_signer_config_resolution};
use executor_state::{RegisterOutcome, StateStore};
use tokio::sync::Mutex;

fn seed_run(store: &mut StateStore) -> String {
    let strategy_id = match store
        .register_strategy("exec", "(ctx) => []", None, None)
        .expect("register")
    {
        RegisterOutcome::Created(s) | RegisterOutcome::AlreadyExists(s) => s.id,
    };
    store
        .insert_run(
            &strategy_id,
            executor_core::schema::execution::RunStatus::Running,
        )
        .expect("insert run")
}

#[tokio::test]
async fn signer_config_resolution_failure_records_execution_action_error() {
    let mut store = StateStore::open(std::path::Path::new(":memory:")).expect("store");
    let run_id = seed_run(&mut store);
    let state = Arc::new(Mutex::new(store));
    let normalized = vec![Some(NormalizedAction {
        tx: Default::default(),
        source: NormalizedActionKind::NativeTransfer,
        selector: None,
        native_value: alloy_primitives::U256::ZERO,
        erc20_amount: None,
    })];

    let err =
        fail_signer_config_resolution(&state, &run_id, &normalized, "invalid signer configuration")
            .await
            .expect_err("config resolution fails");

    assert_eq!(err.code.0, -32017);
    let data = err.data.as_ref().expect("error data");
    assert_eq!(data["kind"].as_str(), Some("signer_not_configured"));

    let executions = {
        let store = state.lock().await;
        store
            .list_executions_for_run(&run_id)
            .expect("execution rows")
    };
    assert_eq!(executions.len(), 1);
    assert_eq!(executions[0].action_index, 0);
    assert_eq!(executions[0].status, "failed");
    assert_eq!(executions[0].signer_address, None);
    assert_eq!(
        executions[0].error_kind.as_deref(),
        Some("signer_not_configured")
    );
}

#[tokio::test]
async fn execution_actions_signer_not_configured_records_execution_action_error() {
    let mut store = StateStore::open(std::path::Path::new(":memory:")).expect("store");
    let run_id = seed_run(&mut store);
    let state = Arc::new(Mutex::new(store));
    let normalized = vec![Some(NormalizedAction {
        tx: Default::default(),
        source: NormalizedActionKind::NativeTransfer,
        selector: None,
        native_value: alloy_primitives::U256::ZERO,
        erc20_amount: None,
    })];

    let err = execute_approved_actions(
        &state,
        &run_id,
        "http://127.0.0.1:8545",
        None,
        31337,
        &normalized,
        None,
        None,
    )
    .await
    .expect_err("missing signer config fails");

    assert_eq!(err.code.0, -32017);
    let data = err.data.as_ref().expect("error data");
    assert_eq!(data["kind"].as_str(), Some("signer_not_configured"));

    let executions = {
        let store = state.lock().await;
        store
            .list_executions_for_run(&run_id)
            .expect("execution rows")
    };
    assert_eq!(executions.len(), 1);
    assert_eq!(executions[0].action_index, 0);
    assert_eq!(executions[0].status, "failed");
    assert_eq!(executions[0].signer_address, None);
    assert_eq!(
        executions[0].error_kind.as_deref(),
        Some("signer_not_configured")
    );
}
