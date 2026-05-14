//! `StateStore` — single `rusqlite::Connection` owner (D-03d, D-06).
//!
//! `StateStore` is **NOT** internally synchronised. Callers wrap this in
//! `Arc<tokio::sync::Mutex<StateStore>>` and enter a `spawn_blocking` +
//! `blocking_lock()` block before mutating calls (RESEARCH Pattern 2).
//! Holding the outer mutex across an `await` is forbidden (Pitfall 4).
//!
//! The bare `pub fn __test_conn` accessor is `#[doc(hidden)]` and only meant
//! for integration tests that need to exercise raw SQL invariants
//! (`partial_index_behaviour.rs`, `foreign_keys_enforced`). Production code
//! must go through the typed façade methods.

use crate::{
    error::StateError, executions, policy_revisions, records_capture, runs, schema, strategies,
    triggers,
};
use executor_core::schema::trigger::{
    RegisterTriggerInput, Trigger, TriggerEvent, TriggerListFilter, TriggerSummary,
};
use executor_core::schema::execution::RunStatus;
use rusqlite::Connection;
use std::path::Path;

pub struct StateStore {
    pub(crate) conn: Connection,
}

impl StateStore {
    pub fn open(path: &Path) -> Result<Self, StateError> {
        let conn = schema::open_conn(path)?;
        Ok(Self { conn })
    }

    /// **Test-only** raw connection accessor.
    #[doc(hidden)]
    pub fn __test_conn(&self) -> &Connection {
        &self.conn
    }

    // ---- Strategy façade ----

    /// Back-compat thin wrapper: registers a legacy (source-only) strategy.
    /// Hash and id are byte-for-byte identical to v1.0..v1.3 — passing
    /// `None`/`None` for records/view here is what keeps existing
    /// `strategy_id` values stable across the v1.4 upgrade.
    pub fn register_strategy(
        &mut self,
        name: &str,
        source: &str,
        description: Option<&str>,
        tags: Option<&[String]>,
    ) -> Result<strategies::RegisterOutcome, StateError> {
        // v1.5 Track 1B back-compat: legacy callers (tests, old code paths)
        // skip the contracts_touched extraction entirely — same id, same row
        // shape as v1.0..v1.4. The bundle-aware path is the new opt-in.
        strategies::register(&self.conn, name, source, description, tags, None, None, None)
    }

    /// v1.4 bundle register. Pass `records_json` (canonical JSON of the
    /// `records` schema) and/or `view_source` (the `view` function JS source)
    /// to opt into self-describing strategy semantics. The strategy id mixes
    /// all three (execute + records + view) so distinct bundles never collide.
    ///
    /// v1.5 Track 1B: `contracts_touched_json` is a DERIVATION computed by
    /// `executor-mcp::contracts_touched::extract` from `source`. It is cached
    /// at register time, NEVER folded into the id hash, and read back on
    /// every `strategy://{id}` to drive policy alignment. Pass `None` from
    /// non-MCP callers that don't run the extractor.
    #[allow(clippy::too_many_arguments)]
    pub fn register_strategy_bundle(
        &mut self,
        name: &str,
        source: &str,
        description: Option<&str>,
        tags: Option<&[String]>,
        records_json: Option<&str>,
        view_source: Option<&str>,
        contracts_touched_json: Option<&str>,
    ) -> Result<strategies::RegisterOutcome, StateError> {
        strategies::register(
            &self.conn,
            name,
            source,
            description,
            tags,
            records_json,
            view_source,
            contracts_touched_json,
        )
    }

    pub fn list_strategies(
        &self,
        include_deleted: bool,
    ) -> Result<Vec<strategies::StrategySummary>, StateError> {
        strategies::list(&self.conn, include_deleted)
    }

    pub fn get_strategy_by_id(
        &self,
        id: &str,
    ) -> Result<Option<strategies::Strategy>, StateError> {
        strategies::get_by_id(&self.conn, id)
    }

    pub fn get_strategy_by_name(
        &self,
        name: &str,
    ) -> Result<Option<strategies::Strategy>, StateError> {
        strategies::get_by_name(&self.conn, name)
    }

    pub fn soft_delete_strategy(&mut self, id: &str) -> Result<String, StateError> {
        strategies::soft_delete(&self.conn, id)
    }

