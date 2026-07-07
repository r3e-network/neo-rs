//! Blockchain service command-loop scheduling.
//!
//! The parent module owns construction and command dispatch. This module owns
//! the Tokio loop policy: wait for the first command, drain a bounded burst
//! without yielding, then yield periodically so network and consensus tasks keep
//! making progress during catch-up.

use super::{BlockchainService, MempoolLike};

const MAX_DRAIN_PER_BATCH: u32 = 128;

impl<S, M> BlockchainService<S, M>
where
    S: crate::service_context::SystemContext,
    M: MempoolLike,
{
    /// Drive the service loop until the command channel is closed.
    ///
    /// Every command is dispatched to a synchronous handler method on the
    /// service struct; the loop itself is just
    /// `while let Some(cmd) = self.cmd_rx.recv().await`, expressed as a normal
    /// `async fn` over typed channels.
    ///
    /// After processing the first command, drains pending commands without
    /// awaiting between them. This keeps the pipeline full during catch-up
    /// bursts, while `MAX_DRAIN_PER_BATCH` prevents starving network I/O,
    /// consensus ticks, and other services.
    pub async fn run(mut self) {
        tracing::debug!(target: "neo", "blockchain service run loop started");
        while let Some(cmd) = self.cmd_rx.recv().await {
            self.dispatch(cmd).await;

            let mut drained = 0u32;
            while let Ok(cmd) = self.cmd_rx.try_recv() {
                self.dispatch(cmd).await;
                drained += 1;
                if drained >= MAX_DRAIN_PER_BATCH {
                    tokio::task::yield_now().await;
                    drained = 0;
                }
            }
        }
        tracing::debug!(target: "neo", "blockchain service run loop exited");
    }
}
