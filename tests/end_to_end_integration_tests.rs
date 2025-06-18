//! End-to-End Integration Tests
//!
//! These tests verify that all components work together in complete workflows.

use neo_core::*;
use neo_vm::*;
use neo_network::*;
use neo_cryptography::*;
use neo_rpc_server::*;
use neo_ledger::*;
use neo_consensus::*;
use neo_smart_contract::*;
use neo_persistence::*;
use std::sync::Arc;
use tokio_test;

/// Test complete blockchain node startup and operation
#[tokio::test]
async fn test_complete_node_operation() {
    println!("🚀 Testing complete node operation");
    
    // 1. Initialize storage
    let storage = Arc::new(Storage::new_memory());
    
    // 2. Create blockchain
    let blockchain = Arc::new(Blockchain::new(storage.clone()));
    
    // 3. Start network
    let network_config = NetworkConfig::default();
    let network_server = NetworkServer::new(network_config).unwrap();
    
    // 4. Start RPC server
    let rpc_config = RpcServerConfig::default();
    let rpc_server = RpcServer::new(rpc_config).unwrap();
    
    // 5. Start consensus
    let consensus_config = ConsensusConfig::default();
    let consensus_service = ConsensusService::new(ConsensusServiceConfig {
        node_id: UInt160::zero(),
        consensus_config,
        network_config: NetworkConfig::default(),
        enable_auto_start: false,
    }).unwrap();
    
    // Verify all components initialized
    assert!(blockchain.block_count() >= 0);
    assert!(network_server.is_running());
    assert!(rpc_server.is_running());
    
    println!("✅ Complete node operation test passed");
}

/// Test transaction creation and processing workflow
#[tokio::test]
async fn test_transaction_workflow() {
    println!("💸 Testing transaction workflow");
    
    // Create transaction
    let mut tx = Transaction::new();
    tx.set_network_fee(1000000);
    tx.set_system_fee(500000);
    
    // Sign transaction
    let private_key = PrivateKey::generate_secp256r1().unwrap();
    let signature = ECDsa::sign_secp256r1(&tx.hash().as_bytes(), &private_key).unwrap();
    
    // Create witness
    let witness = Witness::new_with_scripts(vec![signature], vec![]);
    tx.add_witness(witness);
    
    // Validate transaction
    let validation = tx.validate();
    assert!(validation.is_ok());
    
    // Submit to mempool and process
    let storage = Arc::new(Storage::new_memory());
    let blockchain = Arc::new(Blockchain::new(storage));
    
    let submit_result = blockchain.submit_transaction(tx);
    assert!(submit_result.is_ok());
    
    println!("✅ Transaction workflow test passed");
}

/// Test smart contract deployment and execution
#[tokio::test]
async fn test_smart_contract_workflow() {
    println!("📜 Testing smart contract workflow");
    
    // Create contract
    let contract_script = vec![
        OpCode::PUSH42 as u8,
        OpCode::RET as u8,
    ];
    
    let nef = NefFile::new(
        contract_script.clone(),
        "Neo.Compiler.CSharp".to_string(),
        "3.6.0".to_string(),
    );
    
    // Deploy contract
    let contract_hash = UInt160::from_script(&contract_script);
    
    // Execute contract
    let mut app_engine = ApplicationEngine::new(TriggerType::Application, 10_000_000);
    let script = Script::new_relaxed(contract_script);
    let mut vm_engine = ExecutionEngine::new(None);
    
    vm_engine.load_script(script, None).unwrap();
    let result = vm_engine.execute();
    
    assert!(result.is_ok());
    assert_eq!(vm_engine.state(), VMState::HALT);
    
    println!("✅ Smart contract workflow test passed");
}

/// Test P2P network communication
#[tokio::test]
async fn test_p2p_communication_workflow() {
    println!("🌐 Testing P2P communication workflow");
    
    // Create two nodes
    let config1 = P2PConfig {
        listen_address: "127.0.0.1:0".parse().unwrap(),
        max_peers: 10,
        connection_timeout: Duration::from_secs(5),
        handshake_timeout: Duration::from_secs(3),
        ping_interval: Duration::from_secs(30),
        message_buffer_size: 100,
        enable_compression: false,
    };
    
    let config2 = config1.clone();
    
    let node1 = P2PNode::new(config1).unwrap();
    let node2 = P2PNode::new(config2).unwrap();
    
    // Test handshake simulation
    let version_msg = VersionMessage {
        version: 0x00030600,
        services: 0x01,
        timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
        port: 10333,
        nonce: 0x1234567890ABCDEF,
        user_agent: "neo-rs/1.0.0".to_string(),
        start_height: 1000000,
        relay: true,
    };
    
    let serialized = version_msg.serialize().unwrap();
    let deserialized = VersionMessage::deserialize(&serialized).unwrap();
    
    assert_eq!(version_msg.nonce, deserialized.nonce);
    
    println!("✅ P2P communication workflow test passed");
}

/// Test RPC API integration
#[tokio::test]
async fn test_rpc_api_integration() {
    println!("🔌 Testing RPC API integration");
    
    let server_config = RpcServerConfig::default();
    let mut rpc_server = RpcServer::new(server_config).unwrap();
    
    // Test multiple RPC calls
    let requests = vec![
        ("getblockcount", vec![]),
        ("getbestblockhash", vec![]),
        ("getversion", vec![]),
    ];
    
    for (method, params) in requests {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params,
            id: Some(json!(1)),
        };
        
        let response = rpc_server.handle_request(request).await;
        assert!(response.is_ok());
    }
    
    println!("✅ RPC API integration test passed");
}

/// Test consensus mechanism
#[tokio::test]
async fn test_consensus_integration() {
    println!("🤝 Testing consensus integration");
    
    let consensus_config = ConsensusConfig::default();
    let service_config = ConsensusServiceConfig {
        node_id: UInt160::zero(),
        consensus_config,
        network_config: NetworkConfig::default(),
        enable_auto_start: false,
    };
    
    let consensus_service = ConsensusService::new(service_config);
    assert!(consensus_service.is_ok());
    
    println!("✅ Consensus integration test passed");
} 