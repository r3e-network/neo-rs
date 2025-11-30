//! Actor execution context for the Neo actor system.
//!
//! This module provides the `ActorContext` type which gives actors access to
//! the actor system, their own reference, parent/child relationships, and
//! scheduling capabilities.

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
///
/// The `ActorContext` provides actors with access to:
/// - Their own `ActorRef` via `self_ref()`
/// - The sender of the current message via `sender()`
/// - Parent and child actor references
/// - The actor system for spawning new actors
/// - Scheduling capabilities for delayed/repeated messages
///
/// # Example
///
/// ```rust,ignore
/// impl Actor for MyActor {
///     fn receive(&mut self, ctx: &mut ActorContext, msg: Box<dyn Any + Send>) {
///         // Access self reference
///         let self_ref = ctx.self_ref();
///
///         // Spawn a child actor
///         let child = ctx.actor_of(Props::new(|| Box::new(ChildActor)), "child")?;
///
///         // Schedule a delayed message
///         ctx.schedule_once(Duration::from_secs(5), &self_ref, "delayed_msg");
///     }
/// }
/// ```
pub struct ActorContext {
    pub(crate) system: Arc<ActorSystemInner>,
    pub(crate) self_ref: ActorRef,
    pub(crate) parent: Option<ActorRef>,
    pub(crate) sender: Option<ActorRef>,
    pub(crate) children: Vec<ActorRef>,
}

impl ActorContext {
    /// Returns a reference to this actor.
    ///
    /// This can be used to send messages to self or to pass to other actors.
    #[inline]
    pub fn self_ref(&self) -> ActorRef {
        self.self_ref.clone()
    }

    /// Returns the sender of the current message, if available.
    ///
    /// Returns `None` if the message was sent without a sender reference
    /// (e.g., from outside the actor system).
    #[inline]
    pub fn sender(&self) -> Option<ActorRef> {
        self.sender.clone()
    }

    /// Returns a reference to this actor's parent, if it has one.
    ///
    /// Top-level actors spawned directly on the system have no parent.
    #[inline]
    pub fn parent(&self) -> Option<ActorRef> {
        self.parent.clone()
    }

    /// Returns a slice of this actor's child actors.
    #[inline]
    pub fn children(&self) -> &[ActorRef] {
        &self.children
    }

    /// Creates a new child actor with the given props and name.
    ///
    /// # Arguments
    ///
    /// * `props` - The props defining how to create the actor
    /// * `name` - A unique name for the child actor within this parent
    ///
    /// # Errors
    ///
    /// Returns an error if the actor could not be created (e.g., duplicate name).
    pub fn actor_of(&mut self, props: Props, name: impl Into<String>) -> AkkaResult<ActorRef> {
        let name = name.into();
        let actor = self
            .system
            .spawn_child(self.self_ref.clone(), props, Some(name))?;
        self.children.push(actor.clone());
        Ok(actor)
    }

    /// Stops the specified actor.
    ///
    /// # Arguments
    ///
    /// * `actor` - The actor to stop
    pub fn stop(&self, actor: &ActorRef) -> AkkaResult<()> {
        actor.stop()
    }

    /// Stops this actor.
    pub fn stop_self(&self) -> AkkaResult<()> {
        self.self_ref.stop()
    }

    /// Registers this actor to watch another actor for termination.
    ///
    /// When the watched actor terminates, this actor will receive a
    /// `Terminated` message.
    ///
    /// # Arguments
    ///
    /// * `actor` - The actor to watch
    pub fn watch(&self, actor: &ActorRef) -> AkkaResult<()> {
        actor.watch(self.self_ref.clone())
    }

    /// Unregisters this actor from watching another actor.
    ///
    /// # Arguments
    ///
    /// * `actor` - The actor to stop watching
    pub fn unwatch(&self, actor: &ActorRef) -> AkkaResult<()> {
        actor.unwatch(self.self_ref.clone())
    }

    /// Returns a handle to the actor system.
    #[inline]
    pub fn system(&self) -> ActorSystemHandle {
        ActorSystemHandle::new(self.system.clone())
    }

    /// Returns the scheduler for scheduling delayed/repeated messages.
    #[inline]
    pub fn scheduler(&self) -> Scheduler {
        self.system().scheduler()
    }

    /// Schedules a message to be sent once after a delay.
    ///
    /// # Arguments
    ///
    /// * `delay` - How long to wait before sending the message
    /// * `target` - The actor to send the message to
    /// * `message` - The message to send
    ///
    /// # Returns
    ///
    /// A `Cancelable` that can be used to cancel the scheduled message.
    pub fn schedule_once<M>(&self, delay: Duration, target: &ActorRef, message: M) -> Cancelable
    where
        M: Send + 'static + std::any::Any,
    {
        self.scheduler()
            .schedule_tell_once(delay, target.clone(), message, None)
    }

    /// Schedules a message to be sent repeatedly at a fixed interval.
    ///
    /// # Arguments
    ///
    /// * `initial_delay` - How long to wait before the first message
    /// * `interval` - The interval between subsequent messages
    /// * `target` - The actor to send messages to
    /// * `message` - The message to send (will be cloned for each send)
    ///
    /// # Returns
    ///
    /// A `Cancelable` that can be used to cancel the scheduled messages.
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

    /// Schedules a message to be sent once after a delay with a custom sender.
    ///
    /// # Arguments
    ///
    /// * `delay` - How long to wait before sending the message
    /// * `target` - The actor to send the message to
    /// * `message` - The message to send
    /// * `sender` - The sender to appear as (or `None` for no sender)
    ///
    /// # Returns
    ///
    /// A `Cancelable` that can be used to cancel the scheduled message.
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

    /// Schedules a message to be sent repeatedly with a custom sender.
    ///
    /// # Arguments
    ///
    /// * `initial_delay` - How long to wait before the first message
    /// * `interval` - The interval between subsequent messages
    /// * `target` - The actor to send messages to
    /// * `message` - The message to send (will be cloned for each send)
    /// * `sender` - The sender to appear as (or `None` for no sender)
    ///
    /// # Returns
    ///
    /// A `Cancelable` that can be used to cancel the scheduled messages.
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
