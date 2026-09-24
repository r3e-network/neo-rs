//! Block Prefetch Worker
//!
//! Pre-loads next N blocks into memory while current block processes.
//! Implements Wave 2 of multi-block pipeline optimization.
//!
//! # Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────────┐
//! │              BlockPrefetchService                               │
//! │                                                                  │
//! │  Network Receiver            Buffer Pool                       │
//! │  ──────────────            ─────────────                       │
//! │                                                                  │
//! │  [Msg N] ─────────→ Pre-process → [BlockBuffer h=n]           │
//! │      ↓                                      ↑                   │
//! │  [Msg N+1] ─────────→ Pre-process → [BlockBuffer h=n+1]        │
//! │      ↓                                      ↑                   │
//! │  [Msg N+2] ─────────→ Pre-process → [BlockBuffer h=n+2]        │
//! │                                           ...                   │
//! │                                          [Main Loop Fetches]  │
//! └──────────────────────────────────────────────────────────────────┘
//!
//! Key Benefits:
//! - Eliminate network latency from critical path
//! - Continuous processing even during transient stalls
//! - Smooth burst handling with buffered capacity
//!

use crossbeam_channel::{Receiver, Sender, TryRecvError};
use parking_lot::RwLock;
use std::sync::Arc;
use tracing::{debug, info, trace, warn};

/// Default prefetch buffer size (blocks ahead)
const DEFAULT_PREFETCH_SIZE: usize = 10;

/// Trigger signal when a new block becomes available in buffer
#[derive(Debug, Clone)]
pub struct PrefetchReadyTrigger {
    /// Block height that is ready
    pub height: u32,
}

/// Prefetched block data ready for execution
#[derive(Debug, Clone)]
pub struct PrefetchedBlockData {
    /// Original block data received from network
    pub block_data: Vec<u8>,

    /// Deserialized block header information
    pub height: u32,

    /// Pre-computed transaction count
    pub tx_count: usize,

    /// Whether block passed basic validation
    pub valid: bool,

    /// Error message if invalid (None when valid)
    pub validation_error: Option<String>,
}

/// Service that prefetches blocks ahead of current processing point
pub struct BlockPrefetchService {
    /// Next block ready fetch receiver
    next_block_rx: Arc<RwLock<Receiver<PrefetchedBlockData>>>,

    /// Ready signals sender (main loop can check if next block available)
    ready_signal_tx: Sender<u32>,

    /// Signals received (main loop listens on this channel)
    ready_signal_rx: Arc<RwLock<Receiver<u32>>>,

    /// Buffer capacity (max blocks to hold)
    buffer_capacity: usize,

    /// Current number of prefetched blocks
    prefetch_count: Arc<RwLock<usize>>,

    /// Metrics
    total_prefetched: Arc<RwLock<u64>>,
    total_delivered: Arc<RwLock<u64>>,
}

impl BlockPrefetchService {
    /// Creates a new prefetch service without spawning workers.
    /// Main loop calls `process_network_message()` to push blocks into buffer.
    pub fn new(buffer_capacity: usize) -> Self {
        let (next_block_tx, next_block_rx) = crossbeam_channel::bounded(buffer_capacity);
        let (ready_tx, ready_rx) = crossbeam_channel::unbounded();

        Self {
            next_block_rx: Arc::new(RwLock::new(next_block_rx)),
            ready_signal_tx: ready_tx,
            ready_signal_rx: Arc::new(RwLock::new(ready_rx)),
            buffer_capacity,
            prefetch_count: Arc::new(RwLock::new(0)),
            total_prefetched: Arc::new(RwLock::new(0)),
            total_delivered: Arc::new(RwLock::new(0)),
        }
    }

    /// Process a network message and potentially add to prefetch buffer.
    ///
    /// This should be called from main loop instead of direct network receive.
    pub fn process_network_message(&self, message_bytes: &[u8]) {
        // Parse block data (placeholder - would deserialize actual block format)
        let parsed_height = self.extract_height(message_bytes);
        let is_valid = self.validate_basic(message_bytes);

        let block_data = message_bytes.to_vec();

        let prefetch_data = PrefetchedBlockData {
            block_data,
            height: parsed_height.unwrap_or(0),
            tx_count: 0,
            valid: is_valid,
            validation_error: if !is_valid {
                Some("Basic validation failed".to_string())
            } else {
                None
            },
        };

        // Add to buffer (non-blocking - drops if full)
        match self.next_block_rx.write().send(prefetch_data) {
            Ok(_) => {
                let height = prefetch_data.height;
                *self.prefetch_count.write() += 1;
                *self.total_prefetched.write() += 1;

                // Signal main loop that next block is ready
                let _ = self.ready_signal_tx.send(height);

                debug!(
                    target: "neo::prefetch",
                    height,
                    buffer_size = *self.prefetch_count.read(),
                    "Prefetched block added to buffer"
                );
            }
            Err(e) => {
                warn!(
                    target: "neo::prefetch",
                    error = %e,
                    buffer_full = *self.prefetch_count.read(),
                    capacity = self.buffer_capacity,
                    "Failed to add block to prefetch buffer (full)"
                );
            }
        }
    }

    /// Extract block height from raw message bytes (placeholder implementation)
    fn extract_height(&self, _message_bytes: &[u8]) -> Option<u32> {
        // In production, parse Neo block header to extract height
        // For now, return None (main loop should fill in correct height)
        None
    }

    /// Basic validation check (placeholder)
    fn validate_basic(&self, _message_bytes: &[u8]) -> bool {
        // Placeholder - would verify block signature, merkle root, etc.
        true
    }

