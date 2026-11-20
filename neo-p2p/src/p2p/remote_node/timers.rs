//! Timer scheduling and periodic upkeep for `RemoteNode` (pings and cache pruning).
use super::RemoteNode;
use super::pending_known_hashes::PENDING_KNOWN_HASH_TTL;
use crate::p2p::ProtocolMessage;
use crate::p2p::messages::NetworkMessage;
use crate::p2p::payloads::ping_payload::PingPayload;
use std::time::{Duration, Instant};
use tracing::trace;

const TIMER_INTERVAL: Duration = Duration::from_secs(30);
const PING_INTERVAL: Duration = Duration::from_secs(60);

impl RemoteNode {
    pub(super) fn ensure_timer(&mut self, ctx: &mut neo_actors::ActorContext) {
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

    pub(super) async fn on_timer(
        &mut self,
        ctx: &mut neo_actors::ActorContext,
    ) -> neo_actors::ActorResult {
        let cutoff = Instant::now()
            .checked_sub(PENDING_KNOWN_HASH_TTL)
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

        self.log_outbound_batch_snapshot();
        self.ensure_timer(ctx);
        Ok(())
    }

    /// Emits one bounded telemetry line per peer describing outbound batching.
    ///
    /// The snapshot is emitted from the periodic timer rather than per write so
    /// the log volume stays proportional to the peer count, not the traffic.
    fn log_outbound_batch_snapshot(&self) {
        let stats = self.outbound_batch_stats();
        if stats.messages_written == 0 {
            return;
        }
        tracing::debug!(
            target: "neo",
            endpoint = %self.endpoint,
            batches = stats.batches,
            batch_messages = stats.batch_messages,
            batch_bytes = stats.batch_bytes,
            timer_flushes = stats.timer_flushes,
            threshold_flushes = stats.threshold_flushes,
            urgent_flushes = stats.urgent_flushes,
            shutdown_flushes = stats.shutdown_flushes,
            immediate_writes = stats.immediate_writes,
            write_calls = stats.write_calls,
            messages_written = stats.messages_written,
            write_calls_saved = stats.write_calls_saved,
            "outbound batching telemetry snapshot"
        );
    }
}
