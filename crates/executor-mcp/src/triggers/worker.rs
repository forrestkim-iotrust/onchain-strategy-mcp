//! `TriggerWorker` trait — common contract for background trigger loops.

use crate::triggers::event::TriggerEvent;

#[async_trait::async_trait]
pub trait TriggerWorker: Send + 'static {
    fn kind() -> &'static str
    where
        Self: Sized;

    async fn run(self: Box<Self>, events: tokio::sync::mpsc::Sender<TriggerEvent>);
}
