//! v1.2 Trigger Core — Stream D (daemon + worker runtime).
//!
//! - [`event`] — `TriggerEvent` shuttled through the dispatcher channel.
//! - [`worker`] — `TriggerWorker` trait. Each kind has its own background loop.
//! - [`workers`] — concrete worker implementations (`interval`, `manual` stub).
//! - [`pool`] — `WorkerPool` spawns/aborts worker tasks keyed by trigger id.
//! - [`dispatcher`] — drains the channel, evaluates predicate + dedup, and
//!   invokes `ExecutorServer::run_strategy_with_event`.
//!
//! Stream A (state CRUD + types) and Stream B (`Sandbox::evaluate_predicate`,
//! `RuntimeContext::with_event`) are stubbed elsewhere in this PR; the
//! merger swaps them for the locked Stream A/B implementations.

pub mod dispatcher;
pub mod event;
pub mod pool;
pub mod worker;
pub mod workers;

pub use event::TriggerEvent;
pub use pool::WorkerPool;
