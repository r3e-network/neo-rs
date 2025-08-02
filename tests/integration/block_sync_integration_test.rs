//! Block Synchronization Integration Tests
//! 
//! These tests verify the complete block synchronization functionality including:
//! - Initial block download (IBD)
//! - Block header synchronization
//! - Block data synchronization
//! - Chain reorganization handling
//! - Checkpoint synchronization

use crate::test_mocks::{
    network::{
        sync::{SyncManager, SyncState, SyncStrategy},
        p2p::Node,
    },
    ledger::{Blockchain, Block, BlockHeader},
    Transaction, MockPeer,
};
use neo_core::{UInt256, UInt160};
use neo_config::NetworkType;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::time::Duration;
use tokio::time::timeout;

/// Test initial block download from genesis
#[tokio::test]
async fn test_initial_block_download() {
    let _ = env_logger::builder().is_test(true).try_init();
    
    // Create two blockchains: one with blocks, one empty
    let source_blockchain = create_blockchain_with_blocks(100).await;
    let target_blockchain = create_empty_blockchain().await;
    
    // Create sync manager for target
    let sync_manager = Arc::new(SyncManager::new(
        target_blockchain.clone(),
        SyncStrategy::FastSync,
    ));
    
    // Create mock peer that serves blocks from source blockchain
    let peer = create_mock_peer(source_blockchain.clone()).await;
    
    // Start synchronization
    sync_manager.add_peer(peer).await;
    sync_manager.start_sync().await.unwrap();
    
    // Wait for sync to complete with timeout
    let sync_result = timeout(Duration::from_secs(30), async {
        loop {
            let state = sync_manager.get_sync_state().await;
            match state {
                SyncState::Synchronized => break,
                SyncState::Failed(err) => panic!("Sync failed: {}", err),
                _ => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        }
    }).await;
    
    assert!(sync_result.is_ok(), "Sync timed out");
    
    // Verify all blocks were synchronized
    let target_height = target_blockchain.get_height().await.unwrap();
    let source_height = source_blockchain.get_height().await.unwrap();
    assert_eq!(target_height, source_height, "Not all blocks were synchronized");
    
    // Verify block hashes match
    for height in 0..=source_height {
        let source_block = source_blockchain.get_block_by_index(height).await.unwrap().unwrap();
        let target_block = target_blockchain.get_block_by_index(height).await.unwrap().unwrap();
        assert_eq!(source_block.hash(), target_block.hash(), 
                   "Block mismatch at height {}", height);
    }
}

/// Test block header synchronization
#[tokio::test]
async fn test_header_synchronization() {
    let _ = env_logger::builder().is_test(true).try_init();
    
    // Create blockchain with many blocks
    let blockchain = create_blockchain_with_blocks(1000).await;
    let sync_manager = Arc::new(SyncManager::new(
        create_empty_blockchain().await,
        SyncStrategy::HeadersFirst,
    ));
    
    // Test header sync in batches
    let peer = create_mock_peer(blockchain.clone()).await;
    sync_manager.add_peer(peer).await;
    
    // Start header sync
    sync_manager.sync_headers().await.unwrap();
    
    // Wait for header sync to complete
    tokio::time::sleep(Duration::from_secs(5)).await;
    
    // Verify headers were downloaded
    let header_height = sync_manager.get_header_height().await;
    assert_eq!(header_height, 1000, "Not all headers were synchronized");
    
    // Verify headers are valid and form a chain
    let mut prev_hash = UInt256::zero(); // Genesis
    for height in 1..=header_height {
        let header = sync_manager.get_header(height).await.unwrap();
        assert_eq!(header.prev_hash, prev_hash, 
                   "Header chain broken at height {}", height);
        prev_hash = header.hash();
    }
}

/// Test parallel block download
#[tokio::test]
async fn test_parallel_block_download() {
    let _ = env_logger::builder().is_test(true).try_init();
    
    // Create source blockchain
    let source_blockchain = create_blockchain_with_blocks(500).await;
    
    // Create multiple peers serving the same blockchain
    let peer_count = 5;
    let mut peers = Vec::new();
    for _ in 0..peer_count {
        peers.push(create_mock_peer(source_blockchain.clone()).await);
    }
    
    // Create sync manager with parallel download strategy
    let target_blockchain = create_empty_blockchain().await;
    let sync_manager = Arc::new(SyncManager::new(
        target_blockchain.clone(),
        SyncStrategy::ParallelDownload { max_parallel: 5 },
    ));
    
    // Add all peers
    for peer in peers {
        sync_manager.add_peer(peer).await;
    }
    
    // Start sync and measure time
    let start = std::time::Instant::now();
    sync_manager.start_sync().await.unwrap();
    
    // Wait for completion
    let sync_result = timeout(Duration::from_secs(20), async {
        loop {
            if sync_manager.get_sync_state().await == SyncState::Synchronized {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }).await;
    
    let elapsed = start.elapsed();
    assert!(sync_result.is_ok(), "Parallel sync timed out");
    
    println!("Parallel sync completed in {:?}", elapsed);
    
    // Verify all blocks downloaded
    let height = target_blockchain.get_height().await.unwrap();
    assert_eq!(height, 500);
    
    // Verify download was distributed across peers
    let stats = sync_manager.get_peer_statistics().await;
    for (peer_id, stat) in stats {
        println!("Peer {} downloaded {} blocks", peer_id, stat.blocks_downloaded);
        assert!(stat.blocks_downloaded > 50, 
                "Peer {} didn't participate enough", peer_id);
    }
}

/// Test chain reorganization handling
#[tokio::test]
async fn test_chain_reorganization() {
    let _ = env_logger::builder().is_test(true).try_init();
    
    // Create main chain
    let main_chain = create_blockchain_with_blocks(100).await;
    
    // Create alternative chain that forks at block 80
    let alt_chain = create_empty_blockchain().await;
    
    // Copy blocks 0-80 from main chain
    for height in 0..=80 {
        let block = main_chain.get_block_by_index(height).await.unwrap().unwrap();
        alt_chain.add_block(block).await.unwrap();
    }
    
    // Create different blocks for 81-110 with more work
    for height in 81..=110 {
        let block = create_test_block(height, alt_chain.clone()).await;
        alt_chain.add_block(block).await.unwrap();
    }
    
    // Sync manager should detect and switch to longer chain
    let sync_manager = Arc::new(SyncManager::new(
        main_chain.clone(),
        SyncStrategy::FastSync,
    ));
    
    let peer = create_mock_peer(alt_chain.clone()).await;
    sync_manager.add_peer(peer).await;
    sync_manager.start_sync().await.unwrap();
    
    // Wait for reorg
    tokio::time::sleep(Duration::from_secs(10)).await;
    
    // Verify reorganization occurred
    let current_height = main_chain.get_height().await.unwrap();
    assert_eq!(current_height, 110, "Chain didn't reorganize to longer chain");
    
    // Verify blocks 81-110 match alternative chain
    for height in 81..=110 {
        let main_block = main_chain.get_block_by_index(height).await.unwrap().unwrap();
        let alt_block = alt_chain.get_block_by_index(height).await.unwrap().unwrap();
        assert_eq!(main_block.hash(), alt_block.hash(), 
                   "Block mismatch after reorg at height {}", height);
    }
}

/// Test checkpoint-based fast sync
#[tokio::test]
async fn test_checkpoint_sync() {
    let _ = env_logger::builder().is_test(true).try_init();
    
    // Create blockchain with checkpoint
    let checkpoint_height = 1000;
    let total_height = 2000;
    let source_blockchain = create_blockchain_with_blocks(total_height).await;
    
    // Create checkpoint
    let checkpoint_block = source_blockchain
        .get_block_by_index(checkpoint_height)
        .await.unwrap().unwrap();
    let checkpoint_hash = checkpoint_block.hash();
    
    // Create sync manager with checkpoint
    let target_blockchain = create_empty_blockchain().await;
    let sync_manager = Arc::new(SyncManager::new_with_checkpoint(
        target_blockchain.clone(),
        SyncStrategy::CheckpointSync,
        checkpoint_height,
        checkpoint_hash,
    ));
    
    let peer = create_mock_peer(source_blockchain.clone()).await;
    sync_manager.add_peer(peer).await;
    
    // Start checkpoint sync
    sync_manager.start_sync().await.unwrap();
    
    // Wait for sync
    let sync_result = timeout(Duration::from_secs(30), async {
        loop {
            if sync_manager.get_sync_state().await == SyncState::Synchronized {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }).await;
    
    assert!(sync_result.is_ok(), "Checkpoint sync timed out");
    
    // Verify only blocks after checkpoint were fully validated
    let validation_stats = sync_manager.get_validation_statistics().await;
    assert!(validation_stats.full_validations < total_height / 2,
            "Too many full validations for checkpoint sync");
    
    // Verify final state matches
    let height = target_blockchain.get_height().await.unwrap();
    assert_eq!(height, total_height);
}

/// Test sync recovery from interrupted state
#[tokio::test]
async fn test_sync_recovery() {
    let _ = env_logger::builder().is_test(true).try_init();
    
    // Create partially synced blockchain
    let target_blockchain = create_blockchain_with_blocks(500).await;
    let source_blockchain = create_blockchain_with_blocks(1000).await;
    
    // Create sync manager that should resume from block 501
    let sync_manager = Arc::new(SyncManager::new(
        target_blockchain.clone(),
        SyncStrategy::FastSync,
    ));
    
    let peer = create_mock_peer(source_blockchain.clone()).await;
    sync_manager.add_peer(peer).await;
    
    // Record starting height
    let start_height = target_blockchain.get_height().await.unwrap();
    assert_eq!(start_height, 500);
    
    // Start sync
    sync_manager.start_sync().await.unwrap();
    
    // Wait for completion
    tokio::time::sleep(Duration::from_secs(10)).await;
    
    // Verify sync resumed from correct position
    let final_height = target_blockchain.get_height().await.unwrap();
    assert_eq!(final_height, 1000);
    
    // Verify no duplicate processing
    let sync_stats = sync_manager.get_statistics().await;
    assert_eq!(sync_stats.blocks_processed, 500, 
               "Sync didn't resume correctly");
}

/// Test handling of slow or unresponsive peers
#[tokio::test]
async fn test_slow_peer_handling() {
    let _ = env_logger::builder().is_test(true).try_init();
    
    let blockchain = create_blockchain_with_blocks(100).await;
    let sync_manager = Arc::new(SyncManager::new(
        create_empty_blockchain().await,
        SyncStrategy::FastSync,
    ));
    
    // Add one fast peer and one slow peer
    let fast_peer = create_mock_peer(blockchain.clone()).await;
    let slow_peer = create_slow_mock_peer(blockchain.clone(), 
                                         Duration::from_secs(2)).await;
    
    sync_manager.add_peer(fast_peer).await;
    sync_manager.add_peer(slow_peer).await;
    
    // Start sync
    let start = std::time::Instant::now();
    sync_manager.start_sync().await.unwrap();
    
    // Wait for completion
    let sync_result = timeout(Duration::from_secs(20), async {
        loop {
            if sync_manager.get_sync_state().await == SyncState::Synchronized {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }).await;
    
    assert!(sync_result.is_ok(), "Sync timed out due to slow peer");
    let elapsed = start.elapsed();
    
    // Verify sync completed reasonably fast despite slow peer
    assert!(elapsed < Duration::from_secs(10), 
            "Sync took too long: {:?}", elapsed);
    
    // Verify slow peer was deprioritized
    let stats = sync_manager.get_peer_statistics().await;
    let fast_peer_blocks = stats.get(&0).unwrap().blocks_downloaded;
    let slow_peer_blocks = stats.get(&1).unwrap().blocks_downloaded;
    
    assert!(fast_peer_blocks > slow_peer_blocks * 2,
            "Slow peer wasn't properly deprioritized");
}

// Helper functions

async fn create_empty_blockchain() -> Arc<Blockchain> {
    let blockchain = Blockchain::new(
        NetworkType::TestNet,
        "/tmp/neo-test-blockchain-empty",
    ).await.unwrap();
    Arc::new(blockchain)
}

async fn create_blockchain_with_blocks(count: u32) -> Arc<Blockchain> {
    let blockchain = Arc::new(Blockchain::new(
        NetworkType::TestNet,
        &format!("/tmp/neo-test-blockchain-{}", count),
    ).await.unwrap());
    
    // Add test blocks
    for i in 1..=count {
        let block = create_test_block(i, blockchain.clone()).await;
        blockchain.add_block(block).await.unwrap();
    }
    
    blockchain
}

async fn create_test_block(index: u32, blockchain: Arc<Blockchain>) -> Block {
    let prev_hash = if index == 0 {
        UInt256::zero()
    } else {
        blockchain.get_block_by_index(index - 1)
            .await.unwrap().unwrap().hash()
    };
    
    Block {
        header: BlockHeader {
            version: 0,
            prev_hash,
            merkle_root: UInt256::zero(),
            timestamp: 1000000 + (index as u64 * 15),
            index,
            primary_index: 0,
            next_consensus: UInt160::zero(),
            witness: Default::default(),
        },
        transactions: vec![],
    }
}

async fn create_mock_peer(blockchain: Arc<Blockchain>) -> MockPeer {
    MockPeer {
        id: 0,
        blockchain,
        response_delay: Duration::from_millis(10),
    }
}

async fn create_slow_mock_peer(
    blockchain: Arc<Blockchain>, 
    delay: Duration
) -> MockPeer {
    MockPeer {
        id: 1,
        blockchain,
        response_delay: delay,
    }
}

// MockPeer implementation is now in test_mocks.rs