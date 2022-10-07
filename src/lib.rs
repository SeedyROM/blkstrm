mod test;

// // use color_eyre::{eyre::eyre, Result};
// // use futures::{Stream, StreamExt};
// // use std::{collections::BTreeSet, fmt::Debug, pin::Pin};
// // use tokio::sync::broadcast;

// // pub struct ProviderState<T> {
// //     status: bool,
// //     last_seen: Option<T>,
// // }

// // pub struct Provider<T, S>
// // where
// //     S: Stream<Item = Result<T>>,
// // {
// //     state: ProviderState<T>,
// //     stream: Pin<Box<S>>,
// // }

// // // Implement provider
// // impl<T, S> Provider<T, S>
// // where
// //     T: Debug + Clone + Send,
// //     S: Stream<Item = Result<T>>,
// // {
// //     pub fn new(stream: S) -> Self {
// //         Self {
// //             state: ProviderState {
// //                 status: false,
// //                 last_seen: None,
// //             },
// //             stream: Box::pin(stream),
// //         }
// //     }

// //     pub fn status(&self) -> bool {
// //         self.state.status
// //     }

// //     pub fn last_seen(&self) -> Option<&T> {
// //         self.state.last_seen.as_ref()
// //     }

// //     pub async fn run(&mut self, sender: broadcast::Sender<T>) -> Result<()> {
// //         while let Some(item) = self.stream.next().await {
// //             println!("Received item: {:?}", item);
// //             let item = item?;
// //             self.state.status = true;
// //             self.state.last_seen = Some(item.clone());
// //             sender
// //                 .send(item)
// //                 .map_err(|err| eyre!("Failed to send item: {}", err))?;
// //         }
// //         self.state.status = false;
// //         Ok(())
// //     }
// // }

// // pub struct ProviderHandle<T, S>
// // where
// //     T: Clone + Send,
// //     S: Stream<Item = Result<T>>,
// // {
// //     sender: broadcast::Sender<T>,
// //     _marker: std::marker::PhantomData<S>,
// // }

// // impl<T, S> ProviderHandle<T, S>
// // where
// //     T: Debug + Clone + Send + 'static,
// //     S: Stream<Item = Result<T>> + Send + 'static,
// // {
// //     pub fn new(sender: broadcast::Sender<T>, stream: S) -> Self {
// //         let mut actor = Provider::<T, S>::new(stream);
// //         let actor_sender = sender.clone();
// //         tokio::spawn(async move { actor.run(actor_sender).await });
// //         Self {
// //             sender,
// //             _marker: std::marker::PhantomData,
// //         }
// //     }
// // }

// // pub struct Producer<T> {
// //     sender: broadcast::Sender<T>,
// //     queue: BTreeSet<T>,
// //     cache: BTreeSet<T>,
// // }

// // impl<T> Producer<T>
// // where
// //     T: Debug + Clone + Ord + Send,
// // {
// //     pub fn new(sender: broadcast::Sender<T>) -> Self {
// //         Self {
// //             sender,
// //             queue: BTreeSet::new(),
// //             cache: BTreeSet::new(),
// //         }
// //     }

// //     pub async fn run(&mut self, mut receiver: broadcast::Receiver<T>) -> Result<()> {
// //         while let Ok(item) = receiver.recv().await {
// //             println!("Received item: {:?}", item);
// //             self.queue.insert(item.clone());
// //             self.cache.insert(item.clone());
// //         }

// //         Ok(())
// //     }
// // }

// // pub struct ProducerHandle<T> {
// //     _marker: std::marker::PhantomData<T>,
// // }

// // impl<T> ProducerHandle<T>
// // where
// //     T: Debug + Clone + Ord + Send + 'static,
// // {
// //     pub fn new(sender: broadcast::Sender<T>, receiver: broadcast::Receiver<T>) -> Self {
// //         let mut actor = Producer::<T>::new(sender);
// //         tokio::spawn(async move { actor.run(receiver).await });
// //         Self {
// //             _marker: std::marker::PhantomData,
// //         }
// //     }
// // }

