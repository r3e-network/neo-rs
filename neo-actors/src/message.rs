use super::actor_ref::ActorRef;
use std::any::Any;

/// Envelope used for delivering user messages to an actor.
#[derive(Debug)]
pub struct Envelope {
    pub(crate) message: Box<dyn Any + Send>,
    /// Sender recorded for `tell_from`; `None` for anonymous sends.
    pub sender: Option<ActorRef>,
}

impl Envelope {
    /// Wraps `message` with its optional `sender`.
    pub fn new<M>(message: M, sender: Option<ActorRef>) -> Self
    where
        M: Any + Send + 'static,
    {
        Self {
            message: Box::new(message),
            sender,
        }
    }

    /// Consumes the envelope into its boxed message and sender.
    pub fn take(self) -> (Box<dyn Any + Send>, Option<ActorRef>) {
        (self.message, self.sender)
    }

    /// Borrows the boxed message as a trait object.
    pub fn message_ref(&self) -> &(dyn Any + Send) {
        self.message.as_ref()
    }

    /// Returns true when the boxed message is of type `T`.
    pub fn is<T: Any>(&self) -> bool {
        self.message_ref().is::<T>()
    }

    /// Downcasts the boxed message to `T` if it is one.
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.message_ref().downcast_ref::<T>()
    }
}

/// Control messages that are consumed by the actor system itself.
#[derive(Debug)]
pub enum SystemMessage {
    /// Stop the target actor.
    Stop,
    /// Start watching `ActorRef`; a `Terminated` follows if it stops.
    Watch(ActorRef),
    /// Stop watching `ActorRef`.
    Unwatch(ActorRef),
}

/// Terminal notification delivered to watchers when an actor stops.
#[derive(Debug, Clone)]
pub struct Terminated {
    /// The actor that stopped.
    pub actor: ActorRef,
}

impl Terminated {
    /// Builds a termination notice for `actor`.
    pub fn new(actor: ActorRef) -> Self {
        Self { actor }
    }
}

/// Messages flowing through an actor mailbox.
#[derive(Debug)]
pub enum MailboxMessage {
    /// A user message envelope.
    User(Envelope),
    /// A runtime control message.
    System(SystemMessage),
}

impl MailboxMessage {
    /// Returns the user envelope if this mailbox message is one.
    pub fn as_user(&self) -> Option<&Envelope> {
        match self {
            MailboxMessage::User(envelope) => Some(envelope),
            _ => None,
        }
    }

    /// Returns the system message if this mailbox message is one.
    pub fn as_system(&self) -> Option<&SystemMessage> {
        match self {
            MailboxMessage::System(msg) => Some(msg),
            _ => None,
        }
    }
}
