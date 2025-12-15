use super::actor_ref::ActorRef;
use std::any::Any;

/// Envelope used for delivering user messages to an actor.
#[derive(Debug)]
pub struct Envelope {
    pub(crate) message: Box<dyn Any + Send>,
    pub sender: Option<ActorRef>,
}

impl Envelope {
    pub fn new<M>(message: M, sender: Option<ActorRef>) -> Self
    where
        M: Any + Send + 'static,
    {
        Self {
            message: Box::new(message),
            sender,
        }
    }

    pub fn take(self) -> (Box<dyn Any + Send>, Option<ActorRef>) {
        (self.message, self.sender)
    }

    pub fn message_ref(&self) -> &(dyn Any + Send) {
        self.message.as_ref()
    }

    pub fn is<T: Any>(&self) -> bool {
        self.message_ref().is::<T>()
    }

    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.message_ref().downcast_ref::<T>()
    }
}

/// Control messages that are consumed by the actor system itself.
#[derive(Debug)]
pub enum SystemMessage {
    Stop,
    Suspend,
    Resume,
    Watch(ActorRef),
    Unwatch(ActorRef),
}

/// Terminal notification delivered to watchers when an actor stops.
#[derive(Debug, Clone)]
pub struct Terminated {
    pub actor: ActorRef,
}

impl Terminated {
    pub fn new(actor: ActorRef) -> Self {
        Self { actor }
    }
}

/// Idle signal used by priority mailboxes to notify actors about an empty queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Idle;

impl Idle {
    pub fn instance() -> Self {
        Idle
    }
}

/// Messages flowing through an actor mailbox.
#[derive(Debug)]
pub enum MailboxMessage {
    User(Envelope),
    System(SystemMessage),
}

impl MailboxMessage {
    pub fn as_user(&self) -> Option<&Envelope> {
        match self {
            MailboxMessage::User(envelope) => Some(envelope),
            _ => None,
        }
    }

    pub fn as_system(&self) -> Option<&SystemMessage> {
        match self {
            MailboxMessage::System(msg) => Some(msg),
            _ => None,
        }
    }
}
