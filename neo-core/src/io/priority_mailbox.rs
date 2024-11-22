use actix::prelude::*;
use std::sync::Arc;
use crate::io::priority_message_queue::PriorityMessageQueue;

pub struct PriorityMailbox {
    settings: Arc<Settings>,
    config: Arc<Config>,
}

impl PriorityMailbox {
    pub fn new(settings: Arc<Settings>, config: Arc<Config>) -> Self {
        PriorityMailbox {
            settings,
            config,
        }
    }

    pub fn create(&self, owner: Addr<dyn Actor>, system: &ActorSystem) -> PriorityMessageQueue {
        PriorityMessageQueue::new(
            Arc::new(move |message, queue| self.shall_drop(message, queue)),
            Arc::new(move |message| self.is_high_priority(message))
        )
    }

    fn is_high_priority(&self, _message: &dyn std::any::Any) -> bool {
        false
    }

    fn shall_drop(&self, _message: &dyn std::any::Any, _queue: &[&dyn std::any::Any]) -> bool {
        false
    }
}

#[derive(Clone)]
pub struct Settings {
    // Add necessary fields
}

#[derive(Clone)]
pub struct Config {
    // Add necessary fields
}

pub struct ActorSystem {
    // Add necessary fields
}