    /// Check if next block has been prefetched (non-blocking).
    pub fn is_next_block_ready(&self) -> bool {
        !self.next_block_rx.read().is_empty()
    }

    /// Get prefetched block (non-blocking, returns None if empty).
    pub fn get_next_block(&self) -> Option<PrefetchedBlockData> {
        match self.next_block_rx.read().try_recv() {
            Ok(block) => {
                *self.prefetch_count.write() -= 1;
                *self.total_delivered.write() += 1;
                Some(block)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => None,
        }
    }

    /// Check if next block is ready WITHOUT consuming it.
    /// Returns (height_opt, consumed) where consumed indicates if you need to call get_next_block.
    pub fn peek_next_block(&self) -> Option<u32> {
        if let Ok(ref rx) = self.next_block_rx.try_read() {
            if !rx.is_empty() {
                // Need to actually receive to get height
                // This is a simplification - in production you'd want a peek mechanism
                None
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Wait for next block to become ready (blocking).
    pub fn wait_for_next_block(&self) -> Option<PrefetchedBlockData> {
        self.next_block_rx.read().recv().ok()
    }

    /// Signal to main loop that a specific height is ready for processing.
    pub fn signal_height_ready(&self, height: u32) {
        let _ = self.ready_signal_tx.send(height);
    }

    /// Receive ready signals from worker threads.
    pub fn try_receive_ready(&self) -> Option<u32> {
        self.ready_signal_rx.read().try_recv().ok()
    }

    /// Get current prefetch buffer size.
    pub fn current_buffer_size(&self) -> usize {
        *self.prefetch_count.read()
    }

    /// Get whether buffer is at capacity.
    pub fn is_at_capacity(&self) -> bool {
        *self.prefetch_count.read() >= self.buffer_capacity
    }

    /// Get statistics snapshot.
    pub fn get_stats(&self) -> PrefetchStats {
        PrefetchStats {
            current_buffer_size: *self.prefetch_count.read(),
            total_prefetched: *self.total_prefetched.read(),
            total_delivered: *self.total_delivered.read(),
            buffer_capacity: self.buffer_capacity,
            utilization: (*self.prefetch_count.read() as f64 / self.buffer_capacity as f64) * 100.0,
        }
    }

    /// Clear prefetch buffer (force discard all pending blocks).
    pub fn clear_buffer(&self) {
        while self.get_next_block().is_some() {}
        *self.prefetch_count.write() = 0;
        debug!(target: "neo::prefetch", "Prefetch buffer cleared");
    }
}

impl Default for BlockPrefetchService {
    fn default() -> Self {
        Self::new(DEFAULT_PREFETCH_SIZE)
    }
}

/// Statistics about prefetch service performance
#[derive(Debug, Clone)]
pub struct PrefetchStats {
    /// Current number of blocks in buffer
    pub current_buffer_size: usize,

    /// Total blocks ever prefetched
    pub total_prefetched: u64,

    /// Total blocks delivered to main loop
    pub total_delivered: u64,

    /// Maximum buffer capacity
    pub buffer_capacity: usize,

    /// Current utilization percentage
    pub utilization: f64,
}

// Example usage in main loop (pseudo-code):
/*
let mut prefetch_service = BlockPrefetchService::new(10);

loop {
    // Step 1: Try to get prefetched block
    let block_n = if prefetch_service.is_next_block_ready() {
        prefetch_service.get_next_block().unwrap()
    } else {
        // Fallback: receive from network
        let msg = network_receiver.recv()?;
        prefetch_service.process_network_message(&msg);

        // If still nothing prefetched after processing, fall back
        prefetch_service.get_next_block().or_else(|| {
            // Last resort: direct receive
            receive_from_network()?.block_data
        })
    };

    // Step 2: Execute transactions (Wave 1 parallel exec)
    execute_transactions_in_block(&block_n)?;

    // Step 3: Commit write batch
    db.write_batch(write_batch)?;

    // Continue immediately without waiting for MPT!
}
*/

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prefetch_service_creation() {
        let service = BlockPrefetchService::new(10);
        assert_eq!(service.current_buffer_size(), 0);
        assert_eq!(service.buffer_capacity, 10);
    }

    #[test]
    fn test_prefetch_add_and_retrieve() {
        let service = BlockPrefetchService::new(5);

        // Simulate receiving a block
        service.process_network_message(&[1, 2, 3, 4]);

        assert!(service.is_next_block_ready());
        assert_eq!(service.current_buffer_size(), 1);

        // Retrieve the block
        let block = service.get_next_block();
        assert!(block.is_some());
        assert_eq!(service.current_buffer_size(), 0);
    }

    #[test]
    fn test_prefetch_buffer_overflow() {
        let service = BlockPrefetchService::new(2);

        // Fill buffer
        service.process_network_message(&[1]);
        service.process_network_message(&[2]);

        assert_eq!(service.current_buffer_size(), 2);
        assert!(service.is_at_capacity());

        // Try to add one more (should drop)
        service.process_network_message(&[3]);

        // Buffer size should remain at capacity
        assert_eq!(service.current_buffer_size(), 2);
    }

    #[test]
    fn test_prefetch_clear() {
        let service = BlockPrefetchService::new(5);

        service.process_network_message(&[1]);
        service.process_network_message(&[2]);

        service.clear_buffer();

        assert_eq!(service.current_buffer_size(), 0);
        assert!(service.is_next_block_ready() == false);
    }
}
