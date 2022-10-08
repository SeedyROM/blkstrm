#[cfg(test)]
mod tests {
    use color_eyre::{eyre::eyre, Result};
    use futures::{
        stream::{self, FuturesUnordered},
        Stream, StreamExt,
    };
    use std::{collections::BTreeSet, fmt::Debug, pin::Pin, sync::Arc};
    use tokio::sync::{broadcast, mpsc, Mutex};

    // #[tokio::test]
    // async fn _test_stream_ideas() {
    //     // Create a channel to send data from the provider
    //     let (tx, mut rx) = mpsc::unbounded_channel::<u64>();

    //     // Provider streams
    //     let mut s0 = stream::iter(vec![0u64, 1, 2]);
    //     let mut s1 = stream::iter(vec![0u64, 3, 4]);
    //     let mut s2 = stream::iter(vec![0u64, 1, 2, 4, 5, 8]);

    //     // Provider emitters
    //     let s0_tx = tx.clone();
    //     let s1_tx = tx.clone();
    //     let s1_handle = tokio::spawn(async move {
    //         while let Some(item) = s1.next().await {
    //             println!("s1 sending item: {}", item);
    //             s1_tx.send(item).unwrap();
    //         }
    //     });
    //     let s0_handle = tokio::spawn(async move {
    //         while let Some(item) = s0.next().await {
    //             println!("s0 sending item: {}", item);
    //             s0_tx.send(item).unwrap();
    //         }
    //     });
    //     let s2_tx = tx.clone();
    //     let s2_handle = tokio::spawn(async move {
    //         while let Some(item) = s2.next().await {
    //             println!("s2 sending item: {}", item);
    //             s2_tx.send(item).unwrap();
    //         }
    //     });

    //     //
    //     // The actual experiment.
    //     //

    //     // Producer stream output.
    //     let (o_tx, mut o_rx) = mpsc::unbounded_channel::<u64>();

    //     // Handle incoming provider values and return them in sequence.
    //     let values = Arc::new(Mutex::new(BTreeSet::<u64>::new()));
    //     let cache = Arc::new(Mutex::new(BTreeSet::<u64>::new()));
    //     let cache_size_max = 3;

    //     // Consumer task.
    //     let consumer_values = values.clone();
    //     let consumer_cache = cache.clone();
    //     let consumer_handle = tokio::spawn(async move {
    //         let mut last_seen = None;
    //         while let Ok(item) = rx.try_recv() {
    //             // Get locks on both values and cache each iteration
    //             let mut values = consumer_values.lock().await;
    //             let mut cache = consumer_cache.lock().await;

    //             // Ignore if in cache
    //             if cache.contains(&item) {
    //                 continue;
    //             }

    //             // If we've seen other values, ignore if below the last seen value
    //             if let Some(last_seen) = last_seen {
    //                 if item < last_seen {
    //                     continue;
    //                 }
    //             }

    //             // Insert into values
    //             values.insert(item);

    //             // Get the last value from the set
    //             let last_value = values.iter().next().unwrap().clone();

    //             // Send it out
    //             o_tx.send(last_value).unwrap();

    //             // Remove it from the set
    //             values.remove(&last_value);

    //             // If the cache is full, remove the oldest item
    //             if &cache.len() >= &cache_size_max {
    //                 let x = cache.iter().next().unwrap().clone();
    //                 cache.remove(&x);
    //             }

    //             // Update the cache
    //             cache.insert(last_value);

    //             // Update the last seen value
    //             last_seen = Some(last_value);
    //         }
    //     });

    //     // Join all the handles
    //     let _ = futures::join!(s0_handle, s1_handle, s2_handle, consumer_handle);

    //     println!("Values left in queue: {:?}", values.lock().await);
    //     println!("Cache: {:?}", cache.lock().await);

    //     // Confirm that we received all values without duplicates in sequential order
    //     // It's possible we'll get 1, 2, 3, 5 but that's fine since tasks run <= block height.
    //     let mut expected = BTreeSet::new();
    //     let mut actual = BTreeSet::new();
    //     for i in 0..6 {
    //         expected.insert(i);
    //     }
    //     while let Some(item) = o_rx.recv().await {
    //         println!("Received item: {}", item);
    //         actual.insert(item);
    //     }
    //     println!("Expected in range: {:?}", expected);
    //     println!("Actual: {:?}", actual);
    //     assert!(expected.iter().last() <= actual.iter().last());
    // }

