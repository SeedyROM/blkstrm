use color_eyre::{eyre::eyre, Result};
use std::{collections::BTreeSet, fmt::Debug, sync::Arc};
use tokio::sync::{mpsc, Mutex};

pub struct Sequencer<T> {
    pub inbound: mpsc::UnboundedReceiver<T>,
    pub outbound: mpsc::UnboundedSender<T>,
    pub queue: Arc<Mutex<BTreeSet<T>>>,
    pub cache: Arc<Mutex<BTreeSet<T>>>,
    pub cache_size_max: usize,
}

impl<T> Sequencer<T>
where
    T: Clone + Debug + Ord + PartialOrd,
{
    pub fn new(
        inbound: mpsc::UnboundedReceiver<T>,
        outbound: mpsc::UnboundedSender<T>,
        cache_size_max: usize,
    ) -> Result<Self> {
        if cache_size_max == 0 {
            return Err(eyre!("Cache size must be greater than 0"));
        }

        Ok(Self {
            inbound,
            outbound,
            queue: Arc::new(Mutex::new(BTreeSet::new())),
            cache: Arc::new(Mutex::new(BTreeSet::new())),
            cache_size_max,
        })
    }

    pub async fn consume(&mut self) -> Result<()> {
        let mut last_seen = None;
        while let Some(item) = self.inbound.recv().await {
            let mut queue = self.queue.lock().await;
            let mut cache = self.cache.lock().await;

            // Ignore if in cache
            if cache.contains(&item) {
                continue;
            }

            // If we've seen other values, ignore if below the last seen value
            if let Some(last_seen) = last_seen.clone() {
                if item < last_seen {
                    continue;
                }
            }

            // Insert into queue
            queue.insert(item);

            // Get the last value from the set
            let last_value = queue.iter().next().unwrap().clone();

            // Send it out
            self.outbound
                .send(last_value.clone())
                .map_err(|err| eyre!("Failed to send item: {}", err))?;

            // Remove it from the set
            queue.remove(&last_value);

            // If the cache is full, remove the oldest item
            if &cache.len() >= &self.cache_size_max {
                let x = cache.iter().next().unwrap().clone();
                cache.remove(&x);
            }

            // Update the cache
            cache.insert(last_value.clone());

            // Update the last seen value
            last_seen = Some(last_value);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use tokio::try_join;

    use crate::util::{is_sorted, no_sequential_duplicates};

    use super::*;

    #[test]
    fn bad_sequencer_cache_size() {
        let (_inbound_tx, inbound_rx) = mpsc::unbounded_channel::<()>();
        let (outbound_tx, _outbound_rx) = mpsc::unbounded_channel();
        let result = Sequencer::new(inbound_rx, outbound_tx, 0);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn sequencer_consume() {
        // Create channels for inbound and outbound
        let (inbound_tx, inbound_rx) = mpsc::unbounded_channel();
        let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel();

        // Create a sequencer
        let mut sequencer = Sequencer::new(inbound_rx, outbound_tx.clone(), 10).unwrap();

        // Spawn a task to consume
        let sequencer_handle = tokio::spawn(async move {
            sequencer.consume().await.unwrap();
        });

        // Spawn 16 tasks to produce values
        let mut producers = vec![];
        for i in 0..16 {
            let inbound_tx = inbound_tx.clone();
            let producer = tokio::spawn(async move {
                let start = i * 2;
                for j in start..(start + 16) {
                    inbound_tx
                        .send(j)
                        .map_err(|err| eyre!("Failed to send item: {}", err))
                        .unwrap();
                }
            });
            producers.push(producer);
        }

        // Wait for the producers to finish
        for producer in producers {
            producer.await.unwrap();
        }

        // Drop the senders to signal the end of the stream
        drop(inbound_tx);
        drop(outbound_tx);

        // Wait for the sequencer to finish
        let _ = try_join!(sequencer_handle).unwrap();

        // Consume the outbound stream
        let mut expected = vec![];
        while let Some(item) = outbound_rx.recv().await {
            expected.push(item);
        }

        // Verify the outbound stream is sorted (aka sequential) and has no sequential duplicates
        is_sorted(&expected);
        no_sequential_duplicates(&expected);
    }
}
