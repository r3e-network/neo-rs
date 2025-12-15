//! Timer scheduling and periodic upkeep for `RemoteNode` (pings and cache pruning).
use super::RemoteNode;
use crate::network::p2p::messages::NetworkMessage;
use crate::network::p2p::payloads::ping_payload::PingPayload;
use crate::network::p2p::ProtocolMessage;
use std::time::{Duration, Instant};
use tracing::trace;

const TIMER_INTERVAL: Duration = Duration::from_secs(30);
const PENDING_HASH_TTL: Duration = Duration::from_secs(60);
const PING_INTERVAL: Duration = Duration::from_secs(60);

impl RemoteNode {
    pub(super) fn ensure_timer(&mut self, ctx: &mut crate::akka::ActorContext) {
        if self.timer.is_some() {
            return;
        }

        let handle = ctx.schedule_tell_repeatedly_cancelable(
            TIMER_INTERVAL,
            TIMER_INTERVAL,
            &ctx.self_ref(),
            super::RemoteNodeCommand::TimerTick,
            None,
        );
        self.timer = Some(handle);
    }

    pub(super) fn cancel_timer(&mut self) {
        if let Some(timer) = self.timer.take() {
            timer.cancel();
        }
    }

    pub(super) async fn on_timer(&mut self, ctx: &mut crate::akka::ActorContext) -> crate::akka::ActorResult {
        let cutoff = Instant::now()
            .checked_sub(PENDING_HASH_TTL)
            .unwrap_or_else(Instant::now);
        let removed = self.pending_known_hashes.prune_older_than(cutoff);
        if removed > 0 {
            trace!(target: "neo", removed, "expired pending known hashes removed");
        }

        if self.handshake_complete && self.last_sent.elapsed() >= PING_INTERVAL {
            let payload = PingPayload::create(self.current_local_block_index());
            self.enqueue_message(NetworkMessage::new(ProtocolMessage::Ping(payload)))
                .await?;
        }

        self.ensure_timer(ctx);
        Ok(())
    }
}
