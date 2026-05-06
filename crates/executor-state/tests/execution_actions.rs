mod common;

use common::{fresh_memory_store, seed_strategies};
use executor_core::schema::execution::RunStatus;

fn seed_run(store: &mut executor_state::StateStore) -> String {
    let strategy_id = seed_strategies(store, 1).remove(0);
    store
        .insert_run(&strategy_id, RunStatus::Running)
        .expect("insert run")
}

#[test]
fn execution_actions_roundtrip() {
    let mut store = fresh_memory_store();
    let run_id = seed_run(&mut store);

    store
        .record_execution_broadcast(
            &run_id,
            0,
            "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            "0x1111111111111111111111111111111111111111111111111111111111111111",
        )
        .expect("record broadcast");
    store
        .record_execution_receipt_success(&run_id, 0, "success", "21000")
        .expect("record receipt");

    let rows = store
        .list_executions_for_run(&run_id)
        .expect("list executions");
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.run_id, run_id);
    assert_eq!(row.action_index, 0);
    assert_eq!(
        row.signer_address.as_deref(),
        Some("0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266")
    );
    assert_eq!(
        row.tx_hash.as_deref(),
        Some("0x1111111111111111111111111111111111111111111111111111111111111111")
    );
    assert_eq!(row.status, "confirmed");
    assert_eq!(row.receipt_status.as_deref(), Some("success"));
    assert_eq!(row.gas_used.as_deref(), Some("21000"));
    assert_eq!(row.error_kind, None);
    assert_eq!(row.error_detail, None);
    assert!(!row.id.is_empty());
    assert!(!row.recorded_at.is_empty());
    assert!(!row.updated_at.is_empty());
}

#[test]
fn execution_actions_order_by_action_index() {
    let mut store = fresh_memory_store();
    let run_id = seed_run(&mut store);

    store
        .record_execution_broadcast(&run_id, 2, "0xsigner", "0x02")
        .expect("record action 2");
    store
        .record_execution_broadcast(&run_id, 0, "0xsigner", "0x00")
        .expect("record action 0");
    store
        .record_execution_broadcast(&run_id, 1, "0xsigner", "0x01")
        .expect("record action 1");

    let indices: Vec<i64> = store
        .list_executions_for_run(&run_id)
        .expect("list executions")
        .into_iter()
        .map(|row| row.action_index)
        .collect();

    assert_eq!(indices, vec![0, 1, 2]);
}

#[test]
fn execution_actions_unique_run_action_index() {
    let mut store = fresh_memory_store();
    let run_id = seed_run(&mut store);

    store
        .record_execution_broadcast(&run_id, 0, "0xsigner-a", "0xaaa")
        .expect("record first broadcast");
    store
        .record_execution_broadcast(&run_id, 0, "0xsigner-b", "0xbbb")
        .expect("upsert duplicate action index");

    let rows = store
        .list_executions_for_run(&run_id)
        .expect("list executions");

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].action_index, 0);
    assert_eq!(rows[0].signer_address.as_deref(), Some("0xsigner-b"));
    assert_eq!(rows[0].tx_hash.as_deref(), Some("0xbbb"));
    assert_eq!(rows[0].status, "broadcasted");
}

#[test]
fn execution_error_allows_missing_signer_address() {
    let mut store = fresh_memory_store();
    let run_id = seed_run(&mut store);

    store
        .record_execution_error(
            &run_id,
            0,
            None,
            "signer_not_configured",
            Some("missing signer"),
        )
        .expect("record execution error");

    let rows = store
        .list_executions_for_run(&run_id)
        .expect("list executions");

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].action_index, 0);
    assert_eq!(rows[0].signer_address, None);
    assert_eq!(rows[0].status, "failed");
    assert_eq!(rows[0].error_kind.as_deref(), Some("signer_not_configured"));
}
