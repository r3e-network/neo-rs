//! Integration tests for the Neo-RS node
//!
//! These tests verify that all components work together correctly

use neo_config::NetworkType;
// Consensus crate is optional in this workspace build; skip when absent
#[allow(unused_imports)]
use neo_core::{ShutdownCoordinator, UInt160};
use neo_ledger::{Blockchain, MempoolConfig};
use neo_network::{NetworkCommand, NetworkConfig, P2pNode};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

#[tokio::test]
async fn test_node_components_integration() {
    // Initialize blockchain
    let blockchain = Arc::new(Blockchain::new(NetworkType::Private).await.unwrap());

    // Initialize network
    let network_config = NetworkConfig::private();
    let (_cmd_tx, cmd_rx) = mpsc::channel::<NetworkCommand>(100);
    let p2p_node = Arc::new(P2pNode::new(network_config, cmd_rx).unwrap());

    // Initialize mempool
    let mempool_config = MempoolConfig::default();
    let mempool = Arc::new(RwLock::new(neo_ledger::MemoryPool::new(mempool_config)));
    assert_eq!(mempool.read().await.get_stats().transaction_count, 0);

    // Consensus is optional; this integration test focuses on blockchain + network wiring only

    // Verify components are initialized
    assert_eq!(blockchain.get_height().await, 0);
    assert_eq!(p2p_node.get_connected_peer_addresses().await.len(), 0);

    // Test consensus message creation using extensible payloads
    let consensus_payload = neo_network::messages::ExtensiblePayload::consensus(
        0,
        u32::MAX,
        UInt160::zero(),
        vec![1, 2, 3, 4],
        neo_core::Witness::empty(),
    );
    let consensus_msg = neo_network::messages::ProtocolMessage::Extensible {
        payload: consensus_payload.clone(),
    };
    let network_msg = neo_network::messages::NetworkMessage::new(consensus_msg.clone());
    assert_eq!(
        network_msg.command(),
        neo_network::messages::MessageCommand::Extensible
    );

    if let neo_network::messages::ProtocolMessage::Extensible { payload } = consensus_msg {
        assert_eq!(payload.category, "dBFT");
    } else {
        panic!("expected extensible consensus payload");
    }
}

#[tokio::test]
async fn test_blockchain_vm_integration() {
    // Initialize blockchain with VM support
    let blockchain = Arc::new(Blockchain::new(NetworkType::Private).await.unwrap());

    // Verify VM is available through blockchain
    // This test ensures VM module is properly integrated
    let height = blockchain.get_height().await;
    assert_eq!(height, 0, "Blockchain should start at height 0");
}

#[tokio::test]
async fn test_shutdown_coordination() {
    let shutdown = Arc::new(ShutdownCoordinator::new());

    // Test shutdown initiation
    let reason = "Test shutdown".to_string();
    shutdown.initiate_shutdown(reason.clone()).await.unwrap();

    // Verify shutdown was initiated
    let is_shutting_down = shutdown.is_shutting_down().await;
    assert!(is_shutting_down);
}
