mod dispatcher;
mod provider;
mod sequencer;

pub use dispatcher::*;
pub use provider::*;
pub use sequencer::*;

#[cfg(test)]
mod tests {
    use color_eyre::eyre::eyre;
    use futures::stream;
    use tokio::sync::{broadcast, mpsc};

    use crate::util::{is_sorted, no_sequential_duplicates};

    use super::*;

    #[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
    struct Block(u64);

    #[tokio::test]
    #[cfg_attr(tarpaulin, ignore)]
    async fn system_architecture() {
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
        let s2 = stream::iter(vec![Ok(Block(2)), Ok(Block(5))]);

        let (provider_tx, provider_rx) = mpsc::unbounded_channel::<Block>();
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

        // Assert these two subscribers receive a dedup'd sequence of blocks.
        // Don't join on them because they'll never finish.
        let mut assertion_handle_rx = dispatcher_tx.subscribe();
        let h0 = tokio::spawn(async move {
            let mut received = vec![];
            while let Ok(item) = assertion_handle_rx.recv().await {
                println!("Subscriber #1 received item: {:?}", item);
                received.push(item);
            }
            assert!(is_sorted(&received));
            assert!(no_sequential_duplicates(&received));
        });
        let mut assertion_handle_rx = dispatcher_tx.subscribe();
        let h1 = tokio::spawn(async move {
            let mut received = vec![];
            while let Ok(item) = assertion_handle_rx.recv().await {
                println!("Subscriber #2 received item: {:?}", item);
                received.push(item);
            }
            assert!(is_sorted(&received));
            assert!(no_sequential_duplicates(&received));
        });

        // Join em up.
        let _ =
            tokio::try_join!(provider_system_handle, sequencer_handle, dispatcher_handle).unwrap();

        // Drop the dispatch connection
        drop(dispatcher_tx);

        // Receive the messages
        let _ = tokio::try_join!(h0, h1).unwrap();

        // Check that all providers are in the expected state
        let expected_states = vec![
            (ProviderStatus::Running, Block(1)),
            (ProviderStatus::Stopped, Block(3)),
            (ProviderStatus::Running, Block(5)),
        ];
        for (i, provider_state) in provider_states.iter().enumerate() {
            let provider_state = provider_state.lock().await;
            println!("Provider_state: {:#?}", provider_state);
            assert_eq!(provider_state.status, expected_states[i].0);
            assert_eq!(
                provider_state.last_seen.as_ref().unwrap().value,
                expected_states[i].1
            );
        }
    }
}