    pub fn is_strategy_deleted(&self, id: &str) -> Result<Option<bool>, StateError> {
        strategies::is_deleted(&self.conn, id)
    }

    // ---- Records capture façade (v1.4 strategy bundle) ----

    /// Insert one row into `strategy_records_capture`. Callers (the
    /// capture-hook in `executor-mcp::tools`) are expected to wrap this in a
    /// swallow-error guard — capture failure must NEVER propagate back into
    /// the action-confirm path.
    pub fn record_strategy_capture(
        &mut self,
        run_id: &str,
        strategy_id: &str,
        record_name: &str,
        payload_json: &str,
    ) -> Result<(), StateError> {
        records_capture::insert(&self.conn, run_id, strategy_id, record_name, payload_json)
    }

    /// List capture rows for a strategy, newest-first. `since` is an
    /// exclusive lower bound on `captured_at` (RFC3339 string compare);
    /// `limit` is hard-capped at 500.
    pub fn list_strategy_records(
        &self,
        strategy_id: &str,
        since: Option<&str>,
        limit: u64,
    ) -> Result<Vec<records_capture::RecordCaptureEntry>, StateError> {
        records_capture::list_for_strategy(&self.conn, strategy_id, since, limit)
    }

    // ---- Run façade ----

    pub fn insert_run(
        &mut self,
        strategy_id: &str,
        status: RunStatus,
    ) -> Result<String, StateError> {
        runs::insert_run(&self.conn, strategy_id, status)
    }

    /// **Test-only.** Insert a run with a caller-supplied `started_at` so
    /// integration tests can assert deterministic ordering in
    /// `list_runs_for_strategy` without `now_rfc3339`'s seconds granularity
    /// causing same-timestamp collisions. Production code paths MUST use
    /// [`StateStore::insert_run`].
    #[doc(hidden)]
    pub fn __test_insert_run_with_time(
        &mut self,
        strategy_id: &str,
        status: RunStatus,
        started_at: &str,
    ) -> Result<String, StateError> {
        runs::insert_run_with_started_at(&self.conn, strategy_id, status, started_at)
    }

