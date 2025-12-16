//! Consensus scheduler for the runtime.
//!
//! Neo N3 starts dBFT for each new block height once the node is synchronized.
//! The Rust node uses a refactored tokio-based architecture, so we run a small
//! scheduler loop that:
//! - starts consensus for `tip_height + 1` when not running
//! - calls `on_timer_tick` periodically for timeouts/view changes

use neo_chain::ChainState;
use neo_consensus::ConsensusService;
use neo_core::UInt256;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, info, warn};

pub async fn run_consensus_scheduler(
    consensus: Arc<RwLock<Option<ConsensusService>>>,
    chain: Arc<RwLock<ChainState>>,
    mut shutdown_rx: broadcast::Receiver<()>,
) {
    let mut ticker = tokio::time::interval(std::time::Duration::from_millis(500));

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                let now = current_timestamp_ms();

                let (next_height, prev_hash) = {
                    let chain_guard = chain.read().await;
                    let height = chain_guard.height();
                    let prev_hash = chain_guard.current_hash().unwrap_or_else(UInt256::zero);
                    (height.saturating_add(1), prev_hash)
                };

                let mut guard = consensus.write().await;
                let Some(ref mut service) = *guard else {
                    continue;
                };

                if !service.is_running() {
                    // Neo N3 block header version is always 0.
                    let block_version = 0;
                    if let Err(e) = service.start(next_height, block_version, prev_hash, now) {
                        debug!(target: "neo::runtime", error = %e, next_height, "failed to start consensus");
                    } else {
                        info!(target: "neo::runtime", next_height, "consensus started");
                    }
                }

                if let Err(e) = service.on_timer_tick(now) {
                    warn!(target: "neo::runtime", error = %e, "consensus timer tick failed");
                }
            }
            _ = shutdown_rx.recv() => {
                info!(target: "neo::runtime", "consensus scheduler shutting down");
                break;
            }
        }
    }
}

fn current_timestamp_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
