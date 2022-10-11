use color_eyre::Result;
use futures::{stream::FuturesUnordered, Stream, StreamExt};
use std::{fmt::Debug, pin::Pin, sync::Arc};
use tokio::sync::{mpsc, Mutex};

use crate::util::LastSeen;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderStatus {
    Running,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderState<T>
where
    T: PartialOrd + Ord,
{
    pub name: String,
    pub status: ProviderStatus,
    pub last_seen: Option<LastSeen<T>>,
}

impl<T> ProviderState<T>
where
    T: PartialOrd + Ord,
{
    fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: ProviderStatus::Stopped,
            last_seen: None,
        }
    }
}

pub struct Provider<T, S>
where
    T: Ord,
    S: Stream<Item = Result<T>> + Send + Sync,
{
    pub(crate) state: Arc<Mutex<ProviderState<T>>>,
    pub(crate) stream: Pin<Box<S>>,
    pub(crate) outbound: mpsc::UnboundedSender<T>,
}

impl<T, S> Provider<T, S>
where
    T: Ord + Clone + Debug + Send + Sync + 'static,
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
                    state.status = ProviderStatus::Running;
                    outbound.send(item.clone())?;
                    state.last_seen = Some(LastSeen::new(item));
                }
                Err(err) => {
                    let mut state = state.lock().await;
                    state.status = ProviderStatus::Stopped;
                    Err(err)?
                }
            }
        }
        Ok(())
    }
}

pub struct ProviderSystem<T, S>
where
    T: Ord,
    S: Stream<Item = Result<T>> + Send + Sync,
{
    pub providers: Vec<Provider<T, S>>,
    pub outbound: mpsc::UnboundedSender<T>,
}

impl<T, S> ProviderSystem<T, S>
where
    T: Ord + Clone + Debug + Send + Sync + 'static,
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

#[derive(Debug, Clone)]
pub enum ProviderSystemMonitorState {
    AllProvidersStopped,
    ProblemProviders {
        lagging: Vec<String>,
        stopped: Vec<String>,
    },
    ProvidersOk,
}

pub struct ProviderSystemMonitor<T>
where
    T: std::fmt::Debug + Ord,
{
    pub provider_states: Vec<Arc<Mutex<ProviderState<T>>>>,
    pub outbound: mpsc::Sender<ProviderSystemMonitorState>,
    pub state: Arc<Mutex<ProviderSystemMonitorState>>,
}

impl<T> ProviderSystemMonitor<T>
where
    T: std::fmt::Debug + PartialOrd + Ord,
{
    pub fn new(
        provider_states: Vec<Arc<Mutex<ProviderState<T>>>>,
        outbound: mpsc::Sender<ProviderSystemMonitorState>,
    ) -> Self {
        Self {
            provider_states,
            outbound,
            state: Arc::new(Mutex::new(ProviderSystemMonitorState::AllProvidersStopped)),
        }
    }

    pub async fn monitor(self, interval_millis: u64) -> Result<()> {
        loop {
            let mut current_provider_states = vec![];
            for state in self.provider_states.iter() {
                let state = state.lock().await;
                current_provider_states.push(state);
            }

            let max_last_seen_value = current_provider_states
                .iter()
                .filter(|state| state.last_seen.is_some())
                .map(|state| &state.last_seen.as_ref().unwrap().value)
                .max();

            let lagging = current_provider_states
                .iter()
                .filter(|state| {
                    state.last_seen.is_some()
                        && &state.last_seen.as_ref().unwrap().value < max_last_seen_value.unwrap()
                })
                .map(|state| state.name.clone())
                .collect::<Vec<_>>();

            let stopped = current_provider_states
                .iter()
                .filter(|state| state.status == ProviderStatus::Stopped)
                .map(|state| state.name.clone())
                .collect::<Vec<_>>();

            let state = if stopped.len() == current_provider_states.len() {
                ProviderSystemMonitorState::AllProvidersStopped
            } else if !lagging.is_empty() || !stopped.is_empty() {
                ProviderSystemMonitorState::ProblemProviders { lagging, stopped }
            } else {
                ProviderSystemMonitorState::ProvidersOk
            };

            self.state.lock().await.clone_from(&state);
            self.outbound.send(state).await?;

            tokio::time::sleep(std::time::Duration::from_millis(interval_millis)).await;
        }
    }
}
