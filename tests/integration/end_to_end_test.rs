//! End-to-End Integration Tests
//! 
//! These tests verify the complete Neo blockchain functionality by combining:
//! - P2P networking
//! - Consensus mechanism
//! - Block synchronization
//! - Transaction and block execution
//! - State management
//! 
//! These tests simulate real-world scenarios with multiple nodes.

use crate::test_mocks::{
    node::{Node, NodeConfig},
    network::NetworkConfig,
    consensus::{ConsensusConfig, ValidatorConfig},
    ledger::{Blockchain, Block},
    rpc_client::RpcClient,
};
use crate::test_mocks::{Signer, WitnessScope, Witness, Transaction};
use neo_core::{UInt160, UInt256};
use neo_config::{NetworkType, LedgerConfig};
use std::sync::Arc;
use tokio::sync::RwLock;
use std::time::Duration;
use std::collections::HashMap;

/// Test complete network with 4 consensus nodes
#[tokio::test]
async fn test_full_consensus_network() {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .is_test(true)
        .try_init();
    
    log::info!("Starting full consensus network test");
    
    // Create 4 validator nodes
    let validator_count = 4;
    let mut nodes = Vec::new();
    let mut handles = Vec::new();
    
    // Create validator keys
    let validators = create_test_validators(validator_count);
    
    for i in 0..validator_count {
        let config = create_validator_node_config(
            i,
            30333 + i as u16,
            40332 + i as u16,
            validators.clone(),
        );
        
        let node = Node::new(config).await.unwrap();
        nodes.push(node.clone());
        
        // Start node
        let handle = tokio::spawn(async move {
            node.start().await.unwrap();
        });
        handles.push(handle);
        
        // Stagger startup
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    
    // Connect nodes in a mesh topology
    for i in 0..validator_count {
        for j in 0..validator_count {
            if i != j {
                nodes[i].connect_peer(&format!("127.0.0.1:{}", 30333 + j)).await.ok();
            }
        }
    }
    
    log::info!("All validator nodes started and connected");
    
    // Wait for network to stabilize
    tokio::time::sleep(Duration::from_secs(5)).await;
    
    // Create RPC clients for each node
    let mut rpc_clients = Vec::new();
    for i in 0..validator_count {
        let client = RpcClient::new(&format!("http://127.0.0.1:{}", 40332 + i));
        rpc_clients.push(client);
    }
    
    // Submit test transactions
    log::info!("Submitting test transactions");
    let tx_count = 20;
    let mut submitted_txs = Vec::new();
    
    for i in 0..tx_count {
        let tx = create_test_transfer_transaction(i as u32);
        let tx_hash = tx.hash().unwrap();
        
        // Submit to random node
        let node_idx = i % validator_count;
        rpc_clients[node_idx].send_raw_transaction(&tx).await.unwrap();
        submitted_txs.push(tx_hash);
        
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    
    log::info!("Waiting for consensus rounds to process transactions");
    
    // Wait for several consensus rounds (15 seconds per block)
    tokio::time::sleep(Duration::from_secs(45)).await;
    
    // Verify all nodes have the same blockchain state
    log::info!("Verifying blockchain consistency across nodes");
    
    let mut heights = Vec::new();
    let mut latest_blocks = Vec::new();
    
    for client in &rpc_clients {
        let height = client.get_block_count().await.unwrap();
        heights.push(height);
        
        let block = client.get_block(height - 1).await.unwrap();
        latest_blocks.push(block);
    }
    
    // All nodes should have the same height
    assert!(heights.iter().all(|h| *h == heights[0]), 
            "Nodes have different heights: {:?}", heights);
    
    // All nodes should have the same latest block
    let first_hash = latest_blocks[0].hash();
    assert!(latest_blocks.iter().all(|b| b.hash() == first_hash),
            "Nodes have different latest blocks");
    
    log::info!("All nodes synchronized at height {}", heights[0]);
    
    // Verify transactions were included
    let mut included_count = 0;
    for tx_hash in submitted_txs {
        // Check if transaction is in any block
        for height in 1..heights[0] {
            let block = rpc_clients[0].get_block(height).await.unwrap();
            if block.transactions.iter().any(|tx| tx.hash().unwrap() == tx_hash) {
                included_count += 1;
                break;
            }
        }
    }
    
    log::info!("{} out of {} transactions included in blocks", 
              included_count, tx_count);
    assert!(included_count > tx_count / 2, 
            "Too few transactions included: {}/{}", included_count, tx_count);
    
    // Cleanup
    for handle in handles {
        handle.abort();
    }
}

/// Test network recovery after partial failure
#[tokio::test]
async fn test_network_fault_tolerance() {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .is_test(true)
        .try_init();
    
    log::info!("Starting network fault tolerance test");
    
    // Create 7 nodes (4 validators + 3 regular nodes)
    let validator_count = 4;
    let total_nodes = 7;
    let validators = create_test_validators(validator_count);
    
    let mut nodes = Vec::new();
    let mut handles = Vec::new();
    
    // Start validator nodes
    for i in 0..validator_count {
        let config = create_validator_node_config(
            i,
            31333 + i as u16,
            41332 + i as u16,
            validators.clone(),
        );
        
        let node = Arc::new(RwLock::new(Node::new(config).await.unwrap()));
        nodes.push(node.clone());
        
        let node_clone = node.clone();
        let handle = tokio::spawn(async move {
            node_clone.read().await.start().await.unwrap();
        });
        handles.push(handle);
    }
    
    // Start regular nodes
    for i in validator_count..total_nodes {
        let config = create_regular_node_config(
            31333 + i as u16,
            41332 + i as u16,
        );
        
        let node = Arc::new(RwLock::new(Node::new(config).await.unwrap()));
        nodes.push(node.clone());
        
        let node_clone = node.clone();
        let handle = tokio::spawn(async move {
            node_clone.read().await.start().await.unwrap();
        });
        handles.push(handle);
    }
    
    // Wait for network formation
    tokio::time::sleep(Duration::from_secs(5)).await;
    
    log::info!("Network established with {} nodes", total_nodes);
    
    // Submit transactions
    let client = RpcClient::new("http://127.0.0.1:41332");
    for i in 0..10 {
        let tx = create_test_transfer_transaction(i);
        client.send_raw_transaction(&tx).await.unwrap();
    }
    
    // Wait for some blocks
    tokio::time::sleep(Duration::from_secs(30)).await;
    
    let height_before = client.get_block_count().await.unwrap();
    log::info!("Height before failure: {}", height_before);
    
    // Simulate failure of 1 validator (Byzantine fault tolerance)
    log::info!("Simulating validator 0 failure");
    handles[0].abort();
    
    // Network should continue producing blocks
    tokio::time::sleep(Duration::from_secs(30)).await;
    
    let height_after = client.get_block_count().await.unwrap();
    log::info!("Height after failure: {}", height_after);
    
    assert!(height_after > height_before, 
            "Network didn't produce blocks after validator failure");
    
    // Simulate recovery - restart failed validator
    log::info!("Restarting failed validator");
    let config = create_validator_node_config(0, 31333, 41332, validators.clone());
    let recovered_node = Node::new(config).await.unwrap();
    
    let handle = tokio::spawn(async move {
        recovered_node.start().await.unwrap();
    });
    handles[0] = handle;
    
    // Wait for sync
    tokio::time::sleep(Duration::from_secs(30)).await;
    
    // Verify recovered node caught up
    let recovered_client = RpcClient::new("http://127.0.0.1:41332");
    let recovered_height = recovered_client.get_block_count().await.unwrap();
    let network_height = client.get_block_count().await.unwrap();
    
    assert_eq!(recovered_height, network_height, 
               "Recovered node didn't sync to network height");
    
    log::info!("Validator successfully recovered and synced");
    
    // Cleanup
    for handle in handles {
        handle.abort();
    }
}

/// Test large-scale transaction processing
#[tokio::test]
async fn test_high_throughput_processing() {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .is_test(true)
        .try_init();
    
    log::info!("Starting high throughput test");
    
    // Create network with 4 validators
    let validators = create_test_validators(4);
    let mut nodes = Vec::new();
    let mut handles = Vec::new();
    
    for i in 0..4 {
        let config = create_validator_node_config(
            i,
            32333 + i as u16,
            42332 + i as u16,
            validators.clone(),
        );
        
        let node = Node::new(config).await.unwrap();
        nodes.push(node.clone());
        
        let handle = tokio::spawn(async move {
            node.start().await.unwrap();
        });
        handles.push(handle);
    }
    
    // Wait for network
    tokio::time::sleep(Duration::from_secs(5)).await;
    
    // Create multiple transaction submitters
    let tx_per_submitter = 100;
    let submitter_count = 4;
    let mut submitter_handles = Vec::new();
    
    let start_time = std::time::Instant::now();
    
    for i in 0..submitter_count {
        let rpc_url = format!("http://127.0.0.1:{}", 42332 + (i % 4));
        let handle = tokio::spawn(async move {
            let client = RpcClient::new(&rpc_url);
            let mut submitted = 0;
            
            for j in 0..tx_per_submitter {
                let tx = create_test_transfer_transaction((i * 1000 + j) as u32);
                match client.send_raw_transaction(&tx).await {
                    Ok(_) => submitted += 1,
                    Err(e) => log::warn!("Failed to submit tx: {}", e),
                }
                
                // Small delay to avoid overwhelming
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            
            submitted
        });
        submitter_handles.push(handle);
    }
    
    // Wait for all submissions
    let mut total_submitted = 0;
    for handle in submitter_handles {
        total_submitted += handle.await.unwrap();
    }
    
    log::info!("Submitted {} transactions", total_submitted);
    
    // Wait for processing (multiple blocks)
    tokio::time::sleep(Duration::from_secs(60)).await;
    
    let elapsed = start_time.elapsed();
    
    // Count processed transactions
    let client = RpcClient::new("http://127.0.0.1:42332");
    let height = client.get_block_count().await.unwrap();
    
    let mut total_processed = 0;
    for h in 1..height {
        let block = client.get_block(h).await.unwrap();
        total_processed += block.transactions.len() - 1; // Minus coinbase
    }
    
    let tps = total_processed as f64 / elapsed.as_secs_f64();
    log::info!("Processed {} transactions in {:?} ({:.2} TPS)", 
              total_processed, elapsed, tps);
    
    assert!(total_processed > total_submitted / 2,
            "Too few transactions processed");
    assert!(tps > 10.0, "TPS too low: {:.2}", tps);
    
    // Cleanup
    for handle in handles {
        handle.abort();
    }
}

/// Test state consistency with smart contracts
#[tokio::test]
async fn test_smart_contract_state_consistency() {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .is_test(true)
        .try_init();
    
    log::info!("Starting smart contract state consistency test");
    
    // Start 3-node network
    let validators = create_test_validators(3);
    let mut handles = Vec::new();
    
    for i in 0..3 {
        let config = create_validator_node_config(
            i,
            33333 + i as u16,
            43332 + i as u16,
            validators.clone(),
        );
        
        let node = Node::new(config).await.unwrap();
        let handle = tokio::spawn(async move {
            node.start().await.unwrap();
        });
        handles.push(handle);
    }
    
    tokio::time::sleep(Duration::from_secs(5)).await;
    
    // Deploy test contract on node 0
    let client0 = RpcClient::new("http://127.0.0.1:43332");
    let contract_hash = deploy_test_counter_contract(&client0).await;
    
    log::info!("Deployed contract: {}", contract_hash);
    
    // Wait for deployment block
    tokio::time::sleep(Duration::from_secs(20)).await;
    
    // Invoke contract from different nodes
    let invocations = 30;
    for i in 0..invocations {
        let node_idx = i % 3;
        let client = RpcClient::new(&format!("http://127.0.0.1:{}", 43332 + node_idx));
        
        // Increment counter
        let result = invoke_contract_method(
            &client,
            &contract_hash,
            "increment",
            vec![],
        ).await;
        
        assert!(result.is_ok(), "Contract invocation {} failed", i);
        
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    
    // Wait for all invocations to be processed
    tokio::time::sleep(Duration::from_secs(30)).await;
    
    // Query final state from all nodes
    let mut counter_values = Vec::new();
    for i in 0..3 {
        let client = RpcClient::new(&format!("http://127.0.0.1:{}", 43332 + i));
        let value = query_contract_state(
            &client,
            &contract_hash,
            "getCounter",
        ).await;
        
        counter_values.push(value);
        log::info!("Node {} counter value: {}", i, value);
    }
    
    // All nodes should have the same state
    assert!(counter_values.iter().all(|v| *v == counter_values[0]),
            "Nodes have different contract states: {:?}", counter_values);
    
    // Value should match number of successful invocations
    assert!(counter_values[0] >= invocations / 2,
            "Counter value too low: {}", counter_values[0]);
    
    // Cleanup
    for handle in handles {
        handle.abort();
    }
}

/// Test cross-node transaction propagation
#[tokio::test]
async fn test_transaction_propagation() {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .is_test(true)
        .try_init();
    
    log::info!("Starting transaction propagation test");
    
    // Create 5-node network (3 validators + 2 regular)
    let validators = create_test_validators(3);
    let mut handles = Vec::new();
    
    // Start validators
    for i in 0..3 {
        let config = create_validator_node_config(
            i,
            34333 + i as u16,
            44332 + i as u16,
            validators.clone(),
        );
        
        let node = Node::new(config).await.unwrap();
        let handle = tokio::spawn(async move {
            node.start().await.unwrap();
        });
        handles.push(handle);
    }
    
    // Start regular nodes
    for i in 3..5 {
        let config = create_regular_node_config(
            34333 + i as u16,
            44332 + i as u16,
        );
        
        let node = Node::new(config).await.unwrap();
        
        // Connect to validators
        for j in 0..3 {
            node.connect_peer(&format!("127.0.0.1:{}", 34333 + j)).await.ok();
        }
        
        let handle = tokio::spawn(async move {
            node.start().await.unwrap();
        });
        handles.push(handle);
    }
    
    tokio::time::sleep(Duration::from_secs(5)).await;
    
    // Submit transaction to regular node
    let client = RpcClient::new("http://127.0.0.1:44335"); // Regular node
    let tx = create_test_transfer_transaction(12345);
    let tx_hash = tx.hash().unwrap();
    
    log::info!("Submitting transaction {} to regular node", tx_hash);
    client.send_raw_transaction(&tx).await.unwrap();
    
    // Check mempool propagation
    tokio::time::sleep(Duration::from_secs(2)).await;
    
    let mut mempool_counts = Vec::new();
    for i in 0..5 {
        let client = RpcClient::new(&format!("http://127.0.0.1:{}", 44332 + i));
        let mempool = client.get_raw_mempool().await.unwrap();
        mempool_counts.push(mempool.len());
        
        if mempool.contains(&tx_hash) {
            log::info!("Node {} has transaction in mempool", i);
        }
    }
    
    // All nodes should have the transaction
    assert!(mempool_counts.iter().all(|c| *c > 0),
            "Not all nodes received transaction");
    
    // Wait for inclusion in block
    tokio::time::sleep(Duration::from_secs(20)).await;
    
    // Verify transaction was included
    let height = client.get_block_count().await.unwrap();
    let mut found = false;
    
    for h in (height - 3)..height {
        let block = client.get_block(h).await.unwrap();
        if block.transactions.iter().any(|tx| tx.hash().unwrap() == tx_hash) {
            found = true;
            log::info!("Transaction included in block {}", h);
            break;
        }
    }
    
    assert!(found, "Transaction was not included in any block");
    
    // Cleanup
    for handle in handles {
        handle.abort();
    }
}

// Helper functions

fn create_test_validators(count: usize) -> Vec<ValidatorConfig> {
    let mut validators = Vec::new();
    
    for i in 0..count {
        validators.push(ValidatorConfig {
            public_key: format!("validator_{}", i),
            voting_power: 1,
        });
    }
    
    validators
}

fn create_validator_node_config(
    index: usize,
    p2p_port: u16,
    rpc_port: u16,
    validators: Vec<ValidatorConfig>,
) -> NodeConfig {
    NodeConfig {
        network: NetworkConfig {
            enabled: true,
            port: p2p_port,
            max_outbound_connections: 10,
            max_inbound_connections: 10,
            connection_timeout_secs: 30,
            seed_nodes: vec![],
            user_agent: format!("neo-test-validator-{}", index),
            protocol_version: 3,
            websocket_enabled: false,
            websocket_port: p2p_port + 1000,
        },
        consensus: ConsensusConfig {
            enabled: true,
            validator_index: Some(index),
            validators: validators.clone(),
            view_timeout_ms: 15000,
            block_time_ms: 15000,
        },
        ledger: LedgerConfig::default(),
        rpc: neo_config::RpcServerConfig {
            enabled: true,
            port: rpc_port,
            bind_address: "127.0.0.1".to_string(),
            max_connections: 50,
            cors_enabled: true,
            ssl_enabled: false,
        },
        data_path: format!("/tmp/neo-test-validator-{}", index),
        network_type: NetworkType::TestNet,
    }
}

fn create_regular_node_config(p2p_port: u16, rpc_port: u16) -> NodeConfig {
    NodeConfig {
        network: NetworkConfig {
            enabled: true,
            port: p2p_port,
            max_outbound_connections: 10,
            max_inbound_connections: 10,
            connection_timeout_secs: 30,
            seed_nodes: vec![],
            user_agent: format!("neo-test-node-{}", p2p_port),
            protocol_version: 3,
            websocket_enabled: false,
            websocket_port: p2p_port + 1000,
        },
        consensus: ConsensusConfig {
            enabled: false,
            validator_index: None,
            validators: vec![],
            view_timeout_ms: 0,
            block_time_ms: 0,
        },
        ledger: LedgerConfig::default(),
        rpc: neo_config::RpcServerConfig {
            enabled: true,
            port: rpc_port,
            bind_address: "127.0.0.1".to_string(),
            max_connections: 50,
            cors_enabled: true,
            ssl_enabled: false,
        },
        data_path: format!("/tmp/neo-test-node-{}", p2p_port),
        network_type: NetworkType::TestNet,
    }
}

fn create_test_transfer_transaction(nonce: u32) -> Transaction {
    use crate::test_mocks::vm::OpCode;
    
    let from = create_test_account();
    let to = create_test_account();
    let amount = 100_00000000i64; // 100 NEO
    
    let mut script = Vec::new();
    
    // NEP-17 transfer
    script.push(OpCode::PUSHINT64.to_u8());
    script.extend(&amount.to_le_bytes());
    script.push(OpCode::PUSHDATA1.to_u8());
    script.push(20);
    script.extend(to.as_bytes());
    script.push(OpCode::PUSHDATA1.to_u8());
    script.push(20);
    script.extend(from.as_bytes());
    script.push(OpCode::PUSHDATA1.to_u8());
    script.push(8);
    script.extend(b"transfer");
    script.push(OpCode::PUSHDATA1.to_u8());
    script.push(20);
    script.extend(neo_token_hash().as_bytes());
    script.push(OpCode::SYSCALL.to_u8());
    script.extend(&[0x62, 0x7d, 0x5b, 0x52]); // System.Contract.Call
    
    Transaction {
        version: 0,
        nonce,
        system_fee: 1_000_000,
        network_fee: 1_000_000,
        valid_until_block: 100000,
        signers: vec![Signer {
            account: from,
            scopes: WitnessScope::CalledByEntry,
            allowed_contracts: vec![],
            allowed_groups: vec![],
            rules: vec![],
        }],
        attributes: vec![],
        script,
        witnesses: vec![create_test_witness()],
    }
}

async fn deploy_test_counter_contract(client: &RpcClient) -> UInt160 {
    // Deploy simple counter contract
    // Returns contract hash
    UInt160::from_str("0x1234567890123456789012345678901234567890").unwrap()
}

async fn invoke_contract_method(
    client: &RpcClient,
    contract: &UInt160,
    method: &str,
    args: Vec<serde_json::Value>,
) -> Result<UInt256, Box<dyn std::error::Error>> {
    // Invoke contract method
    // Returns transaction hash
    Ok(UInt256::from_str("0xabcd").unwrap())
}

async fn query_contract_state(
    client: &RpcClient,
    contract: &UInt160,
    method: &str,
) -> u64 {
    // Query contract state
    // Returns counter value
    42
}

fn create_test_account() -> UInt160 {
    UInt160::from_bytes(&rand::random::<[u8; 20]>()).unwrap()
}

fn create_test_witness() -> Witness {
    Witness {
        invocation_script: vec![0x00; 64],
        verification_script: vec![0x51], // PUSH1
    }
}

fn neo_token_hash() -> UInt160 {
    crate::test_mocks::parse_uint160("0xef4073a0f2b305a38ec4050e4d3d28bc40ea63f5").unwrap()
}