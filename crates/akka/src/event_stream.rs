use crate::actor_ref::ActorRef;
use parking_lot::RwLock;
use std::{any::TypeId, collections::HashMap, sync::Arc};

#[derive(Default)]
pub struct EventStream {
    subscribers: RwLock<HashMap<TypeId, Vec<ActorRef>>>,
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

    pub fn publish<T>(&self, message: T)
    where
        T: Clone + Send + 'static,
    {
        let subs = self.subscribers.read();
        if let Some(entry) = subs.get(&TypeId::of::<T>()) {
            for actor in entry.iter() {
                let _ = actor.tell(message.clone());
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

    pub fn publish<T>(&self, message: T)
    where
        T: Clone + Send + 'static,
    {
        self.inner.publish(message);
    }
}
