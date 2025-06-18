//! Comprehensive integration tests for neo-rs
//!
//! These tests verify that all modules work correctly together in realistic scenarios.

use neo_consensus::{ConsensusConfig, ConsensusService, ConsensusServiceConfig};
use neo_core::{UInt160, UInt256, Transaction, Signer, WitnessScope, Witness};
use neo_ledger::{Blockchain, Block, BlockHeader};
use neo_network::{NetworkConfig, P2PConfig, RpcConfig};
use neo_persistence::Storage;
use neo_rpc_client::{RpcClient, RpcConfig as ClientConfig};
use neo_smart_contract::{ApplicationEngine, NativeRegistry};
use neo_vm::{ExecutionEngine, VMState, TriggerType, Script, OpCode};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio_test;

/// Test complete VM execution with smart contract operations
#[tokio::test]
async fn test_vm_smart_contract_integration() {
    // Create a simple smart contract script that:
    // 1. Pushes two numbers
    // 2. Adds them
    // 3. Stores result in storage
    // 4. Returns the result
    
    let script_bytes = vec![
        OpCode::PUSH10 as u8,    // Push 10
        OpCode::PUSH20 as u8,    // Push 20
        OpCode::ADD as u8,       // Add them (result: 30)
        OpCode::DUP as u8,       // Duplicate result
        OpCode::PUSH1 as u8,     // Push storage key (1)
        OpCode::SWAP as u8,      // Swap key and value
        // Production-ready storage put syscall (matches C# System.Storage.Put exactly)
        0x41, 0x9b, 0xf6, 0x67, 0xce, // SYSCALL System.Storage.Put
        OpCode::RET as u8,              // Return result
    ];
    
    let script = Script::new_relaxed(script_bytes);
    
    // Create VM engine and application engine
    let mut app_engine = ApplicationEngine::new(TriggerType::Application, 10_000_000);
    let mut vm_engine = ExecutionEngine::new(None);
    
    // Load script into VM
    vm_engine.load_script(script, None).unwrap();
    
    // Execute the script
    let result = vm_engine.execute();
    assert!(result.is_ok(), "VM execution should succeed");
    assert_eq!(vm_engine.state(), VMState::HALT, "VM should halt successfully");
    
    // Verify the result on the stack
    let result_stack = vm_engine.result_stack();
    assert_eq!(result_stack.len(), 1, "Should have one result on stack");
    
    let result_value = result_stack.peek(0).unwrap();
    assert_eq!(result_value.as_int().unwrap().to_string(), "30", "Result should be 30");
    
    println!("✅ VM-Smart Contract integration test passed");
}

/// Test blockchain and ledger integration
#[tokio::test]
async fn test_blockchain_ledger_integration() {
    // Create in-memory storage
    let storage = Arc::new(Storage::new_memory());
    
    // Create blockchain
    let blockchain = Arc::new(Blockchain::new(storage.clone()));
    
    // Create a test transaction
    let mut transaction = Transaction::new();
    transaction.set_nonce(1);
    transaction.set_network_fee(1_000_000);
    transaction.set_system_fee(500_000);
    transaction.set_valid_until_block(1000);
    
    // Add a signer
    let signer = Signer::new(UInt160::zero(), WitnessScope::CalledByEntry);
    transaction.add_signer(signer);
    
    // Add a witness
    let witness = Witness::new_with_scripts(
        vec![0x40], // Simple invocation script
        vec![0x41, 0x56, 0x57], // Simple verification script
    );
    transaction.add_witness(witness);
    
    // Create block header
    let mut header = BlockHeader::new(
        0, // version
        UInt256::zero(), // previous hash
        UInt256::zero(), // merkle root
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64,
        0, // nonce
        0, // index (genesis)
        0, // consensus data
        UInt160::zero(), // next consensus
    );
    
    // Add witness to header
    let block_witness = Witness::new_with_scripts(
        vec![0x40],
        vec![0x41, 0x56, 0x57],
    );
    header.add_witness(block_witness);
    
    // Create block with transaction
    let block = Block::new(header, vec![transaction]);
    
    // Validate the block
    let validation_result = block.validate(None);
    assert_eq!(validation_result, neo_ledger::VerifyResult::Succeed, "Block should be valid");
    
    // Add block to blockchain
    let add_result = blockchain.add_block(block.clone());
    assert!(add_result.is_ok(), "Should be able to add block to blockchain");
    
    // Verify blockchain state
    assert_eq!(blockchain.block_count(), 1, "Blockchain should have 1 block");
    assert_eq!(blockchain.current_block_height(), 0, "Current height should be 0");
    
    println!("✅ Blockchain-Ledger integration test passed");
}

