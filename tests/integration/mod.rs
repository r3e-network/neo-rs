//! Neo-RS Integration Test Suite
//! 
//! This test suite provides comprehensive integration testing for the Neo blockchain
//! implementation in Rust. It verifies that all major components work correctly
//! together in realistic scenarios.
//! 
//! ## Test Categories:
//! 
//! 1. **P2P Networking Tests** (`p2p_integration_test.rs`)
//!    - Peer discovery and connection management
//!    - Message propagation and routing
//!    - Network resilience and fault tolerance
//!    - Protocol compliance and security
//! 
//! 2. **Consensus Tests** (`consensus_integration_test.rs`)
//!    - dBFT consensus rounds with multiple validators
//!    - Byzantine fault tolerance verification
//!    - View change and recovery mechanisms
//!    - Performance under various network conditions
//! 
//! 3. **Block Synchronization Tests** (`block_sync_integration_test.rs`)
//!    - Initial block download (IBD)
//!    - Header-first synchronization
//!    - Parallel block downloading
//!    - Chain reorganization handling
//!    - Checkpoint-based fast sync
//! 
//! 4. **Execution Tests** (`execution_integration_test.rs`)
//!    - Transaction validation and execution
//!    - Block processing and state transitions
//!    - Smart contract deployment and invocation
//!    - Gas calculation and resource limits
//!    - State persistence and rollback
//! 
//! 5. **End-to-End Tests** (`end_to_end_test.rs`)
//!    - Full network simulation with multiple nodes
//!    - Complete transaction lifecycle
//!    - Cross-node state consistency
//!    - High-throughput scenarios
//!    - Fault recovery and resilience
//! 
//! ## Running the Tests
//! 
//! Run all integration tests:
//! ```bash
//! cargo test --test integration_tests --features integration-tests
//! ```
//! 
//! Run specific test category:
//! ```bash
//! cargo test --test integration_tests p2p -- --nocapture
//! cargo test --test integration_tests consensus -- --nocapture
//! cargo test --test integration_tests block_sync -- --nocapture
//! cargo test --test integration_tests execution -- --nocapture
//! cargo test --test integration_tests end_to_end -- --nocapture
//! ```
//! 
//! Run with debug logging:
//! ```bash
//! RUST_LOG=debug cargo test --test integration_tests -- --nocapture
//! ```
//! 
//! ## Test Environment
//! 
//! These tests create temporary blockchain data in `/tmp/neo-test-*` directories
//! which are automatically cleaned up after test completion. The tests use
//! non-standard ports to avoid conflicts with running Neo nodes.

#[cfg(test)]
mod test_mocks;

#[cfg(test)]
mod p2p_integration_test;

#[cfg(test)]
mod consensus_integration_test;

#[cfg(test)]
mod block_sync_integration_test;

#[cfg(test)]
mod execution_integration_test;

#[cfg(test)]
mod end_to_end_test;

/// Common test utilities and helpers
#[cfg(test)]
pub mod test_utils {
    use neo_core::{UInt160, UInt256};
    use std::sync::atomic::{AtomicU16, Ordering};
    
    /// Port allocator to avoid conflicts between tests
    static NEXT_PORT: AtomicU16 = AtomicU16::new(50000);
    
    /// Get next available port for testing
    pub fn get_test_port() -> u16 {
        NEXT_PORT.fetch_add(1, Ordering::SeqCst)
    }
    
    /// Create deterministic test account from index
    pub fn test_account(index: u32) -> UInt160 {
        let mut bytes = [0u8; 20];
        bytes[0..4].copy_from_slice(&index.to_le_bytes());
        UInt160::from_bytes(&bytes).unwrap()
    }
    
    /// Create deterministic test hash from index
    pub fn test_hash(index: u32) -> UInt256 {
        let mut bytes = [0u8; 32];
        bytes[0..4].copy_from_slice(&index.to_le_bytes());
        UInt256::from_bytes(&bytes).unwrap()
    }
    
    /// Clean up test data directory
    pub fn cleanup_test_dir(path: &str) {
        std::fs::remove_dir_all(path).ok();
    }
}

/// Integration test configuration
#[cfg(test)]
pub struct IntegrationTestConfig {
    /// Enable debug logging
    pub debug_logging: bool,
    /// Test timeout in seconds
    pub timeout_secs: u64,
    /// Number of validator nodes
    pub validator_count: usize,
    /// Number of regular nodes
    pub regular_node_count: usize,
    /// Transactions per test
    pub transaction_count: usize,
}

#[cfg(test)]
impl Default for IntegrationTestConfig {
    fn default() -> Self {
        Self {
            debug_logging: false,
            timeout_secs: 300, // 5 minutes
            validator_count: 4,
            regular_node_count: 2,
            transaction_count: 100,
        }
    }
}