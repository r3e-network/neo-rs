//! Ractor bridge layer for akka compatibility.
//!
//! This module provides the internal implementation that bridges our akka-style
//! Actor trait to ractor's typed actor system. It uses interior mutability
//! (via `tokio::sync::Mutex`) to adapt ractor's `&self` handle signature to
//! our `&mut self` signature.

use super::{
    actor::Actor,
    actor_ref::ActorRef,
    actor_system::ActorSystemInner,
    context::ActorContext,
    error::AkkaError,
    message::{MailboxMessage, SystemMessage, Terminated},
    props::Props,
    supervision::{FailureTracker, SupervisorDirective, SupervisorStrategy},
};
use async_trait::async_trait;
use ractor::{Actor as RactorActor, ActorProcessingErr, ActorRef as RactorActorRef};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::trace;

/// Message type for the ractor bridge.
///
/// Wraps our `MailboxMessage` for ractor's typed message passing.
#[derive(Debug)]
pub(crate) enum BridgeMessage {
    /// A mailbox message (user or system).
    Mailbox(MailboxMessage),
}

/// State for the ractor bridge actor.
///
/// Contains the actual actor instance wrapped in a Mutex for interior mutability,
/// along with supervision and context information.
pub(crate) struct BridgeState {
    /// The wrapped actor, protected by a mutex for interior mutability.
    pub actor: Mutex<Box<dyn Actor>>,
    /// Props for potential actor restart.
    pub props: Props,
    /// Supervisor strategy.
    pub strategy: Option<SupervisorStrategy>,
    /// Failure tracking for supervision.
    pub failures: FailureTracker,
    /// List of actors watching this one.
    pub watchers: Vec<ActorRef>,
    /// Current message sender.
    pub sender: Option<ActorRef>,
    /// Child actors.
    pub children: Vec<ActorRef>,
    /// Parent actor reference.
    pub parent: Option<ActorRef>,
    /// Flag indicating pre_start has been called.
    pub started: bool,
}

/// Arguments for spawning a bridge actor.
pub(crate) struct BridgeArgs {
    pub props: Props,
    pub parent: Option<ActorRef>,
    pub system: Arc<ActorSystemInner>,
    pub self_path: super::actor_system::ActorPath,
}

/// The ractor Actor implementation that bridges to our akka-style Actor.
pub(crate) struct ActorBridge {
    /// Reference to the actor system.
    pub system: Arc<ActorSystemInner>,
    /// The actor's path in the hierarchy.
    pub path: super::actor_system::ActorPath,
}

impl ActorBridge {
    /// Creates a new actor bridge.
    pub fn new(system: Arc<ActorSystemInner>, path: super::actor_system::ActorPath) -> Self {
        Self { system, path }
    }

    /// Creates an ActorContext from the current state.
    fn make_context(
        &self,
        state: &BridgeState,
        self_ref: ActorRef,
    ) -> ActorContext {
        ActorContext {
            system: Arc::clone(&self.system),
            self_ref,
            parent: state.parent.clone(),
            sender: state.sender.clone(),
            children: state.children.clone(),
        }
    }

    /// Handles a failure according to the supervision strategy.
    async fn handle_failure(
        &self,
        error: AkkaError,
        state: &mut BridgeState,
        myself: &RactorActorRef<BridgeMessage>,
        self_ref: &ActorRef,
    ) -> bool {
        let directive = if let Some(ref strategy) = state.strategy {
            strategy.decide(&error, &mut state.failures)
        } else {
            let mut actor = state.actor.lock().await;
            let mut ctx = self.make_context(state, self_ref.clone());
            actor.on_failure(&mut ctx, &error).await
        };

        match directive {
            SupervisorDirective::Stop(_) | SupervisorDirective::Escalate => {
                myself.stop(None);
                false
            }
            SupervisorDirective::Resume => true,
            SupervisorDirective::Restart => {
                // Stop the current actor
                {
                    let mut actor = state.actor.lock().await;
                    let mut ctx = self.make_context(state, self_ref.clone());
                    let _ = actor.post_stop(&mut ctx).await;
                }
                // Create a new actor instance
                let new_actor = state.props.create();
                *state.actor.lock().await = new_actor;
                state.sender = None;
                state.started = false;

                // Run pre_start on the new instance
                let mut actor = state.actor.lock().await;
                let mut ctx = self.make_context(state, self_ref.clone());
                actor.pre_start(&mut ctx).await.is_ok()
            }
        }
    }
}

