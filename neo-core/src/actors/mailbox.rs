use super::message::MailboxMessage;
use std::collections::VecDeque;

/// Interface implemented by all mailbox implementations.
pub trait Mailbox: Send {
    fn enqueue(&mut self, message: MailboxMessage);
    fn dequeue(&mut self) -> Option<MailboxMessage>;
    fn is_empty(&self) -> bool;
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

/// Convenience alias for working with cancellable schedule handles.
pub type Cancelable = super::scheduler::ScheduleHandle;
