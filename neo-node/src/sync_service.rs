//! Block Sync Service for neo-node runtime
//!
//! This module provides block synchronization logic that coordinates
//! with P2P peers to download and apply blocks to the local chain.

use neo_chain::{ChainEvent, ChainState};
use neo_core::UInt256;
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, info};

/// Sync service state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncState {
    /// Not syncing
    Idle,
    /// Syncing headers
    SyncingHeaders,
    /// Syncing blocks
    SyncingBlocks,
    /// Fully synced
    Synced,
}

/// Sync statistics
#[derive(Debug, Clone, Default)]
pub struct SyncStats {
    /// Current local height
    pub local_height: u32,
    /// Best known height from peers
    pub best_height: u32,
    /// Number of blocks downloaded
    pub blocks_downloaded: u64,
    /// Number of headers downloaded
    pub headers_downloaded: u64,
    /// Sync progress (0.0 - 1.0)
    pub progress: f64,
}

/// Pending block request
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields will be used when full sync service is implemented
struct PendingRequest {
    /// Height (if known)
    height: Option<u32>,
    /// Request timestamp
    _requested_at: u64,
    /// Peer that should respond
    _peer: SocketAddr,
}

/// Block data in queue
type BlockData = (u32, UInt256, Vec<u8>);

/// Block Sync Service
pub struct SyncService {
    /// Current sync state
    state: Arc<RwLock<SyncState>>,
    /// Sync statistics
    stats: Arc<RwLock<SyncStats>>,
    /// Chain state reference
    chain: Arc<RwLock<ChainState>>,
    /// Pending block requests
    pending_requests: Arc<RwLock<HashMap<UInt256, PendingRequest>>>,
    /// Block queue (blocks waiting to be applied)
    block_queue: Arc<RwLock<VecDeque<BlockData>>>,
    /// Known peer heights
    peer_heights: Arc<RwLock<HashMap<SocketAddr, u32>>>,
    /// Chain event sender
    chain_tx: broadcast::Sender<ChainEvent>,
    /// Shutdown signal
    _shutdown_tx: broadcast::Sender<()>,
}

impl SyncService {
    /// Creates a new sync service
    pub fn new(chain: Arc<RwLock<ChainState>>, chain_tx: broadcast::Sender<ChainEvent>) -> Self {
        let (shutdown_tx, _) = broadcast::channel(8);

        Self {
            state: Arc::new(RwLock::new(SyncState::Idle)),
            stats: Arc::new(RwLock::new(SyncStats::default())),
            chain,
            pending_requests: Arc::new(RwLock::new(HashMap::new())),
            block_queue: Arc::new(RwLock::new(VecDeque::new())),
            peer_heights: Arc::new(RwLock::new(HashMap::new())),
            chain_tx,
            _shutdown_tx: shutdown_tx,
        }
    }

    /// Returns the current sync state
    pub async fn state(&self) -> SyncState {
        *self.state.read().await
    }

    /// Returns sync statistics
    pub async fn stats(&self) -> SyncStats {
        self.stats.read().await.clone()
    }

    /// Updates peer height information
    pub async fn update_peer_height(&self, peer: SocketAddr, height: u32) {
        self.peer_heights.write().await.insert(peer, height);

        // Update best known height
        let best = *self.peer_heights.read().await.values().max().unwrap_or(&0);
        self.stats.write().await.best_height = best;

        // Check if we need to sync
        let local = self.chain.read().await.height();
        if best > local + 1 {
            self.maybe_start_sync().await;
        }
    }

    /// Removes a peer from tracking
    pub async fn remove_peer(&self, peer: &SocketAddr) {
        self.peer_heights.write().await.remove(peer);
    }

    /// Handles a received block
    pub async fn on_block_received(
        &self,
        hash: UInt256,
        height: u32,
        data: Vec<u8>,
        from: SocketAddr,
    ) {
        debug!(
            target: "neo::sync",
            hash = %hash,
            height,
            from = %from,
            "block received"
        );

        // Remove from pending requests
        self.pending_requests.write().await.remove(&hash);

        // Add to block queue
        self.block_queue
            .write()
            .await
            .push_back((height, hash, data));

        // Update stats
        self.stats.write().await.blocks_downloaded += 1;

        // Try to apply queued blocks
        self.apply_queued_blocks().await;
    }