    #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct ProviderState<T> {
        pub status: bool,
        pub last_seen: Option<T>,
    }

    impl<T> Default for ProviderState<T> {
        fn default() -> Self {
            Self {
                status: false,
                last_seen: None,
            }
        }
    }

    pub struct Provider<T, S>
    where
        S: Stream<Item = Result<T>> + Send + Sync,
    {
        pub(crate) state: Arc<Mutex<ProviderState<T>>>,
        pub(crate) stream: Pin<Box<S>>,
        pub(crate) outbound: mpsc::UnboundedSender<T>,
    }

    impl<T, S> Provider<T, S>
    where
        T: Clone + Debug + Send + Sync + 'static,
        S: Stream<Item = Result<T>> + Send + Sync,
    {
        pub fn new(outbound: mpsc::UnboundedSender<T>, stream: S) -> Self {
            Self {
                state: Arc::new(Mutex::new(ProviderState::default())),
                stream: Box::pin(stream),
                outbound,
            }
        }

        pub async fn run(
            state: Arc<Mutex<ProviderState<T>>>,
            stream: &mut Pin<Box<S>>,
            outbound: &mpsc::UnboundedSender<T>,
        ) -> Result<()> {
            while let Some(item) = stream.next().await {
                match item {
                    Ok(item) => {
                        let mut state = state.lock().await;
                        state.status = true;
                        outbound.send(item.clone())?;
                        state.last_seen = Some(item.clone());
                    }
                    Err(_) => {
                        let mut state = state.lock().await;
                        state.status = false;
                    }
                }
            }
            Ok(())
        }
    }

    pub struct ProviderSystem<T, S>
    where
        S: Stream<Item = Result<T>> + Send + Sync,
    {
        pub providers: Vec<Provider<T, S>>,
        pub outbound: mpsc::UnboundedSender<T>,
    }

    impl<T, S> ProviderSystem<T, S>
    where
        T: Clone + Debug + Send + Sync + 'static,
        S: Stream<Item = Result<T>> + Send + Sync,
    {
        pub fn new(outbound: mpsc::UnboundedSender<T>) -> Self {
            Self {
                providers: Vec::new(),
                outbound,
            }
        }

        pub fn add_provider_stream(&mut self, stream: S) {
            self.providers
                .push(Provider::new(self.outbound.clone(), stream));
        }

        pub async fn produce(&mut self) -> Result<()> {
            let mut provider_futures = FuturesUnordered::new();

            for provider in self.providers.iter_mut() {
                provider_futures.push(Provider::run(
                    provider.state.clone(),
                    &mut provider.stream,
                    &provider.outbound,
                ));
            }

            while let Some(_) = provider_futures.next().await {}

            Ok(())
        }
    }

    pub struct Sequencer<T> {
        pub inbound: mpsc::UnboundedReceiver<T>,
        pub outbound: mpsc::UnboundedSender<T>,
        pub queue: Arc<Mutex<BTreeSet<T>>>,
        pub cache: Arc<Mutex<BTreeSet<T>>>,
        pub cache_size_max: usize,
    }

    impl<T> Sequencer<T>
    where
        T: Clone + Debug + Ord,
    {
        pub fn new(
            inbound: mpsc::UnboundedReceiver<T>,
            outbound: mpsc::UnboundedSender<T>,
            cache_size_max: usize,
        ) -> Self {
            Self {
                inbound,
                outbound,
                queue: Arc::new(Mutex::new(BTreeSet::new())),
                cache: Arc::new(Mutex::new(BTreeSet::new())),
                cache_size_max,
            }
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

    #[tokio::test]
    async fn test_stream_ideas_v2() {
        let s0 = stream::iter(vec![Ok(0), Ok(1)]);
        let s1 = stream::iter(vec![Ok(0), Ok(1), Ok(2), Ok(3), Ok(4)]);
        let s2 = stream::iter(vec![Ok(2), Err(eyre!("Bad block!!!!")), Ok(5)]);

        let (provider_tx, provider_rx) = mpsc::unbounded_channel::<i32>();
        let mut provider_system = ProviderSystem::new(provider_tx);

        provider_system.add_provider_stream(s0);
        provider_system.add_provider_stream(s1);
        provider_system.add_provider_stream(s2);

        let provider_system_handle = tokio::spawn(async move {
            provider_system.produce().await.unwrap();
        });

        let (sequencer_tx, sequencer_rx) = mpsc::unbounded_channel();
        let mut sequencer = Sequencer::new(provider_rx, sequencer_tx, 1);

        let sequencer_handle = tokio::spawn(async move {
            sequencer.consume().await.unwrap();
        });

        let (dispatcher_tx, _dispatcher_rx) = broadcast::channel(10);
        let mut dispatcher = Dispatcher::new(sequencer_rx, dispatcher_tx.clone());

        let dispatcher_handle = tokio::spawn(async move {
            dispatcher.fanout().await.unwrap();
        });

        let mut assertion_handle_rx = dispatcher_tx.subscribe();
        let assertion_handle_0 = tokio::spawn(async move {
            let expected = vec![0, 1, 2, 3, 4, 5];
            while let Ok(item) = assertion_handle_rx.try_recv() {
                println!("Process #1 received item: {}", item);
                assert!(item <= *expected.iter().last().unwrap());
            }
        });

        let mut assertion_handle_rx = dispatcher_tx.subscribe();
        let assertion_handle_1 = tokio::spawn(async move {
            let expected = vec![0, 1, 2, 3, 4, 5];
            while let Ok(item) = assertion_handle_rx.try_recv() {
                println!("Process #2 received item: {}", item);
                assert!(item <= *expected.iter().last().unwrap());
            }
        });

        let _ = tokio::try_join!(
            provider_system_handle,
            sequencer_handle,
            dispatcher_handle,
            assertion_handle_0,
            assertion_handle_1
        );
    }
}