    /// **Deprecated** — use [`StateStore::update_run_status_with_transition`]
    /// (D-12 transition guard). The unguarded API allows non-monotonic
    /// status mutations and is a defense-in-depth bypass surface (MR-02).
    /// Phase 5/6 simulation/policy-failure transitions MUST also route
    /// through the transition-guarded variant.
    #[deprecated(
        note = "use update_run_status_with_transition (D-12 transition guard); \
                the unguarded variant bypasses the state-machine and will be \
                removed once Phase 5/6 emit reserved-variant transitions \
                through the guarded API"
    )]
    pub fn update_run_status(
        &mut self,
        run_id: &str,
        status: RunStatus,
    ) -> Result<(), StateError> {
        #[allow(deprecated)]
        runs::update_run_status(&self.conn, run_id, status)
    }

    pub fn get_run(&self, run_id: &str) -> Result<Option<runs::Run>, StateError> {
        runs::get_run(&self.conn, run_id)
    }

    pub fn list_runs_for_strategy(
        &self,
        strategy_id: &str,
    ) -> Result<Vec<runs::Run>, StateError> {
        runs::list_runs_for_strategy(&self.conn, strategy_id)
    }

    /// v1.4 Track C: filtered, paginated run-summary listing for the
    /// `execution://list` MCP resource. See [`runs::RunListFilter`] for filter
    /// semantics; `limit` defaults to [`runs::LIST_RUNS_DEFAULT_LIMIT`] (50)
    /// and is hard-capped at [`runs::LIST_RUNS_LIMIT_CAP`] (500). Results
    /// are sorted newest-first by `(started_at DESC, id DESC)`.
    pub fn list_runs(
        &self,
        filter: &runs::RunListFilter,
    ) -> Result<Vec<runs::RunSummary>, StateError> {
        runs::list_runs(&self.conn, filter)
    }

    /// D-12: transition-guarded status update. See
    /// `runs::update_run_status_with_transition` doc.
    pub fn update_run_status_with_transition(
        &mut self,
        run_id: &str,
        from: RunStatus,
        to: RunStatus,
    ) -> Result<(), StateError> {
        runs::update_run_status_with_transition(&self.conn, run_id, from, to)
    }

    // ---- Journal façade (Phase 3 D-06) ----

    pub fn record_source_read(
        &mut self,
        run_id: &str,
        kind: &str,
        target: &str,
        payload_json: Option<&str>,
    ) -> Result<String, StateError> {
        crate::journal::record_source_read(&self.conn, run_id, kind, target, payload_json)
    }

    pub fn record_action_outcome(
        &mut self,
        run_id: &str,
        outcome: executor_core::schema::execution::JournalActionOutcome,
        payload_json: &str,
    ) -> Result<String, StateError> {
        crate::journal::record_action_outcome(&self.conn, run_id, outcome, payload_json)
    }

    pub fn record_log(&mut self, run_id: &str, message: &str) -> Result<String, StateError> {
        crate::journal::record_log(&self.conn, run_id, message)
    }

    /// Test-only deterministic-time variant for ordering assertions.
    /// Mirrors `__test_insert_run_with_time`.
    #[doc(hidden)]
    pub fn __test_record_log_with_time(
        &mut self,
        run_id: &str,
        message: &str,
        recorded_at: &str,
    ) -> Result<String, StateError> {
        crate::journal::record_log_with_time(&self.conn, run_id, message, recorded_at)
    }

    /// Test-only deterministic-time variant of `record_source_read` (Phase 4
    /// MR-04 carry-forward — same-millisecond ordering proof).
    #[doc(hidden)]
    pub fn __test_record_source_read_with_time(
        &mut self,
        run_id: &str,
        kind: &str,
        target: &str,
        payload_json: Option<&str>,
        recorded_at: &str,
    ) -> Result<String, StateError> {
        crate::journal::record_source_read_with_time(
            &self.conn, run_id, kind, target, payload_json, recorded_at,
        )
    }

    pub fn list_source_reads_for_run(
        &self,
        run_id: &str,
    ) -> Result<Vec<crate::journal::SourceReadEntry>, StateError> {
        crate::journal::list_source_reads_for_run(&self.conn, run_id)
    }

    pub fn list_actions_for_run(
        &self,
        run_id: &str,
    ) -> Result<Vec<crate::journal::ActionEntry>, StateError> {
        crate::journal::list_actions_for_run(&self.conn, run_id)
    }

    pub fn list_logs_for_run(
        &self,
        run_id: &str,
    ) -> Result<Vec<crate::journal::LogEntry>, StateError> {
        crate::journal::list_logs_for_run(&self.conn, run_id)
    }

    // ---- Phase 5 D-09 journal_decisions façade ----

    #[allow(clippy::too_many_arguments)]
    pub fn record_decision(
        &mut self,
        run_id: &str,
        action_index: i64,
        gate: crate::journal::DecisionGate,
        verdict: crate::journal::DecisionVerdict,
        rule: Option<&str>,
        detail: Option<&str>,
        payload: Option<&serde_json::Value>,
    ) -> Result<String, StateError> {
        crate::journal::record_decision(
            &self.conn,
            run_id,
            action_index,
            gate,
            verdict,
            rule,
            detail,
            payload,
        )
    }

    #[doc(hidden)]
    #[allow(clippy::too_many_arguments)]
    pub fn __test_record_decision_with_time(
        &mut self,
        run_id: &str,
        action_index: i64,
        gate: crate::journal::DecisionGate,
        verdict: crate::journal::DecisionVerdict,
        rule: Option<&str>,
        detail: Option<&str>,
        payload: Option<&serde_json::Value>,
        recorded_at: &str,
    ) -> Result<String, StateError> {
        crate::journal::record_decision_with_time(
            &self.conn,
            run_id,
            action_index,
            gate,
            verdict,
            rule,
            detail,
            payload,
            recorded_at,
        )
    }

    pub fn list_decisions_for_run(
        &self,
        run_id: &str,
    ) -> Result<Vec<crate::journal::DecisionEntry>, StateError> {
        crate::journal::list_decisions_for_run(&self.conn, run_id)
    }

    // ---- Phase 6 execution_actions façade ----

    pub fn record_execution_broadcast(
        &mut self,
        run_id: &str,
        action_index: i64,
        signer_address: &str,
        tx_hash: &str,
    ) -> Result<String, StateError> {
        executions::record_broadcast(
            &self.conn,
            executions::NewExecutionBroadcast {
                run_id,
                action_index,
                signer_address,
                tx_hash,
            },
        )
    }

    pub fn record_execution_receipt_success(
        &mut self,
        run_id: &str,
        action_index: i64,
        receipt_status: &str,
        gas_used: &str,
    ) -> Result<(), StateError> {
        executions::record_receipt_success(
            &self.conn,
            run_id,
            action_index,
            receipt_status,
            gas_used,
        )
    }

    pub fn record_execution_error(
        &mut self,
        run_id: &str,
        action_index: i64,
        signer_address: Option<&str>,
        error_kind: &str,
        error_detail: Option<&str>,
    ) -> Result<(), StateError> {
        executions::record_execution_error(
            &self.conn,
            run_id,
            action_index,
            signer_address,
            error_kind,
            error_detail,
        )
    }

    pub fn list_executions_for_run(
        &self,
        run_id: &str,
    ) -> Result<Vec<executions::ExecutionActionEntry>, StateError> {
        executions::list_executions_for_run(&self.conn, run_id)
    }

    // ---- Trigger façade (v1.2 Trigger Core) ----

    pub fn register_trigger(
        &mut self,
        input: RegisterTriggerInput,
    ) -> Result<triggers::TriggerRegisterOutcome, StateError> {
        triggers::register(&self.conn, input)
    }

    pub fn list_triggers(
        &self,
        filter: Option<&TriggerListFilter>,
    ) -> Result<Vec<TriggerSummary>, StateError> {
        triggers::list(&self.conn, filter)
    }

    pub fn get_trigger(&self, id: &str) -> Result<Option<Trigger>, StateError> {
        triggers::get_by_id(&self.conn, id)
    }

    pub fn delete_trigger(&mut self, id: &str) -> Result<bool, StateError> {
        triggers::delete(&self.conn, id)
    }

    pub fn set_trigger_enabled(&mut self, id: &str, enabled: bool) -> Result<(), StateError> {
        triggers::set_enabled(&self.conn, id, enabled)
    }

    pub fn record_trigger_event(
        &mut self,
        trigger_id: &str,
        event_json: Option<&str>,
        run_id: Option<&str>,
        dedup_key: Option<&str>,
        skipped_reason: Option<&str>,
    ) -> Result<TriggerEvent, StateError> {
        triggers::record_event(
            &self.conn,
            trigger_id,
            event_json,
            run_id,
            dedup_key,
            skipped_reason,
        )
    }

    pub fn list_trigger_events(
        &self,
        trigger_id: &str,
        limit: u64,
    ) -> Result<Vec<TriggerEvent>, StateError> {
        triggers::list_events(&self.conn, trigger_id, limit)
    }

    pub fn check_trigger_dedup(
        &self,
        trigger_id: &str,
        dedup_key: &str,
        window_ms: u64,
    ) -> Result<bool, StateError> {
        triggers::check_dedup(&self.conn, trigger_id, dedup_key, window_ms)
    }

    // ---- v1.5 Track 1A — policy revisions façade ----

    /// Atomic deactivate-old + insert-new for the policy revisions table.
    /// Returns the freshly written row (revision_id, set_at populated).
    pub fn set_active_policy(
        &mut self,
        body_json: &str,
        rationale: Option<&str>,
    ) -> Result<policy_revisions::PolicyRevision, StateError> {
        policy_revisions::set_active(&mut self.conn, body_json, rationale)
    }

    /// Read the active policy revision, if one has been set.
    pub fn get_active_policy(
        &self,
    ) -> Result<Option<policy_revisions::PolicyRevision>, StateError> {
        policy_revisions::get_active(&self.conn)
    }

    /// List policy revisions newest-first. `limit` is hard-capped at 200.
    pub fn list_policy_revisions(
        &self,
        limit: u64,
    ) -> Result<Vec<policy_revisions::PolicyRevisionSummary>, StateError> {
        policy_revisions::list_revisions(&self.conn, limit)
    }
}