    /// Handles received headers
    pub async fn on_headers_received(&self, count: usize, _from: SocketAddr) {
        self.stats.write().await.headers_downloaded += count as u64;
    }

    /// Starts sync if needed
    async fn maybe_start_sync(&self) {
        let current_state = *self.state.read().await;
        if current_state != SyncState::Idle && current_state != SyncState::Synced {
            return; // Already syncing
        }

        let local_height = self.chain.read().await.height();
        let best_height = self.stats.read().await.best_height;

        if best_height > local_height + 1 {
            info!(
                target: "neo::sync",
                local = local_height,
                best = best_height,
                behind = best_height - local_height,
                "starting block sync"
            );

            *self.state.write().await = SyncState::SyncingBlocks;
        }
    }

    /// Applies queued blocks to the chain
    async fn apply_queued_blocks(&self) {
        let local_height = self.chain.read().await.height();

        loop {
            // Find the next block to apply
            let next_block: Option<BlockData> = {
                let queue = self.block_queue.read().await;
                queue
                    .iter()
                    .find(|(h, _, _)| *h == local_height + 1)
                    .cloned()
            };

            match next_block {
                Some((height, hash, _data)) => {
                    // Remove from queue
                    {
                        let mut queue = self.block_queue.write().await;
                        queue.retain(|(h, _, _)| *h != height);
                    }

                    info!(
                        target: "neo::sync",
                        height,
                        hash = %hash,
                        "applying block"
                    );

                    // Emit chain event
                    let _ = self.chain_tx.send(ChainEvent::BlockAdded {
                        hash,
                        height,
                        on_main_chain: true,
                    });

                    // Update stats
                    {
                        let mut stats = self.stats.write().await;
                        stats.local_height = height;
                        if stats.best_height > 0 {
                            stats.progress = height as f64 / stats.best_height as f64;
                        }
                    }
                }
                None => break,
            }
        }

        // Check if sync is complete
        let stats = self.stats.read().await;
        if stats.local_height >= stats.best_height && stats.best_height > 0 {
            *self.state.write().await = SyncState::Synced;
            info!(target: "neo::sync", height = stats.local_height, "sync complete");
        }
    }

    /// Returns blocks needed for sync
    pub async fn get_blocks_to_request(&self, max_count: usize) -> Vec<(u32, Option<UInt256>)> {
        let local_height = self.chain.read().await.height();
        let best_height = self.stats.read().await.best_height;
        let pending = self.pending_requests.read().await;

        let mut requests = Vec::new();
        for height in (local_height + 1)..=best_height {
            if requests.len() >= max_count {
                break;
            }

            // Skip if already pending
            let already_pending = pending.values().any(|r| r.height == Some(height));
            if !already_pending {
                requests.push((height, None));
            }
        }

        requests
    }

    /// Stops the sync service
    pub async fn stop(&self) {
        *self.state.write().await = SyncState::Idle;
        info!(target: "neo::sync", "sync service stopped");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_sync_service_creation() {
        let chain = Arc::new(RwLock::new(ChainState::new()));
        let (chain_tx, _) = broadcast::channel(100);
        let service = SyncService::new(chain, chain_tx);

        assert_eq!(service.state().await, SyncState::Idle);
    }

    #[tokio::test]
    async fn test_peer_height_tracking() {
        let chain = Arc::new(RwLock::new(ChainState::new()));
        let (chain_tx, _) = broadcast::channel(100);
        let service = SyncService::new(chain, chain_tx);

        let peer: SocketAddr = "127.0.0.1:10333".parse().unwrap();
        service.update_peer_height(peer, 1000).await;

        let stats = service.stats().await;
        assert_eq!(stats.best_height, 1000);
    }

    #[tokio::test]
    async fn test_sync_stats() {
        let chain = Arc::new(RwLock::new(ChainState::new()));
        let (chain_tx, _) = broadcast::channel(100);
        let service = SyncService::new(chain, chain_tx);

        let stats = service.stats().await;
        assert_eq!(stats.local_height, 0);
        assert_eq!(stats.blocks_downloaded, 0);
    }
}
