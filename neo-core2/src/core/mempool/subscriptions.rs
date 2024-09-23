use std::collections::HashMap;
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use std::sync::mpsc::{self, Sender, Receiver};
use std::thread;
use crate::core::mempoolevent::Event;

pub struct Pool {
    subscriptions_enabled: bool,
    subscriptions_on: Arc<AtomicBool>,
    stop_ch: Option<Sender<()>>,
    sub_ch: Option<Sender<Sender<Event>>>,
    unsub_ch: Option<Sender<Sender<Event>>>,
    events: Option<Receiver<Event>>,
}

impl Pool {
    // RunSubscriptions runs subscriptions thread if mempool subscriptions are enabled.
    // You should manually free the resources by calling StopSubscriptions on mempool shutdown.
    pub fn run_subscriptions(&mut self) {
        if !self.subscriptions_enabled {
            panic!("subscriptions are disabled");
        }
        if !self.subscriptions_on.load(Ordering::SeqCst) {
            self.subscriptions_on.store(true, Ordering::SeqCst);
            let stop_ch = self.stop_ch.clone().unwrap();
            let sub_ch = self.sub_ch.clone().unwrap();
            let unsub_ch = self.unsub_ch.clone().unwrap();
            let events = self.events.clone().unwrap();
            thread::spawn(move || {
                Pool::notification_dispatcher(stop_ch, sub_ch, unsub_ch, events);
            });
        }
    }

    // StopSubscriptions stops mempool events loop.
    pub fn stop_subscriptions(&mut self) {
        if !self.subscriptions_enabled {
            panic!("subscriptions are disabled");
        }
        if self.subscriptions_on.load(Ordering::SeqCst) {
            self.subscriptions_on.store(false, Ordering::SeqCst);
            if let Some(ref stop_ch) = self.stop_ch {
                let _ = stop_ch.send(());
            }
        }
    }

    // SubscribeForTransactions adds the given channel to the new mempool event broadcasting, so when
    // there is a new transactions added to the mempool or an existing transaction removed from
    // the mempool, you'll receive it via this channel. Make sure you're not changing the received
    // mempool events, as it may affect the functionality of other subscribers.
    pub fn subscribe_for_transactions(&self, ch: Sender<Event>) {
        if self.subscriptions_on.load(Ordering::SeqCst) {
            if let Some(ref sub_ch) = self.sub_ch {
                let _ = sub_ch.send(ch);
            }
        }
    }

    // UnsubscribeFromTransactions unsubscribes the given channel from new mempool notifications,
    // you can close it afterwards. Passing non-subscribed channel is a no-op.
    pub fn unsubscribe_from_transactions(&self, ch: Sender<Event>) {
        if self.subscriptions_on.load(Ordering::SeqCst) {
            if let Some(ref unsub_ch) = self.unsub_ch {
                let _ = unsub_ch.send(ch);
            }
        }
    }

    // notificationDispatcher manages subscription to events and broadcasts new events.
    fn notification_dispatcher(
        stop_ch: Sender<()>,
        sub_ch: Sender<Sender<Event>>,
        unsub_ch: Sender<Sender<Event>>,
        events: Receiver<Event>,
    ) {
        let mut tx_feed: HashMap<Sender<Event>, bool> = HashMap::new();
        loop {
            select! {
                recv(stop_ch) -> _ => {
                    return;
                }
                recv(sub_ch) -> sub => {
                    if let Ok(sub) = sub {
                        tx_feed.insert(sub, true);
                    }
                }
                recv(unsub_ch) -> unsub => {
                    if let Ok(unsub) = unsub {
                        tx_feed.remove(&unsub);
                    }
                }
                recv(events) -> event => {
                    if let Ok(event) = event {
                        for ch in tx_feed.keys() {
                            let _ = ch.send(event.clone());
                        }
                    }
                }
            }
        }
    }
}
