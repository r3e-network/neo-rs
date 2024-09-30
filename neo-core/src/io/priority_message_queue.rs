// Copyright (C) 2015-2024 The Neo Project.
//
// PriorityMessageQueue.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;
use std::any::Any;
use actix::prelude::*;

#[derive(Clone)]
pub struct Envelope {
    message: Box<dyn Any + Send>,
    sender: Addr<dyn Actor>,
}

pub struct PriorityMessageQueue {
    high: VecDeque<Envelope>,
    low: VecDeque<Envelope>,
    dropper: Arc<dyn Fn(&dyn Any, &[&dyn Any]) -> bool + Send + Sync>,
    priority_generator: Arc<dyn Fn(&dyn Any) -> bool + Send + Sync>,
    idle: AtomicI32,
}

impl PriorityMessageQueue {
    pub fn new(
        dropper: impl Fn(&dyn Any, &[&dyn Any]) -> bool + Send + Sync + 'static,
        priority_generator: impl Fn(&dyn Any) -> bool + Send + Sync + 'static,
    ) -> Self {
        PriorityMessageQueue {
            high: VecDeque::new(),
            low: VecDeque::new(),
            dropper: Arc::new(dropper),
            priority_generator: Arc::new(priority_generator),
            idle: AtomicI32::new(1),
        }
    }

    pub fn has_messages(&self) -> bool {
        !self.high.is_empty() || !self.low.is_empty()
    }

    pub fn count(&self) -> usize {
        self.high.len() + self.low.len()
    }

    pub fn enqueue(&mut self, receiver: Addr<dyn Actor>, envelope: Envelope) {
        self.idle.fetch_add(1, Ordering::SeqCst);
        if envelope.message.is::<Idle>() {
            return;
        }
        let all_messages: Vec<&dyn Any> = self.high.iter().chain(self.low.iter())
            .map(|e| e.message.as_ref() as &dyn Any)
            .collect();
        if (self.dropper)(envelope.message.as_ref(), &all_messages) {
            return;
        }
        let queue = if (self.priority_generator)(envelope.message.as_ref()) {
            &mut self.high
        } else {
            &mut self.low
        };
        queue.push_back(envelope);
    }

    pub fn try_dequeue(&mut self) -> Option<Envelope> {
        self.high.pop_front()
            .or_else(|| self.low.pop_front())
            .or_else(|| {
                if self.idle.fetch_sub(1, Ordering::SeqCst) > 0 {
                    Some(Envelope {
                        message: Box::new(Idle),
                        sender: Addr::new(actix::Arbiter::current()),
                    })
                } else {
                    None
                }
            })
    }

    pub fn clean_up(&mut self, _owner: Addr<dyn Actor>, _deadletters: &mut dyn MessageQueue) {
        // Implementation left empty as per the C# version
    }
}

pub struct Idle;

impl Default for PriorityMessageQueue {
    fn default() -> Self {
        Self::new(
            |_, _| false,
            |_| false,
        )
    }
}

pub trait MessageQueue {
    fn enqueue(&mut self, receiver: Addr<dyn Actor>, envelope: Envelope);
    fn try_dequeue(&mut self) -> Option<Envelope>;
    fn has_messages(&self) -> bool;
    fn count(&self) -> usize;
}

impl MessageQueue for PriorityMessageQueue {
    fn enqueue(&mut self, receiver: Addr<dyn Actor>, envelope: Envelope) {
        self.enqueue(receiver, envelope);
    }

    fn try_dequeue(&mut self) -> Option<Envelope> {
        self.try_dequeue()
    }

    fn has_messages(&self) -> bool {
        self.has_messages()
    }

    fn count(&self) -> usize {
        self.count()
    }
}