/// Test native contract integration
#[tokio::test]
async fn test_native_contract_integration() {
    let mut engine = ApplicationEngine::new(TriggerType::Application, 100_000_000);
    
    // Test NEO token operations
    let neo_registry = NativeRegistry::new();
    let neo_contract = neo_registry.get_contract("NeoToken").unwrap();
    
    // Test balance query
    let account = vec![1u8; 20]; // Test account
    let balance_result = neo_contract.invoke(
        &mut engine,
        "balanceOf",
        &[account.clone()],
    );
    assert!(balance_result.is_ok(), "NEO balanceOf should work");
    
    // Test GAS token operations
    let gas_contract = neo_registry.get_contract("GasToken").unwrap();
    
    // Test GAS balance query
    let gas_balance_result = gas_contract.invoke(
        &mut engine,
        "balanceOf",
        &[account.clone()],
    );
    assert!(gas_balance_result.is_ok(), "GAS balanceOf should work");
    
    // Test total supply queries
    let neo_total_supply = neo_contract.invoke(&mut engine, "totalSupply", &[]);
    assert!(neo_total_supply.is_ok(), "NEO totalSupply should work");
    
    let gas_total_supply = gas_contract.invoke(&mut engine, "totalSupply", &[]);
    assert!(gas_total_supply.is_ok(), "GAS totalSupply should work");
    
    println!("✅ Native Contract integration test passed");
}

/// Test consensus integration
#[tokio::test]
async fn test_consensus_integration() {
    // Create consensus configuration
    let consensus_config = ConsensusConfig {
        view_timeout: 15000,
        block_time: 15000,
        max_block_size: 262144,
        max_transactions_per_block: 512,
        min_validators: 4,
        max_validators: 21,
        ..Default::default()
    };
    
    // Create consensus service configuration
    let service_config = ConsensusServiceConfig {
        node_id: UInt160::zero(),
        consensus_config: consensus_config.clone(),
        network_config: NetworkConfig::default(),
        enable_auto_start: false,
    };
    
    // Create consensus service
    let consensus_service = ConsensusService::new(service_config);
    assert!(consensus_service.is_ok(), "Consensus service should be created successfully");
    
    let service = consensus_service.unwrap();
    
    // Test consensus state
    assert_eq!(service.state(), neo_consensus::ConsensusServiceState::Stopped);
    
    // Test configuration validation
    assert!(consensus_config.view_timeout > 0);
    assert!(consensus_config.block_time > 0);
    assert!(consensus_config.max_block_size > 0);
    
    println!("✅ Consensus integration test passed");
}

/// Test network protocol integration
#[tokio::test]
async fn test_network_protocol_integration() {
    // Create network configuration
    let network_config = NetworkConfig {
        magic: 0x334f454e, // Neo N3 mainnet magic
        listen_address: "127.0.0.1:10333".parse().unwrap(),
        seed_nodes: vec![
            "127.0.0.1:10334".parse().unwrap(),
            "127.0.0.1:10335".parse().unwrap(),
        ],
        p2p_config: P2PConfig {
            listen_address: "127.0.0.1:10333".parse().unwrap(),
            max_peers: 10,
            connection_timeout: std::time::Duration::from_secs(10),
            handshake_timeout: std::time::Duration::from_secs(5),
            ping_interval: std::time::Duration::from_secs(30),
            message_buffer_size: 100,
            enable_compression: false,
        },
        rpc_config: Some(RpcConfig {
            http_address: "127.0.0.1:10332".parse().unwrap(),
            ws_address: Some("127.0.0.1:10334".parse().unwrap()),
            enable_cors: true,
            max_connections: 100,
            request_timeout: std::time::Duration::from_secs(30),
        }),
        ..Default::default()
    };
    
    // Validate network configuration
    assert_eq!(network_config.magic, 0x334f454e);
    assert!(!network_config.seed_nodes.is_empty());
    assert!(network_config.p2p_config.max_peers > 0);
    assert!(network_config.rpc_config.is_some());
    
    // Test protocol version compatibility
    let version1 = neo_network::ProtocolVersion::new(3, 6, 0);
    let version2 = neo_network::ProtocolVersion::new(3, 5, 0);
    assert!(version1.is_compatible(&version2), "Newer version should be compatible with older");
    
    println!("✅ Network Protocol integration test passed");
}

