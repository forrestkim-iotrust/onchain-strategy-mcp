//! `TriggerWorker` async trait — one trait, many sources.
//!
//! Every trigger kind (interval / block / log / mempool / webhook) implements
//! this trait. Workers are spawned by the [`super::pool::WorkerPool`] and
//! send events into the shared dispatcher channel.

use async_trait::async_trait;
use tokio::sync::mpsc;

use super::event::TriggerEvent;

/// One running trigger source. Workers are boxed and spawned on the tokio
/// runtime; the returned future drives the source until cancelled (`JoinHandle::
/// abort`) or its `events` channel closes.
#[async_trait]
pub trait TriggerWorker: Send + 'static {
    /// Trigger kind string ("interval", "block", ...). Matches the
    /// `triggers.kind` column. Used by [`super::pool::WorkerPool::spawn`] to
    /// log + dispatch to the right concrete worker.
    fn kind() -> &'static str
    where
        Self: Sized;

    /// Run loop. Drops back to the runtime when the channel closes or the
    /// task is aborted. Must not panic on transient errors — log and continue.
    async fn run(self: Box<Self>, events: mpsc::Sender<TriggerEvent>);
}
