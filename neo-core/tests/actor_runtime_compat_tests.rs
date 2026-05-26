#![cfg(feature = "runtime")]

use async_trait::async_trait;
use neo_core::akka::{
    Actor, ActorContext, ActorResult, AkkaError, AkkaResult, SupervisorDirective,
};
use std::any::Any;

struct LegacyFacadeActor;

#[async_trait]
impl Actor for LegacyFacadeActor {
    async fn handle(
        &mut self,
        _message: Box<dyn Any + Send>,
        _ctx: &mut ActorContext,
    ) -> ActorResult {
        Err(AkkaError::actor("legacy facade"))
    }

    async fn on_failure(
        &mut self,
        _ctx: &mut ActorContext,
        error: &AkkaError,
    ) -> SupervisorDirective {
        SupervisorDirective::Stop(error.to_string())
    }
}

#[test]
fn legacy_akka_facade_remains_available_to_external_callers() {
    fn assert_actor<T: Actor>() {}

    assert_actor::<LegacyFacadeActor>();

    let legacy_result: AkkaResult<()> = Err(AkkaError::actor("legacy compatibility"));
    assert!(matches!(
        legacy_result,
        Err(AkkaError::Actor(message)) if message.contains("legacy")
    ));
}
