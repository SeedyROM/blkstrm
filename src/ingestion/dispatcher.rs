use color_eyre::Result;

use std::fmt::Debug;
use tokio::sync::{broadcast, mpsc};

pub struct Dispatcher<T> {
    pub inbound: mpsc::UnboundedReceiver<T>,
    pub outbound: broadcast::Sender<T>,
}

impl<T> Dispatcher<T>
where
    T: Clone + Debug + Send + Sync + 'static,
{
    pub fn new(inbound: mpsc::UnboundedReceiver<T>, outbound: broadcast::Sender<T>) -> Self {
        Self { inbound, outbound }
    }

    pub async fn fanout(&mut self) -> Result<()> {
        while let Some(item) = self.inbound.recv().await {
            self.outbound.send(item.clone())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tokio::{
        sync::{broadcast, mpsc},
        try_join,
    };

    use super::*;

    #[tokio::test]
    async fn dispatch_fanout() {
        // Create channels for inbound and outbound
        let (inbound_tx, inbound_rx) = mpsc::unbounded_channel();
        let (outbound_tx, _outbound_rx) = broadcast::channel(100);

        // Create a dispatcher
        let mut dispatcher = Dispatcher::new(inbound_rx, outbound_tx.clone());

        // Spawn a task to fanout
        let dispatcher_handle = tokio::spawn(async move {
            dispatcher.fanout().await.unwrap();
        });

        // Send 16 messages
        for i in 0..16 {
            inbound_tx.send(i).unwrap();
        }

        // Spawn 16 tasks to receive messages
        let mut subscriber_handles = Vec::new();
        for _ in 0..16 {
            let mut outbound_rx = outbound_tx.subscribe();
            let handle = tokio::task::spawn(async move {
                for i in 0..10 {
                    assert_eq!(outbound_rx.recv().await.unwrap(), i);
                }
            });
            subscriber_handles.push(handle);
        }

        // Wait for all tasks to complete
        for handle in subscriber_handles {
            handle.await.unwrap();
        }

        // Drop senders to signal end of stream
        drop(inbound_tx);
        drop(outbound_tx);

        // Wait for dispatcher to complete
        let _ = try_join!(dispatcher_handle).unwrap();
    }
}
