use super::actor_ref::ActorRef;
use parking_lot::RwLock;
use std::{
    any::{Any, TypeId},
    collections::HashMap,
    sync::Arc,
};
use tokio::sync::mpsc;

/// Type-erased channel sender used to fan published events out to plain async
/// tasks rather than actor mailboxes. Stored as `Box<dyn Any>` keyed by the
/// event `TypeId` and downcast back to `mpsc::UnboundedSender<T>` on publish.
type ChannelSubscriber = Box<dyn Any + Send + Sync>;

#[derive(Default)]
pub struct EventStream {
    subscribers: RwLock<HashMap<TypeId, Vec<ActorRef>>>,
    channel_subscribers: RwLock<HashMap<TypeId, Vec<ChannelSubscriber>>>,
}

impl EventStream {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn subscribe<T: 'static + Send>(&self, actor: ActorRef) {
        let mut subs = self.subscribers.write();
        let entry = subs.entry(TypeId::of::<T>()).or_default();
        if !entry.iter().any(|existing| existing == &actor) {
            entry.push(actor);
        }
    }

    pub fn unsubscribe<T: 'static + Send>(&self, actor: &ActorRef) {
        let mut subs = self.subscribers.write();
        if let Some(entry) = subs.get_mut(&TypeId::of::<T>()) {
            entry.retain(|existing| existing != actor);
            if entry.is_empty() {
                subs.remove(&TypeId::of::<T>());
            }
        }
    }

    pub fn unsubscribe_all(&self, actor: &ActorRef) {
        let mut subs = self.subscribers.write();
        subs.retain(|_, entry| {
            entry.retain(|existing| existing != actor);
            !entry.is_empty()
        });
    }

    /// Subscribes a plain async task to events of type `T`, delivered over an
    /// unbounded tokio channel instead of an actor mailbox.
    ///
    /// This is the actor-free counterpart to [`subscribe`](Self::subscribe):
    /// the returned [`mpsc::UnboundedReceiver`] is driven directly from a
    /// `tokio::select!` loop. The subscription is removed automatically the
    /// next time an event is published after the receiver is dropped, so there
    /// is no explicit unsubscribe.
    pub fn subscribe_channel<T: Clone + Send + 'static>(&self) -> mpsc::UnboundedReceiver<T> {
        let (sender, receiver) = mpsc::unbounded_channel::<T>();
        let mut subs = self.channel_subscribers.write();
        subs.entry(TypeId::of::<T>())
            .or_default()
            .push(Box::new(sender));
        receiver
    }

    pub fn publish<T>(&self, message: T)
    where
        T: Clone + Send + 'static,
    {
        {
            let subs = self.subscribers.read();
            if let Some(entry) = subs.get(&TypeId::of::<T>()) {
                for actor in entry.iter() {
                    let _ = actor.tell(message.clone());
                }
            }
        }

        let mut channel_subs = self.channel_subscribers.write();
        if let Some(entry) = channel_subs.get_mut(&TypeId::of::<T>()) {
            // Deliver to each channel subscriber, pruning any whose receiver has
            // been dropped (send returns Err once the receiver is gone).
            entry.retain(|boxed| {
                match boxed.downcast_ref::<mpsc::UnboundedSender<T>>() {
                    Some(sender) => sender.send(message.clone()).is_ok(),
                    None => true,
                }
            });
            if entry.is_empty() {
                channel_subs.remove(&TypeId::of::<T>());
            }
        }
    }
}

#[derive(Clone)]
pub struct EventStreamHandle {
    inner: Arc<EventStream>,
}

impl EventStreamHandle {
    pub fn new(inner: Arc<EventStream>) -> Self {
        Self { inner }
    }

    pub fn subscribe<T: Send + 'static>(&self, actor: ActorRef) {
        self.inner.subscribe::<T>(actor);
    }

    pub fn unsubscribe<T: Send + 'static>(&self, actor: &ActorRef) {
        self.inner.unsubscribe::<T>(actor);
    }

    pub fn unsubscribe_all(&self, actor: &ActorRef) {
        self.inner.unsubscribe_all(actor);
    }

    /// Actor-free subscription: see [`EventStream::subscribe_channel`].
    pub fn subscribe_channel<T: Clone + Send + 'static>(&self) -> mpsc::UnboundedReceiver<T> {
        self.inner.subscribe_channel::<T>()
    }

    pub fn publish<T>(&self, message: T)
    where
        T: Clone + Send + 'static,
    {
        self.inner.publish(message);
    }
}