#[async_trait]
impl RactorActor for ActorBridge {
    type Msg = BridgeMessage;
    type State = BridgeState;
    type Arguments = BridgeArgs;

    async fn pre_start(
        &self,
        myself: RactorActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        let actor = args.props.create();
        let strategy = args.props.strategy.clone();

        let state = BridgeState {
            actor: Mutex::new(actor),
            props: args.props,
            strategy,
            failures: FailureTracker::new(),
            watchers: Vec::new(),
            sender: None,
            children: Vec::new(),
            parent: args.parent,
            started: false,
        };

        // Create the ActorRef for this actor
        let self_ref = ActorRef::from_ractor(
            args.self_path.clone(),
            myself.clone(),
            Arc::downgrade(&args.system),
        );

        // Call pre_start on the wrapped actor
        {
            let mut actor_guard = state.actor.lock().await;
            let mut ctx = ActorContext {
                system: Arc::clone(&args.system),
                self_ref: self_ref.clone(),
                parent: state.parent.clone(),
                sender: None,
                children: Vec::new(),
            };

            if let Err(err) = actor_guard.pre_start(&mut ctx).await {
                trace!(target: "neo", path = %self.path, error = %err, "actor pre_start failed");
                return Err(Box::new(err));
            }
        }

        Ok(BridgeState {
            started: true,
            ..state
        })
    }

    async fn handle(
        &self,
        myself: RactorActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        // Create self_ref for context
        let self_ref = ActorRef::from_ractor(
            self.path.clone(),
            myself.clone(),
            Arc::downgrade(&self.system),
        );

        match message {
            BridgeMessage::Mailbox(mailbox_msg) => {
                match mailbox_msg {
                    MailboxMessage::User(envelope) => {
                        let (msg, sender) = envelope.take();
                        state.sender = sender;

                        let result = {
                            let mut actor = state.actor.lock().await;
                            let mut ctx = self.make_context(state, self_ref.clone());
                            actor.handle(msg, &mut ctx).await
                        };

                        if let Err(err) = result {
                            if !self.handle_failure(err, state, &myself, &self_ref).await {
                                return Ok(());
                            }
                        }
                    }
                    MailboxMessage::System(sys_msg) => {
                        match sys_msg {
                            SystemMessage::Stop => {
                                myself.stop(None);
                            }
                            SystemMessage::Suspend => {
                                // Currently a no-op
                            }
                            SystemMessage::Resume => {
                                // Currently a no-op
                            }
                            SystemMessage::Watch(watcher) => {
                                if watcher != self_ref
                                    && !state.watchers.iter().any(|w| w == &watcher)
                                {
                                    state.watchers.push(watcher);
                                }
                            }
                            SystemMessage::Unwatch(watcher) => {
                                state.watchers.retain(|w| w != &watcher);
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn post_stop(
        &self,
        myself: RactorActorRef<Self::Msg>,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        // Create self_ref for cleanup
        let self_ref = ActorRef::from_ractor(
            self.path.clone(),
            myself,
            Arc::downgrade(&self.system),
        );

        // Call post_stop on the wrapped actor
        {
            let mut actor = state.actor.lock().await;
            let mut ctx = self.make_context(state, self_ref.clone());
            let _ = actor.post_stop(&mut ctx).await;
        }

        // Stop all children
        for child in state.children.drain(..) {
            let _ = child.stop();
        }

        // Unregister from the system
        self.system.unregister(&self.path);

        // Notify watchers
        let terminated = Terminated::new(self_ref.clone());
        for watcher in state.watchers.drain(..) {
            let _ = watcher.tell_from(terminated.clone(), Some(self_ref.clone()));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_message_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<BridgeMessage>();
    }
}
