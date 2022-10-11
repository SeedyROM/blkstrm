#![cfg(not(tarpaulin_include))]

use async_stream::try_stream;
use blkstrm::{
    Dispatcher, ProviderSystem, ProviderSystemMonitor, ProviderSystemMonitorState, Sequencer,
};
use color_eyre::{eyre::eyre, Result};
use futures::Stream;
use rand::distributions::{Distribution, Uniform};
use tokio::sync::{broadcast, mpsc};

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
struct Block(u64);

fn block_stream(start: u64) -> impl Stream<Item = Result<Block>> {
    let step = Uniform::new(5, 8);
    let mut rng = rand::thread_rng();
    let rand_error = step.sample(&mut rng);
    try_stream! {
        for i in start.. {
            if i % rand_error == 0 {
                Err(eyre!("Transient error at block {}", i))?
            } else {
            yield Block(i);
            }

            tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let (provider_tx, provider_rx) = mpsc::unbounded_channel::<Block>();

    // Create streams of blocks
    let s0 = block_stream(1);
    let s1 = block_stream(5);
    let s2 = block_stream(3);

    // Create a provider for each stream
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
    let mut sequencer = Sequencer::new(provider_rx, sequencer_tx, 4).unwrap();
    let sequencer_handle = tokio::spawn(async move {
        sequencer.consume().await.unwrap();
    });

    // Take in a stream of data and fan it out to all subscribers
    let (dispatcher_tx, _dispatcher_rx) = broadcast::channel(10);
    let mut dispatcher = Dispatcher::new(sequencer_rx, dispatcher_tx.clone());
    let dispatcher_handle = tokio::spawn(async move {
        dispatcher.fanout().await.unwrap();
    });

    let (monitor_tx, mut monitor_rx) = mpsc::channel(10);
    let monitor = ProviderSystemMonitor::new(provider_states, monitor_tx);
    let monitor_handle = tokio::spawn(async move {
        monitor.monitor(2000).await.unwrap();
    });
    let monitor_watch_handle = tokio::spawn(async move {
        while let Some(msg) = monitor_rx.recv().await {
            match msg {
                ProviderSystemMonitorState::AllProvidersStopped => {
                    println!("All providers stopped");
                }
                ProviderSystemMonitorState::ProblemProviders { lagging, stopped } => {
                    println!(
                        "Problem providers: lagging: {:#?}, stopped: {:#?}",
                        lagging, stopped
                    );
                }
                ProviderSystemMonitorState::ProvidersOk => {
                    println!("All providers ok");
                }
            }
        }
    });

    let _ = tokio::try_join!(
        provider_system_handle,
        sequencer_handle,
        dispatcher_handle,
        monitor_handle,
        monitor_watch_handle
    )
    .unwrap();

    Ok(())
}
