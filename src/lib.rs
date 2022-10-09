use color_eyre::{eyre::eyre, Result};
use futures::{stream::FuturesUnordered, Stream, StreamExt};
use std::{collections::BTreeSet, fmt::Debug, pin::Pin, sync::Arc};
use tokio::sync::{broadcast, mpsc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProviderState<T> {
    pub name: String,
    pub status: bool,
    pub last_seen: Option<T>,
}

impl<T> ProviderState<T> {
    fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
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
    pub fn new(name: String, outbound: mpsc::UnboundedSender<T>, stream: S) -> Self {
        Self {
            state: Arc::new(Mutex::new(ProviderState::new(name))),
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

    pub fn add_provider_stream(&mut self, name: impl Into<String>, stream: S) {
        self.providers
            .push(Provider::new(name.into(), self.outbound.clone(), stream));
    }

    pub fn get_provider_states(&self) -> Vec<Arc<Mutex<ProviderState<T>>>> {
        self.providers
            .iter()
            .map(|provider| provider.state.clone())
            .collect::<Vec<_>>()
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
    use color_eyre::eyre::eyre;
    use futures::stream;

    use super::*;

    // Helpers
    pub fn is_sorted<T: IntoIterator>(t: T) -> bool
    where
        <T as IntoIterator>::Item: std::cmp::PartialOrd,
    {
        let mut iter = t.into_iter();

        if let Some(first) = iter.next() {
            iter.try_fold(first, |previous, current| {
                if previous > current {
                    Err(())
                } else {
                    Ok(current)
                }
            })
            .is_ok()
        } else {
            true
        }
    }

    pub fn no_sequential_duplicates<T: IntoIterator>(t: T) -> bool
    where
        <T as IntoIterator>::Item: std::cmp::PartialEq,
    {
        let mut iter = t.into_iter();

        if let Some(first) = iter.next() {
            iter.try_fold(first, |previous, current| {
                if previous == current {
                    Err(())
                } else {
                    Ok(current)
                }
            })
            .is_ok()
        } else {
            true
        }
    }

    #[tokio::test]
    async fn system_architecture() {
        #[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
        struct Block(u64);

        let s0 = stream::iter(vec![Ok(Block(0)), Ok(Block(1))]);
        let s1 = stream::iter(vec![
            Ok(Block(0)),
            Ok(Block(2)),
            Ok(Block(1)),
            Ok(Block(0)),
            Ok(Block(4)),
            Ok(Block(3)),
            Err(eyre!("Bad block!!!!")),
        ]);
        let s2 = stream::iter(vec![
            Ok(Block(2)),
            Err(eyre!("Bad block!!!!")),
            Ok(Block(5)),
        ]);

        let (provider_tx, provider_rx) = mpsc::unbounded_channel::<Block>();
        let mut provider_system = ProviderSystem::new(provider_tx);

        provider_system.add_provider_stream("stream_1", s0);
        provider_system.add_provider_stream("stream_2", s1);
        provider_system.add_provider_stream("stream_3", s2);

        // Get information about each provider
        let provider_states = provider_system.get_provider_states();

        // Produce data from each provider into the sequencer
        let provider_system_handle = tokio::spawn(async move {
            provider_system.produce().await.unwrap();
        });

        // Take in a stream of data from the providers and produce a stream of dedup'd sorted data
        let (sequencer_tx, sequencer_rx) = mpsc::unbounded_channel();
        let mut sequencer = Sequencer::new(provider_rx, sequencer_tx, 1).unwrap();

        let sequencer_handle = tokio::spawn(async move {
            sequencer.consume().await.unwrap();
        });

        // Take in a stream of data and fan it out to all subscribers
        let (dispatcher_tx, _dispatcher_rx) = broadcast::channel(10);
        let mut dispatcher = Dispatcher::new(sequencer_rx, dispatcher_tx.clone());

        let dispatcher_handle = tokio::spawn(async move {
            dispatcher.fanout().await.unwrap();
        });

        // Assert these two subscribers receive a dedup'd sequence of blocks.
        let mut assertion_handle_rx = dispatcher_tx.subscribe();
        let assertion_handle_0 = tokio::spawn(async move {
            let mut received = vec![];
            while let Ok(item) = assertion_handle_rx.try_recv() {
                // println!("Subscriber #1 received item: {:?}", item);
                received.push(item);
            }
            assert!(is_sorted(&received));
            assert!(no_sequential_duplicates(&received));
        });
        let mut assertion_handle_rx = dispatcher_tx.subscribe();
        let assertion_handle_1 = tokio::spawn(async move {
            let mut received = vec![];
            while let Ok(item) = assertion_handle_rx.try_recv() {
                // println!("Subscriber #2 received item: {:?}", item);
                received.push(item);
            }
            assert!(is_sorted(&received));
            assert!(no_sequential_duplicates(&received));
        });

        // Join em up.
        let _ = tokio::try_join!(
            provider_system_handle,
            sequencer_handle,
            dispatcher_handle,
            assertion_handle_0,
            assertion_handle_1
        )
        .unwrap();

        // Check that all providers are in the expected state
        let expected_states = vec![(true, Block(1)), (false, Block(3)), (true, Block(5))];
        for (i, provider_state) in provider_states.iter().enumerate() {
            let provider_state = provider_state.lock().await;
            assert_eq!(provider_state.status, expected_states[i].0);
            assert_eq!(provider_state.last_seen, Some(expected_states[i].1));
        }
    }
}