// use std::{
//     collections::BTreeSet,
//     fmt::Debug,
//     pin::Pin,
//     task::{Context, Poll},
// };

// use async_stream::try_stream;
// use color_eyre::{eyre::eyre, Result};
// use futures::{Stream, StreamExt};
// use tokio::{
//     sync::broadcast,
//     task::{self, JoinHandle},
// };

// pub struct ProviderState<T> {
//     status: bool,
//     last_seen: Option<T>,
// }

// pub struct Provider<T, S> {
//     state: ProviderState<T>,
//     stream: Pin<Box<S>>,
// }

// // Implement provider
// impl<T, S> Provider<T, S>
// where
//     T: Debug + Clone + Send,
//     S: futures::Stream<Item = Result<T>>,
// {
//     pub fn new(stream: S) -> Self {
//         Self {
//             state: ProviderState {
//                 status: false,
//                 last_seen: None,
//             },
//             stream: Box::pin(stream),
//         }
//     }

//     pub fn status(&self) -> bool {
//         self.state.status
//     }

//     pub fn last_seen(&self) -> Option<&T> {
//         self.state.last_seen.as_ref()
//     }

//     pub async fn run(&mut self, sender: broadcast::Sender<T>) -> Result<()> {
//         while let Some(item) = self.stream.next().await {
//             println!("Received item: {:?}", item);
//             let item = item?;
//             self.state.status = true;
//             self.state.last_seen = Some(item.clone());
//             sender
//                 .send(item)
//                 .map_err(|err| eyre!("Failed to send item: {}", err))?;
//         }
//         self.state.status = false;
//         Ok(())
//     }
// }

// pub struct Producer<T, S> {
//     providers: Vec<Provider<T, S>>,
//     queue: BTreeSet<T>,
//     cache: BTreeSet<T>,
// }

// impl<T, S> Producer<T, S>
// where
//     T: Debug + Clone + Ord + Send,
//     S: futures::Stream<Item = Result<T>> + Send,
//     Provider<T, S>: Send + 'static,
// {
//     pub fn new() -> Self {
//         Self {
//             providers: Vec::new(),
//             queue: BTreeSet::new(),
//             cache: BTreeSet::new(),
//         }
//     }

//     pub fn add_provider(&mut self, provider: Provider<T, S>) {
//         self.providers.push(provider);
//     }

//     pub fn run(&mut self) -> impl Stream<Item = Result<()>> {
//         try_stream! {
//             let mut receivers = Vec::new();
//             for provider in self.providers.iter_mut() {
//                 let (sender, receiver) = broadcast::channel(1);
//                 receivers.push(receiver);
//                 task::spawn(async move { provider.run(sender).await });
//             }

//             loop {
//                 for receiver in receivers.iter_mut() {
//                     while let Ok(item) = receiver.recv().await {
//                         println!("Received item: {:?}", item);
//                         self.queue.insert(item.clone());
//                         self.cache.insert(item.clone());
//                     }
//                 }
//                 yield ();
//             }
//         }
//     }
// }

// #[cfg(test)]
// mod tests {
//     use futures::stream;

//     use super::*;

//     #[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
//     struct Block(u64);

//     #[tokio::test]
//     async fn it_works() {
//         // Example providers
//         let p0 = Provider::new(stream::iter(vec![Ok(Block(0)), Ok(Block(1))]));
//         let p1 = Provider::new(stream::iter(vec![
//             Ok(Block(0)),
//             Ok(Block(1)),
//             Ok(Block(2)),
//             Ok(Block(3)),
//             Ok(Block(4)),
//         ]));
//         let p2 = Provider::new(stream::iter(vec![
//             Ok(Block(2)),
//             Err(eyre!("Bad block!!!!")),
//             Ok(Block(5)),
//         ]));

//         // The main producer
//         let producer = Producer::new();
//         producer.add_provider(p0);
//         producer.add_provider(p1);
//         producer.add_provider(p2);

//         let stream = producer.stream();

//         // // Assert we recv blocks in sequence and no duplicates
//         // for i in 0..5 {
//         //     assert_eq!(stream.next().await, Some(Ok(Block(i))));
//         // }
//     }
// }
