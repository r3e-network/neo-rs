use super::{
    actor::{Actor, ActorResult},
    actor_ref::ActorRef,
    actor_system::ActorSystem,
    context::ActorContext,
    error::{AkkaError, AkkaResult},
    props::Props,
};
use async_trait::async_trait;
use std::{any::Any, sync::Arc, time::Duration};
use tokio::sync::{Mutex, mpsc};
use tokio::time;
use uuid::Uuid;

/// Default inbox capacity for bounded channels.
const INBOX_CAPACITY: usize = 1000;

struct InboxActor {
    mailbox: mpsc::Sender<Box<dyn Any + Send>>,
}

#[async_trait]
impl Actor for InboxActor {
    async fn handle(
        &mut self,
        message: Box<dyn Any + Send>,
        _ctx: &mut ActorContext,
    ) -> ActorResult {
        // Use try_send to avoid blocking when the inbox is full
        self.mailbox
            .try_send(message)
            .map_err(|err| AkkaError::send(format!("{}", err)))?;
        Ok(())
    }
}

pub struct Inbox {
    actor: ActorRef,
    receiver: Arc<Mutex<mpsc::Receiver<Box<dyn Any + Send>>>>,
}

impl Inbox {
    pub fn create(system: &ActorSystem) -> AkkaResult<Self> {
        let (tx, rx) = mpsc::channel(INBOX_CAPACITY);
        let props = Props::new({
            let tx = tx.clone();
            move || InboxActor {
                mailbox: tx.clone(),
            }
        });

        let name = format!("$inbox-{}", Uuid::new_v4().simple());
        let actor = system.actor_of(props, name)?;

        Ok(Self {
            actor,
            receiver: Arc::new(Mutex::new(rx)),
        })
    }

    pub fn actor_ref(&self) -> ActorRef {
        self.actor.clone()
    }

    pub fn watch(&self, actor: &ActorRef) -> AkkaResult<()> {
        actor.watch(self.actor.clone())
    }

    pub fn send<M>(&self, actor: &ActorRef, message: M) -> AkkaResult<()>
    where
        M: Any + Send + 'static,
    {
        actor.tell_from(message, Some(self.actor.clone()))
    }

    pub async fn receive(&self, timeout: Duration) -> AkkaResult<Box<dyn Any + Send>> {
        let mut receiver = self.receiver.lock().await;
        match time::timeout(timeout, receiver.recv()).await {
            Ok(Some(message)) => Ok(message),
            Ok(None) => Err(AkkaError::system("inbox closed")),
            Err(_) => Err(AkkaError::AskTimeout),
        }
    }
}

impl Drop for Inbox {
    fn drop(&mut self) {
        let _ = self.actor.stop();
    }
}