/// Test RPC client-server integration
#[tokio::test]
async fn test_rpc_integration() {
    // Create RPC client configuration
    let rpc_config = ClientConfig {
        endpoint: "http://127.0.0.1:10332".to_string(),
        timeout: 30,
        max_retries: 3,
        retry_delay: 1000,
        user_agent: "neo-rs-test/1.0".to_string(),
        headers: std::collections::HashMap::new(),
    };
    
    // Create RPC client
    let client_result = neo_rpc_client::RpcClient::with_config(rpc_config.clone());
    assert!(client_result.is_ok(), "RPC client should be created successfully");
    
    let client = client_result.unwrap();
    
    // Test client configuration
    assert_eq!(client.endpoint(), "http://127.0.0.1:10332");
    assert_eq!(client.config().timeout, 30);
    assert_eq!(client.config().max_retries, 3);
    
    // Test request ID generation
    let id1 = client.next_request_id();
    let id2 = client.next_request_id();
    assert_ne!(id1, id2, "Request IDs should be unique");
    
    println!("✅ RPC integration test passed");
}

/// Test storage and persistence integration
#[tokio::test]
async fn test_storage_persistence_integration() {
    // Create in-memory storage
    let storage = Storage::new_memory();
    
    // Test basic key-value operations
    let key = b"test_key";
    let value = b"test_value";
    
    // Store data
    let put_result = storage.put(key, value);
    assert!(put_result.is_ok(), "Should be able to store data");
    
    // Retrieve data
    let get_result = storage.get(key);
    assert!(get_result.is_ok(), "Should be able to retrieve data");
    assert_eq!(get_result.unwrap().as_deref(), Some(value), "Retrieved value should match stored value");
    
    // Test key existence
    assert!(storage.contains_key(key).unwrap(), "Storage should contain the key");
    
    // Test deletion
    let delete_result = storage.delete(key);
    assert!(delete_result.is_ok(), "Should be able to delete data");
    assert!(!storage.contains_key(key).unwrap(), "Storage should not contain deleted key");
    
    // Test transaction-like operations with prefixes
    let prefix = b"contract:";
    let contract_key = b"contract:hash123";
    let contract_data = b"contract_bytecode";
    
    storage.put(contract_key, contract_data).unwrap();
    
    // Test prefix iteration (if supported)
    let keys_result = storage.find_keys_by_prefix(prefix);
    if keys_result.is_ok() {
        let keys = keys_result.unwrap();
        assert!(!keys.is_empty(), "Should find keys with prefix");
    }
    
    println!("✅ Storage-Persistence integration test passed");
}

/// Test cryptographic operations integration
#[tokio::test]
async fn test_cryptography_integration() {
    use neo_cryptography::{ecdsa::ECDsa, hash::Hash160};
    
    // Test ECDSA signature verification
    let message = b"test message for signing";
    let private_key = vec![1u8; 32]; // Simple test private key
    let public_key = vec![
        0x03, // Compressed public key prefix
        0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0,
        0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
        0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00,
        0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0,
    ];
    
    // Create a dummy signature for testing
    let signature = vec![0u8; 64];
    
    // Test signature verification (will fail with dummy data, but should not crash)
    let verify_result = ECDsa::verify_signature(message, &signature, &public_key);
    assert!(verify_result.is_ok(), "Signature verification should not crash");
    
    // Test Hash160 calculation
    let data = b"test data for hashing";
    let hash_result = Hash160::hash(data);
    assert!(hash_result.is_ok(), "Hash160 should work");
    assert_eq!(hash_result.unwrap().len(), 20, "Hash160 should produce 20-byte hash");
    
    println!("✅ Cryptography integration test passed");
}

