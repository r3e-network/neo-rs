use super::message::MailboxMessage;
use std::collections::VecDeque;

/// Default FIFO mailbox that prioritises system messages.
#[derive(Default)]
pub(crate) struct DefaultMailbox {
    queue: VecDeque<MailboxMessage>,
}

impl DefaultMailbox {
    pub(crate) fn enqueue(&mut self, message: MailboxMessage) {
        match message {
            MailboxMessage::System(_) => self.queue.push_front(message),
            _ => self.queue.push_back(message),
        }
    }

    pub(crate) fn dequeue(&mut self) -> Option<MailboxMessage> {
        self.queue.pop_front()
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}
