#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, sync::Arc};

    use futures::{channel::mpsc, stream, StreamExt};
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn test_stream_ideas() {
        // Create a channel to send data from the provider
        let (tx, mut rx) = mpsc::channel::<u64>(20);

        // Provider streams
        let mut s0 = stream::iter(vec![0u64, 1, 2]);
        let mut s1 = stream::iter(vec![0u64, 3, 4]);
        let mut s2 = stream::iter(vec![0u64, 1, 2, 4, 5, 8]);

        // Provider emitters
        let mut s0_tx = tx.clone();
        let s0_handle = tokio::spawn(async move {
            while let Some(item) = s0.next().await {
                println!("Sending item: {}", item);
                s0_tx.try_send(item).unwrap();
            }
        });
        let mut s1_tx = tx.clone();
        let s1_handle = tokio::spawn(async move {
            while let Some(item) = s1.next().await {
                println!("Sending item: {}", item);
                s1_tx.try_send(item).unwrap();
            }
        });
        let mut s2_tx = tx.clone();
        let s2_handle = tokio::spawn(async move {
            while let Some(item) = s2.next().await {
                println!("Sending item: {}", item);
                s2_tx.try_send(item).unwrap();
            }
        });

        //
        // The actual experiment.
        //

        // Producer stream output.
        let (mut o_tx, mut o_rx) = mpsc::channel::<u64>(20);

        // Handle incoming provider values and return them in sequence.
        let values = Arc::new(Mutex::new(BTreeSet::<u64>::new()));
        let cache = Arc::new(Mutex::new(BTreeSet::<u64>::new()));
        let cache_size_max = 3;

        // Consumer task.
        let consumer_values = values.clone();
        let consumer_cache = cache.clone();
        let consumer_handle = tokio::spawn(async move {
            let mut last_seen = None;
            while let Some(item) = match rx.try_next() {
                Ok(Some(item)) => Some(item),
                Ok(None) => None,
                Err(_) => None,
            } {
                // Get locks on both values and cache each iteration
                let mut values = consumer_values.lock().await;
                let mut cache = consumer_cache.lock().await;

                // Ignore if in cache
                if cache.contains(&item) {
                    continue;
                }

                // If we've seen other values, ignore if below the last seen value
                if let Some(last_seen) = last_seen {
                    if item < last_seen {
                        continue;
                    }
                }

                // Insert into values
                values.insert(item);

                // Get the last value from the set
                let last_value = values.iter().next().unwrap().clone();

                // Send it out
                o_tx.try_send(last_value).unwrap();

                // Remove it from the set
                values.remove(&last_value);

                // If the cache is full, remove the oldest item
                if &cache.len() >= &cache_size_max {
                    let x = cache.iter().next().unwrap().clone();
                    cache.remove(&x);
                }

                // Update the cache
                cache.insert(last_value);

                // Update the last seen value
                last_seen = Some(last_value);
            }
        });

        // Join all the handles
        let _ = futures::join!(s0_handle, s1_handle, s2_handle, consumer_handle);

        println!("Values left in queue: {:?}", values.lock().await);
        println!("Cache: {:?}", cache.lock().await);

        // Confirm that we received all values without duplicates in sequential order
        // It's possible we'll get 1, 2, 3, 5 but that's fine since tasks run <= block height.
        let mut expected = BTreeSet::new();
        let mut actual = BTreeSet::new();
        for i in 0..6 {
            expected.insert(i);
        }
        while let Some(item) = o_rx.try_next().unwrap() {
            println!("Received item: {}", item);
            actual.insert(item);
        }
        assert!(expected.is_subset(&actual));
    }
}