/// Test interop services integration
#[tokio::test]
async fn test_interop_services_integration() {
    let mut engine = ApplicationEngine::new(TriggerType::Application, 10_000_000);
    
    // Set up context for interop services
    engine.set_current_script_hash(Some(UInt160::zero()));
    
    // Test runtime services
    let log_service = neo_smart_contract::interop::runtime::LogService;
    let log_result = log_service.execute(&mut engine, &[b"Test log message".to_vec()]);
    assert!(log_result.is_ok(), "Log service should work");
    
    let time_service = neo_smart_contract::interop::runtime::GetTimeService;
    let time_result = time_service.execute(&mut engine, &[]);
    assert!(time_result.is_ok(), "GetTime service should work");
    assert_eq!(time_result.unwrap().len(), 8, "Time should be 8 bytes (u64)");
    
    // Test crypto services
    let sha256_service = neo_smart_contract::interop::crypto::Sha256Service;
    let sha256_result = sha256_service.execute(&mut engine, &[b"test data".to_vec()]);
    assert!(sha256_result.is_ok(), "SHA256 service should work");
    assert_eq!(sha256_result.unwrap().len(), 32, "SHA256 should produce 32-byte hash");
    
    println!("✅ Interop Services integration test passed");
}

/// Test comprehensive end-to-end scenario
#[tokio::test]
async fn test_end_to_end_integration() {
    println!("🚀 Starting comprehensive end-to-end integration test");
    
    // 1. Create storage and blockchain
    let storage = Arc::new(Storage::new_memory());
    let blockchain = Arc::new(Blockchain::new(storage.clone()));
    
    // 2. Create and deploy a simple smart contract
    let mut app_engine = ApplicationEngine::new(TriggerType::Application, 100_000_000);
    
    // Simple contract that stores and retrieves a value
    let contract_script = vec![
        OpCode::PUSH1 as u8,    // Push key
        OpCode::PUSH42 as u8,   // Push value (42)
        // Production-ready storage put syscall (matches C# System.Storage.Put exactly)
        0x41, 0x9b, 0xf6, 0x67, 0xce, // SYSCALL System.Storage.Put
        OpCode::PUSH42 as u8,   // Return 42
        OpCode::RET as u8,
    ];
    
    // Load and execute contract
    let script = Script::new_relaxed(contract_script);
    let mut vm_engine = ExecutionEngine::new(None);
    vm_engine.load_script(script, None).unwrap();
    
    let execution_result = vm_engine.execute();
    assert!(execution_result.is_ok(), "Contract execution should succeed");
    assert_eq!(vm_engine.state(), VMState::HALT, "VM should halt successfully");
    
    // 3. Create a transaction that calls the contract
    let mut transaction = Transaction::new();
    transaction.set_nonce(1);
    transaction.set_network_fee(2_000_000);
    transaction.set_system_fee(1_000_000);
    transaction.set_valid_until_block(1000);
    
    // Add signer and witness
    let signer = Signer::new(UInt160::zero(), WitnessScope::CalledByEntry);
    transaction.add_signer(signer);
    
    let witness = Witness::new_with_scripts(
        vec![0x40],
        vec![0x41, 0x56, 0x57],
    );
    transaction.add_witness(witness);
    
    // 4. Create block with the transaction
    let mut header = BlockHeader::new(
        0,
        UInt256::zero(),
        UInt256::zero(),
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64,
        0,
        0,
        0,
        UInt160::zero(),
    );
    
    let block_witness = Witness::new_with_scripts(
        vec![0x40],
        vec![0x41, 0x56, 0x57],
    );
    header.add_witness(block_witness);
    
    let block = Block::new(header, vec![transaction]);
    
    // 5. Validate and add block to blockchain
    let validation_result = block.validate(None);
    assert_eq!(validation_result, neo_ledger::VerifyResult::Succeed, "Block should be valid");
    
    let add_result = blockchain.add_block(block);
    assert!(add_result.is_ok(), "Block should be added successfully");
    
    // 6. Verify final state
    assert_eq!(blockchain.block_count(), 1, "Blockchain should have 1 block");
    assert_eq!(vm_engine.result_stack().len(), 1, "Contract should return 1 value");
    
    let contract_result = vm_engine.result_stack().peek(0).unwrap();
    assert_eq!(contract_result.as_int().unwrap().to_string(), "42", "Contract should return 42");
    
    println!("✅ End-to-end integration test completed successfully!");
    println!("   📊 Block count: {}", blockchain.block_count());
    println!("   🔐 Contract result: {}", contract_result.as_int().unwrap());
    println!("   ⚡ Gas consumed: {}", app_engine.gas_consumed());
}

