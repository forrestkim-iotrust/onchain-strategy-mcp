//! v1.2 Trigger Core — daemon runtime + workers (Stream D).
//!
//! This module owns the **runtime** half of the trigger spike: workers,
//! dispatcher, worker pool, shared event type.
//!
//! State CRUD and core types come from Streams A (`executor-state::triggers`,
//! `executor-core::schema::trigger`) and Stream B (`strategy_js::Sandbox::
//! evaluate_predicate`). Until those merge, this module hosts thin
//! `state_adapter` helpers and a placeholder predicate evaluator that
//! exercise the same call sites — the merge will replace these with the
//! canonical APIs.
//!
//! Layout:
//! - [`event`] — shared `TriggerEvent` struct emitted by all workers.
//! - [`worker`] — `TriggerWorker` async trait.
//! - [`workers`] — concrete workers (`interval`, `manual` stub).
//! - [`pool`] — `WorkerPool` lifecycle (spawn / stop / restart).
//! - [`dispatcher`] — consumes the shared `mpsc::Receiver<TriggerEvent>` and
//!   fires `strategy_run` once per accepted event.
//! - [`state_adapter`] — temporary local CRUD shim over `StateStore` (Stream A
//!   surface). Marked `pub(crate)` so the dispatcher / pool / boot wiring
//!   can use it without leaking to the MCP surface.

pub mod dispatcher;
pub mod event;
pub mod pool;
pub mod state_adapter;
pub mod worker;
pub mod workers;

pub use dispatcher::Dispatcher;
pub use event::TriggerEvent;
pub use pool::WorkerPool;
pub use worker::TriggerWorker;
