use super::{
    actor_system::{ActorPath, ActorSystemInner, MailboxCommand},
    error::{AkkaError, AkkaResult},
    message::{Envelope, MailboxMessage, SystemMessage},
};
use std::{any::Any, fmt, sync::Weak};
use tokio::sync::mpsc;
use uuid::Uuid;

/// Addressable reference to an actor instance.
#[derive(Clone)]
pub struct ActorRef {
    pub(crate) path: ActorPath,
    mailbox: mpsc::Sender<MailboxCommand>,
    pub(crate) system: Weak<ActorSystemInner>,
}

impl ActorRef {
    /// Creates a new ActorRef with a mailbox channel.
    pub(crate) fn new(
        path: ActorPath,
        mailbox: mpsc::Sender<MailboxCommand>,
        system: Weak<ActorSystemInner>,
    ) -> Self {
        Self {
            path,
            mailbox,
            system,
        }
    }

    /// Sends a message to the actor without specifying a sender.
    pub fn tell<M>(&self, message: M) -> AkkaResult<()>
    where
        M: Any + Send + 'static,
    {
        self.tell_from(message, None)
    }

    /// Sends a message to the actor without specifying a sender, awaiting
    /// mailbox capacity when necessary.
    pub async fn tell_async<M>(&self, message: M) -> AkkaResult<()>
    where
        M: Any + Send + 'static,
    {
        self.tell_from_async(message, None).await
    }

    /// Sends a message to the actor specifying the sender.
    /// Uses try_send to avoid blocking when the mailbox is full.
    pub fn tell_from<M>(&self, message: M, sender: Option<ActorRef>) -> AkkaResult<()>
    where
        M: Any + Send + 'static,
    {
        let envelope = Envelope::new(message, sender);
        self.mailbox
            .try_send(MailboxCommand::Message(MailboxMessage::User(envelope)))
            .map_err(|e| AkkaError::send(format!("{}", e)))
    }

    /// Sends a message to the actor specifying the sender.
    /// Uses a backpressured send so callers can avoid dropping critical messages
    /// under load.
    pub async fn tell_from_async<M>(&self, message: M, sender: Option<ActorRef>) -> AkkaResult<()>
    where
        M: Any + Send + 'static,
    {
        self.mailbox
            .send(MailboxCommand::Message(MailboxMessage::User(Envelope::new(
                message, sender,
            ))))
            .await
            .map_err(|e| AkkaError::send(format!("{}", e)))
    }

    /// Registers `watcher` to receive a [`Terminated`](super::Terminated) message when this actor stops.
    pub fn watch(&self, watcher: ActorRef) -> AkkaResult<()> {
        self.mailbox
            .try_send(MailboxCommand::Message(MailboxMessage::System(
                SystemMessage::Watch(watcher),
            )))
            .map_err(|e| AkkaError::send(format!("{}", e)))
    }

    /// Removes `watcher` from the current actor's watch list.
    pub fn unwatch(&self, watcher: ActorRef) -> AkkaResult<()> {
        self.mailbox
            .try_send(MailboxCommand::Message(MailboxMessage::System(
                SystemMessage::Unwatch(watcher),
            )))
            .map_err(|e| AkkaError::send(format!("{}", e)))
    }

    /// Sends a message that expects a response.
    /// The `builder` closure is responsible for embedding the reply channel
    /// into a message type understood by the receiving actor.
    #[cfg(test)]
    pub async fn ask<R, F>(&self, builder: F, timeout: std::time::Duration) -> AkkaResult<R>
    where
        R: Send + 'static,
        F: FnOnce(tokio::sync::oneshot::Sender<R>) -> Box<dyn Any + Send>,
    {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        let message = builder(reply_tx);

        self.mailbox
            .try_send(MailboxCommand::Message(MailboxMessage::User(Envelope {
                message,
                sender: None,
            })))
            .map_err(|e| AkkaError::send(format!("{}", e)))?;

        match tokio::time::timeout(timeout, reply_rx).await {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(_)) => Err(AkkaError::AskTimeout),
            Err(_) => Err(AkkaError::AskTimeout),
        }
    }

    /// Commands the actor to stop. This is asynchronous and returns immediately.
    pub fn stop(&self) -> AkkaResult<()> {
        self.mailbox
            .try_send(MailboxCommand::Message(MailboxMessage::System(
                SystemMessage::Stop,
            )))
            .map_err(|e| AkkaError::send(format!("{}", e)))
    }

    pub fn path(&self) -> ActorPath {
        self.path.clone()
    }

    pub fn is_alive(&self) -> bool {
        self.system.upgrade().is_some()
    }

    pub(crate) fn unique_child_name() -> String {
        format!("$anon-{}", Uuid::new_v4().simple())
    }
}

impl fmt::Debug for ActorRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActorRef")
            .field("path", &self.path)
            .finish()
    }
}

impl PartialEq for ActorRef {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}

impl Eq for ActorRef {}

impl fmt::Display for ActorRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.path)
    }
}
