use std::sync::Arc;

use tokio::{
    sync::broadcast::{error::RecvError, Receiver},
    task::JoinSet,
};

use crate::remote::Message;

#[async_trait::async_trait]
pub(crate) trait EventListener: Send + Sync + 'static {
    async fn on_event(&self, event: &Event) -> eyre::Result<()>;

    fn spawn_listener(
        self: &Arc<Self>,
        join_set: &mut JoinSet<eyre::Result<()>>,
        mut events: Receiver<Arc<Event>>,
    ) {
        let this = Arc::clone(self);
        join_set.spawn(async move {
            loop {
                let event = match events.recv().await {
                    Ok(e) => e,
                    Err(RecvError::Closed) => return Ok(()),
                    Err(e) => return Err(e.into()),
                };

                this.on_event(&event).await?;
            }
        });
    }
}

#[non_exhaustive]
pub(crate) enum Event {
    Message(Message),
}
