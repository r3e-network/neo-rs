use crate::{
    context::ActorContext,
    error::{AkkaError, AkkaResult},
    supervision::SupervisorDirective,
};
use async_trait::async_trait;
use std::any::Any;

/// Result type alias used by actor lifecycle callbacks.
pub type ActorResult = AkkaResult<()>;

/// Core trait that must be implemented by all actors.
#[async_trait]
pub trait Actor: Send + 'static {
    /// Invoked after the actor is started and before the first message is processed.
    async fn pre_start(&mut self, _ctx: &mut ActorContext) -> ActorResult {
        Ok(())
    }

    /// Handles a single incoming message.
    async fn handle(&mut self, message: Box<dyn Any + Send>, ctx: &mut ActorContext)
        -> ActorResult;

    /// Invoked before the actor is permanently stopped.
    async fn post_stop(&mut self, _ctx: &mut ActorContext) -> ActorResult {
        Ok(())
    }

    /// Called when message processing results in an error.
    async fn on_failure(
        &mut self,
        _ctx: &mut ActorContext,
        error: &AkkaError,
    ) -> SupervisorDirective {
        SupervisorDirective::Stop(error.to_string())
    }
}
