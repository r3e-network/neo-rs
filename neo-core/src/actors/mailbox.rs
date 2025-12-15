use super::message::{Envelope, Idle, MailboxMessage};
use std::collections::VecDeque;
use std::sync::Arc;

/// Interface implemented by all mailbox implementations.
pub trait Mailbox: Send {
    fn enqueue(&mut self, message: MailboxMessage);
    fn dequeue(&mut self) -> Option<MailboxMessage>;
    fn is_empty(&self) -> bool;
}

/// Factory for constructing mailboxes when spawning actors.
pub type MailboxFactory = Arc<dyn Fn() -> Box<dyn Mailbox> + Send + Sync>;

/// Creates a mailbox factory that produces default FIFO mailboxes.
pub fn default_mailbox_factory() -> MailboxFactory {
    Arc::new(|| Box::new(DefaultMailbox::default()) as Box<dyn Mailbox>)
}

/// Default FIFO mailbox that prioritises system messages.
#[derive(Default)]
pub struct DefaultMailbox {
    queue: VecDeque<MailboxMessage>,
}

impl Mailbox for DefaultMailbox {
    fn enqueue(&mut self, message: MailboxMessage) {
        match message {
            MailboxMessage::System(_) => self.queue.push_front(message),
            _ => self.queue.push_back(message),
        }
    }

    fn dequeue(&mut self) -> Option<MailboxMessage> {
        self.queue.pop_front()
    }

    fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

/// View over the contents of a priority mailbox queue.
pub struct QueueView<'a> {
    high: &'a VecDeque<MailboxMessage>,
    low: &'a VecDeque<MailboxMessage>,
}

impl<'a> QueueView<'a> {
    pub fn iter(&'a self) -> impl Iterator<Item = &'a MailboxMessage> + 'a {
        self.high.iter().chain(self.low.iter())
    }
}

type DropperFn = Arc<dyn for<'a> Fn(&MailboxMessage, QueueView<'a>) -> bool + Send + Sync>;
type PriorityFn = Arc<dyn Fn(&MailboxMessage) -> bool + Send + Sync>;

/// Configuration options for [`PriorityMailbox`].
#[derive(Default, Clone)]
pub struct PriorityMailboxConfig {
    dropper: Option<DropperFn>,
    priority: Option<PriorityFn>,
}

impl PriorityMailboxConfig {
    pub fn with_dropper<F>(mut self, dropper: F) -> Self
    where
        F: for<'a> Fn(&MailboxMessage, QueueView<'a>) -> bool + Send + Sync + 'static,
    {
        self.dropper = Some(Arc::new(dropper));
        self
    }

    pub fn with_priority<F>(mut self, func: F) -> Self
    where
        F: Fn(&MailboxMessage) -> bool + Send + Sync + 'static,
    {
        self.priority = Some(Arc::new(func));
        self
    }
}

/// Mailbox implementation with support for priority ordering and duplicate dropping.
pub struct PriorityMailbox {
    high: VecDeque<MailboxMessage>,
    low: VecDeque<MailboxMessage>,
    dropper: Option<DropperFn>,
    priority: Option<PriorityFn>,
    idle_permit: bool,
}

impl PriorityMailbox {
    pub fn new(config: PriorityMailboxConfig) -> Self {
        Self {
            high: VecDeque::new(),
            low: VecDeque::new(),
            dropper: config.dropper,
            priority: config.priority,
            idle_permit: true,
        }
    }

    pub fn factory(config: PriorityMailboxConfig) -> MailboxFactory {
        Arc::new(move || Box::new(Self::new(config.clone())) as Box<dyn Mailbox>)
    }

    fn should_drop(&self, message: &MailboxMessage) -> bool {
        if let Some(dropper) = &self.dropper {
            let view = QueueView {
                high: &self.high,
                low: &self.low,
            };
            dropper(message, view)
        } else {
            false
        }
    }

    fn is_high_priority(&self, message: &MailboxMessage) -> bool {
        if matches!(message, MailboxMessage::System(_)) {
            return true;
        }
        if let Some(priority) = &self.priority {
            return priority(message);
        }
        false
    }
}

impl Mailbox for PriorityMailbox {
    fn enqueue(&mut self, message: MailboxMessage) {
        // Allow idle delivery once after each enqueue operation.
        self.idle_permit = true;

        if let MailboxMessage::User(ref envelope) = message {
            if envelope.is::<Idle>() {
                return;
            }
        }

        if self.should_drop(&message) {
            return;
        }

        if self.is_high_priority(&message) {
            self.high.push_back(message);
        } else {
            self.low.push_back(message);
        }
    }

    fn dequeue(&mut self) -> Option<MailboxMessage> {
        if let Some(message) = self.high.pop_front() {
            return Some(message);
        }
        if let Some(message) = self.low.pop_front() {
            return Some(message);
        }

        if self.idle_permit {
            self.idle_permit = false;
            return Some(MailboxMessage::User(Envelope::new(Idle::instance(), None)));
        }

        None
    }

    fn is_empty(&self) -> bool {
        self.high.is_empty() && self.low.is_empty()
    }
}

/// Utility to create a factory for the default mailbox implementation.
pub fn mailbox_factory_from<M>(factory: M) -> MailboxFactory
where
    M: Fn() -> Box<dyn Mailbox> + Send + Sync + 'static,
{
    Arc::new(factory)
}

/// Utility to create a factory producing priority mailboxes.
pub fn priority_mailbox_factory(config: PriorityMailboxConfig) -> MailboxFactory {
    PriorityMailbox::factory(config)
}

/// Convenience alias for working with cancellable schedule handles.
pub type Cancelable = super::scheduler::ScheduleHandle;
