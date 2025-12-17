//
// actor_impl.rs - Actor trait implementation for LocalNodeActor
//

use super::actor::LocalNodeActor;
use super::*;

#[async_trait]
impl Actor for LocalNodeActor {
    async fn handle(
        &mut self,
        envelope: Box<dyn Any + Send>,
        ctx: &mut ActorContext,
    ) -> ActorResult {
        match envelope.downcast::<PeerCommand>() {
            Ok(command) => self.handle_peer_command(*command, ctx).await,
            Err(payload) => match payload.downcast::<LocalNodeCommand>() {
                Ok(command) => self.handle_local_command(*command, ctx).await,
                Err(payload) => match payload.downcast::<PeerTimer>() {
                    Ok(_) => self.handle_peer_timer(ctx).await,
                    Err(payload) => match payload.downcast::<Terminated>() {
                        Ok(terminated) => self.handle_terminated(terminated.actor, ctx).await,
                        Err(payload) => {
                            warn!(
                                target: "neo",
                                message_type_id = ?payload.as_ref().type_id(),
                                "unknown message routed to local node actor"
                            );
                            Ok(())
                        }
                    },
                },
            },
        }
    }

    async fn post_stop(&mut self, _ctx: &mut ActorContext) -> ActorResult {
        self.peer.cancel_timer();
        if let Some(handle) = self.listener.take() {
            handle.abort();
        }
        Ok(())
    }
}