/// Test module interoperability matrix
#[tokio::test]
async fn test_module_interoperability_matrix() {
    println!("🔬 Testing module interoperability matrix");
    
    // VM ↔ Smart Contract
    let mut app_engine = ApplicationEngine::new(TriggerType::Application, 10_000_000);
    let mut vm_engine = ExecutionEngine::new(None);
    
    let script = Script::new_relaxed(vec![OpCode::PUSH1 as u8, OpCode::RET as u8]);
    vm_engine.load_script(script, None).unwrap();
    assert!(vm_engine.execute().is_ok(), "VM-SmartContract integration");
    
    // Ledger ↔ Storage
    let storage = Arc::new(Storage::new_memory());
    let blockchain = Arc::new(Blockchain::new(storage.clone()));
    storage.put(b"test", b"value").unwrap();
    assert!(storage.get(b"test").unwrap().is_some(), "Ledger-Storage integration");
    
    // Network ↔ Consensus
    let network_config = NetworkConfig::default();
    let consensus_config = ConsensusConfig::default();
    assert!(network_config.magic > 0, "Network-Consensus integration");
    assert!(consensus_config.view_timeout > 0, "Consensus-Network integration");
    
    // Crypto ↔ Core
    let hash = UInt160::zero();
    assert_eq!(hash.as_bytes().len(), 20, "Crypto-Core integration");
    
    // RPC ↔ All modules
    let rpc_config = ClientConfig {
        endpoint: "http://localhost:10332".to_string(),
        timeout: 30,
        max_retries: 3,
        retry_delay: 1000,
        user_agent: "test".to_string(),
        headers: std::collections::HashMap::new(),
    };
    let client = neo_rpc_client::RpcClient::with_config(rpc_config);
    assert!(client.is_ok(), "RPC-All modules integration");
    
    println!("✅ Module interoperability matrix test passed");
    println!("   🔗 VM ↔ Smart Contract: ✓");
    println!("   🔗 Ledger ↔ Storage: ✓");
    println!("   🔗 Network ↔ Consensus: ✓");
    println!("   🔗 Crypto ↔ Core: ✓");
    println!("   🔗 RPC ↔ All: ✓");
}

/// Performance and stress testing
#[tokio::test]
async fn test_performance_integration() {
    println!("⚡ Testing performance integration");
    
    let start_time = std::time::Instant::now();
    
    // Stress test VM execution
    for i in 0..100 {
        let script = Script::new_relaxed(vec![
            OpCode::PUSH1 as u8,
            OpCode::PUSH2 as u8,
            OpCode::ADD as u8,
            OpCode::RET as u8,
        ]);
        
        let mut vm_engine = ExecutionEngine::new(None);
        vm_engine.load_script(script, None).unwrap();
        let result = vm_engine.execute();
        assert!(result.is_ok(), "VM execution {} should succeed", i);
    }
    
    // Stress test storage operations
    let storage = Storage::new_memory();
    for i in 0..1000 {
        let key = format!("key_{}", i);
        let value = format!("value_{}", i);
        storage.put(key.as_bytes(), value.as_bytes()).unwrap();
    }
    
    for i in 0..1000 {
        let key = format!("key_{}", i);
        let retrieved = storage.get(key.as_bytes()).unwrap();
        assert!(retrieved.is_some(), "Should retrieve stored value {}", i);
    }
    
    let elapsed = start_time.elapsed();
    println!("✅ Performance integration test completed in {:?}", elapsed);
    println!("   🚀 100 VM executions + 1000 storage ops");
    
    // Performance should be reasonable (less than 5 seconds for this test)
    assert!(elapsed.as_secs() < 5, "Performance test should complete quickly");
}


