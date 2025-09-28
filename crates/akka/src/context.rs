use crate::{
    actor_ref::ActorRef,
    actor_system::{ActorSystemHandle, ActorSystemInner},
    error::AkkaResult,
    mailbox::Cancelable,
    props::Props,
    scheduler::Scheduler,
};
use std::{sync::Arc, time::Duration};

/// Execution context available to actors while processing messages.
pub struct ActorContext {
    pub(crate) system: Arc<ActorSystemInner>,
    pub(crate) self_ref: ActorRef,
    pub(crate) parent: Option<ActorRef>,
    pub(crate) sender: Option<ActorRef>,
    pub(crate) children: Vec<ActorRef>,
}

impl ActorContext {
    pub fn self_ref(&self) -> ActorRef {
        self.self_ref.clone()
    }

    pub fn sender(&self) -> Option<ActorRef> {
        self.sender.clone()
    }

    pub fn parent(&self) -> Option<ActorRef> {
        self.parent.clone()
    }

    pub fn children(&self) -> &[ActorRef] {
        &self.children
    }

    pub fn actor_of(&mut self, props: Props, name: impl Into<String>) -> AkkaResult<ActorRef> {
        let name = name.into();
        let actor = self
            .system
            .spawn_child(self.self_ref.clone(), props, Some(name))?;
        self.children.push(actor.clone());
        Ok(actor)
    }

    pub fn stop(&self, actor: &ActorRef) -> AkkaResult<()> {
        actor.stop()
    }

    pub fn stop_self(&self) -> AkkaResult<()> {
        self.self_ref.stop()
    }

    pub fn watch(&self, actor: &ActorRef) -> AkkaResult<()> {
        actor.watch(self.self_ref.clone())
    }

    pub fn unwatch(&self, actor: &ActorRef) -> AkkaResult<()> {
        actor.unwatch(self.self_ref.clone())
    }

    pub fn system(&self) -> ActorSystemHandle {
        ActorSystemHandle::new(self.system.clone())
    }

    pub fn scheduler(&self) -> Scheduler {
        self.system().scheduler()
    }

    pub fn schedule_once<M>(&self, delay: Duration, target: &ActorRef, message: M) -> Cancelable
    where
        M: Send + 'static + std::any::Any,
    {
        self.scheduler()
            .schedule_tell_once(delay, target.clone(), message, None)
    }

    pub fn schedule_repeatedly<M>(
        &self,
        initial_delay: Duration,
        interval: Duration,
        target: &ActorRef,
        message: M,
    ) -> Cancelable
    where
        M: Clone + Send + 'static + std::any::Any,
    {
        self.scheduler().schedule_tell_repeatedly(
            initial_delay,
            interval,
            target.clone(),
            message,
            None,
        )
    }

    pub fn schedule_tell_once_cancelable<M>(
        &self,
        delay: Duration,
        target: &ActorRef,
        message: M,
        sender: Option<ActorRef>,
    ) -> Cancelable
    where
        M: Send + 'static + std::any::Any,
    {
        self.scheduler()
            .schedule_tell_once(delay, target.clone(), message, sender)
    }

    pub fn schedule_tell_repeatedly_cancelable<M>(
        &self,
        initial_delay: Duration,
        interval: Duration,
        target: &ActorRef,
        message: M,
        sender: Option<ActorRef>,
    ) -> Cancelable
    where
        M: Clone + Send + 'static + std::any::Any,
    {
        self.scheduler().schedule_tell_repeatedly(
            initial_delay,
            interval,
            target.clone(),
            message,
            sender,
        )
    }
}
