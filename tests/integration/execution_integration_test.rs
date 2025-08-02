//! Block and Transaction Execution Integration Tests
//! 
//! These tests verify the complete execution functionality including:
//! - Transaction validation and execution
//! - Block validation and state transitions
//! - Smart contract execution
//! - State persistence and rollback
//! - Gas calculation and limits

use crate::test_mocks::{
    ledger::{Blockchain, Block, BlockHeader, MemoryPool},
    Transaction, Witness, Signer, WitnessScope,
    vm::{Script, OpCode, StackItem, ExecutionEngine},
    smart_contract::{
        ApplicationEngine, Contract, ContractState, 
        NefFile, ContractManifest, TriggerType
    },
};
use neo_core::{UInt160, UInt256};
use neo_config::NetworkType;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::time::Duration;

/// Test basic transaction execution
#[tokio::test]
async fn test_transaction_execution() {
    let _ = env_logger::builder().is_test(true).try_init();
    
    // Create blockchain
    let blockchain = create_test_blockchain().await;
    
    // Create a simple transfer transaction
    let from_account = create_test_account();
    let to_account = create_test_account();
    let amount = 1000_00000000i64; // 1000 NEO
    
    // Create transaction script
    let script = create_transfer_script(&from_account, &to_account, amount);
    
    let transaction = Transaction {
        version: 0,
        nonce: rand::random(),
        system_fee: 100000,
        network_fee: 100000,
        valid_until_block: 1000,
        signers: vec![Signer {
            account: from_account,
            scopes: WitnessScope::CalledByEntry,
        }],
        attributes: vec![],
        script,
        witnesses: vec![create_test_witness(&from_account)],
    };
    
    // Execute transaction
    let engine = ApplicationEngine::new(
        TriggerType::Application,
        &transaction,
        blockchain.clone(),
        None,
        10_000_000, // 0.1 GAS limit
    );
    
    let result = engine.execute().await;
    assert!(result.is_ok(), "Transaction execution failed: {:?}", result);
    
    // Verify state changes
    let from_balance = engine.get_balance(&from_account, &neo_token_hash()).await;
    let to_balance = engine.get_balance(&to_account, &neo_token_hash()).await;
    
    assert_eq!(from_balance, initial_balance() - amount - 200000); // Minus fees
    assert_eq!(to_balance, amount);
    
    // Verify gas consumption
    let gas_consumed = engine.get_gas_consumed();
    assert!(gas_consumed > 0 && gas_consumed < 10_000_000, 
            "Unexpected gas consumption: {}", gas_consumed);
}

/// Test block execution with multiple transactions
#[tokio::test]
async fn test_block_execution() {
    let _ = env_logger::builder().is_test(true).try_init();
    
    // Create blockchain
    let blockchain = create_test_blockchain().await;
    let mempool = Arc::new(RwLock::new(MemoryPool::new()));
    
    // Create multiple transactions
    let mut transactions = Vec::new();
    for i in 0..10 {
        let tx = create_test_transaction(i);
        transactions.push(tx.clone());
        mempool.write().await.add_transaction(tx).await.unwrap();
    }
    
    // Create block
    let prev_block = blockchain.get_current_block().await.unwrap();
    let block = Block {
        header: BlockHeader {
            version: 0,
            prev_hash: prev_block.hash(),
            merkle_root: calculate_merkle_root(&transactions),
            timestamp: chrono::Utc::now().timestamp() as u64,
            index: prev_block.header.index + 1,
            primary_index: 0,
            next_consensus: create_test_account(),
            witness: create_test_witness(&create_test_account()),
        },
        transactions,
    };
    
    // Execute block
    let result = blockchain.add_block(block.clone()).await;
    assert!(result.is_ok(), "Block execution failed: {:?}", result);
    
    // Verify block was persisted
    let stored_block = blockchain
        .get_block_by_index(block.header.index)
        .await.unwrap().unwrap();
    assert_eq!(stored_block.hash(), block.hash());
    
    // Verify transactions were removed from mempool
    assert_eq!(mempool.read().await.size(), 0);
    
    // Verify state changes from all transactions
    for (i, tx) in block.transactions.iter().enumerate() {
        let receipt = blockchain
            .get_transaction_receipt(&tx.hash().unwrap())
            .await.unwrap().unwrap();
        assert!(receipt.vm_state.is_success(), 
                "Transaction {} failed", i);
    }
}

/// Test smart contract deployment and execution
#[tokio::test]
async fn test_smart_contract_execution() {
    let _ = env_logger::builder().is_test(true).try_init();
    
    // Create blockchain
    let blockchain = create_test_blockchain().await;
    
    // Create contract deployment transaction
    let contract_owner = create_test_account();
    let (nef, manifest) = create_test_contract();
    
    let deploy_script = create_deploy_script(&nef, &manifest);
    let deploy_tx = Transaction {
        version: 0,
        nonce: rand::random(),
        system_fee: 10_000_000_000, // 100 GAS for deployment
        network_fee: 1_000_000,
        valid_until_block: 1000,
        signers: vec![Signer {
            account: contract_owner,
            scopes: WitnessScope::Global,
        }],
        attributes: vec![],
        script: deploy_script,
        witnesses: vec![create_test_witness(&contract_owner)],
    };
    
    // Deploy contract
    let engine = ApplicationEngine::new(
        TriggerType::Application,
        &deploy_tx,
        blockchain.clone(),
        None,
        100_000_000_000,
    );
    
    let deploy_result = engine.execute().await;
    assert!(deploy_result.is_ok(), "Contract deployment failed: {:?}", deploy_result);
    
    let contract_hash = engine.get_deployed_contract_hash();
    assert!(contract_hash.is_some(), "Contract hash not found after deployment");
    
    // Invoke contract method
    let invoke_script = create_invoke_script(&contract_hash.unwrap(), "add", vec![10, 20]);
    let invoke_tx = Transaction {
        version: 0,
        nonce: rand::random(),
        system_fee: 1_000_000,
        network_fee: 100_000,
        valid_until_block: 2000,
        signers: vec![Signer {
            account: contract_owner,
            scopes: WitnessScope::CalledByEntry,
        }],
        attributes: vec![],
        script: invoke_script,
        witnesses: vec![create_test_witness(&contract_owner)],
    };
    
    // Execute contract
    let engine2 = ApplicationEngine::new(
        TriggerType::Application,
        &invoke_tx,
        blockchain.clone(),
        None,
        10_000_000,
    );
    
    let invoke_result = engine2.execute().await;
    assert!(invoke_result.is_ok(), "Contract invocation failed: {:?}", invoke_result);
    
    // Verify result
    let stack = engine2.get_result_stack();
    assert_eq!(stack.len(), 1);
    if let StackItem::Integer(value) = &stack[0] {
        assert_eq!(*value, 30, "Contract returned wrong result");
    } else {
        panic!("Contract didn't return integer");
    }
}

/// Test transaction validation rules
#[tokio::test]
async fn test_transaction_validation() {
    let _ = env_logger::builder().is_test(true).try_init();
    
    let blockchain = create_test_blockchain().await;
    
    // Test 1: Invalid witness
    let mut tx = create_test_transaction(1);
    tx.witnesses[0].verification_script = vec![0xFF]; // Invalid script
    
    let result = blockchain.verify_transaction(&tx).await;
    assert!(result.is_err(), "Transaction with invalid witness should fail");
    
    // Test 2: Expired transaction
    let mut tx2 = create_test_transaction(2);
    tx2.valid_until_block = 0; // Already expired
    
    let result2 = blockchain.verify_transaction(&tx2).await;
    assert!(result2.is_err(), "Expired transaction should fail");
    
    // Test 3: Insufficient fees
    let mut tx3 = create_test_transaction(3);
    tx3.system_fee = 0;
    tx3.network_fee = 0;
    
    let result3 = blockchain.verify_transaction(&tx3).await;
    assert!(result3.is_err(), "Transaction with zero fees should fail");
    
    // Test 4: Script too large
    let mut tx4 = create_test_transaction(4);
    tx4.script = vec![0x00; 100_000]; // Too large
    
    let result4 = blockchain.verify_transaction(&tx4).await;
    assert!(result4.is_err(), "Transaction with oversized script should fail");
    
    // Test 5: Valid transaction
    let tx5 = create_test_transaction(5);
    let result5 = blockchain.verify_transaction(&tx5).await;
    assert!(result5.is_ok(), "Valid transaction should pass");
}

/// Test state rollback on execution failure
#[tokio::test]
async fn test_execution_rollback() {
    let _ = env_logger::builder().is_test(true).try_init();
    
    let blockchain = create_test_blockchain().await;
    
    // Create account with initial balance
    let account = create_test_account();
    let initial_balance = 1000_00000000i64;
    blockchain.set_balance(&account, &neo_token_hash(), initial_balance).await;
    
    // Create transaction that will fail mid-execution
    let script = vec![
        OpCode::PUSH2.to_u8(),    // Push 2
        OpCode::PUSH3.to_u8(),    // Push 3
        OpCode::ADD.to_u8(),      // Add: 2 + 3 = 5
        OpCode::PUSH10.to_u8(),   // Push 10
        OpCode::LT.to_u8(),       // 5 < 10 = true
        OpCode::ASSERT.to_u8(),   // Pass
        OpCode::PUSH0.to_u8(),    // Push 0
        OpCode::PUSH1.to_u8(),    // Push 1
        OpCode::DIV.to_u8(),      // 1 / 0 = Error!
    ];
    
    let tx = Transaction {
        version: 0,
        nonce: rand::random(),
        system_fee: 1_000_000,
        network_fee: 100_000,
        valid_until_block: 1000,
        signers: vec![Signer {
            account,
            scopes: WitnessScope::None,
        }],
        attributes: vec![],
        script,
        witnesses: vec![create_test_witness(&account)],
    };
    
    // Execute transaction
    let engine = ApplicationEngine::new(
        TriggerType::Application,
        &tx,
        blockchain.clone(),
        None,
        10_000_000,
    );
    
    let result = engine.execute().await;
    assert!(result.is_err(), "Transaction should fail on division by zero");
    
    // Verify state was rolled back
    let final_balance = blockchain.get_balance(&account, &neo_token_hash()).await;
    assert_eq!(final_balance, initial_balance, "State should be rolled back");
    
    // Verify fees were still consumed
    let gas_consumed = engine.get_gas_consumed();
    assert!(gas_consumed > 0, "Gas should be consumed even on failure");
}

/// Test gas calculation and limits
#[tokio::test]
async fn test_gas_limits() {
    let _ = env_logger::builder().is_test(true).try_init();
    
    let blockchain = create_test_blockchain().await;
    
    // Test 1: Transaction exceeding gas limit
    let expensive_script = vec![
        OpCode::PUSH0.to_u8(),
        OpCode::PUSH1.to_u8(),
        // Loop 1000 times
        OpCode::PUSH2.to_u8(),
        OpCode::PUSH10.to_u8(),
        OpCode::PUSH10.to_u8(),
        OpCode::MUL.to_u8(),
        OpCode::MUL.to_u8(), // 1000
        OpCode::JMPIF.to_u8(), 0xFA, // Jump back if counter > 0
    ];
    
    let tx = create_transaction_with_script(expensive_script);
    
    let engine = ApplicationEngine::new(
        TriggerType::Application,
        &tx,
        blockchain.clone(),
        None,
        100_000, // Very low gas limit
    );
    
    let result = engine.execute().await;
    assert!(result.is_err(), "Transaction should exceed gas limit");
    
    // Test 2: Measure gas for different operations
    let operations = vec![
        ("ADD", vec![OpCode::PUSH1.to_u8(), OpCode::PUSH2.to_u8(), OpCode::ADD.to_u8()]),
        ("MUL", vec![OpCode::PUSH10.to_u8(), OpCode::PUSH10.to_u8(), OpCode::MUL.to_u8()]),
        ("SHA256", vec![OpCode::PUSH1.to_u8(), OpCode::SHA256.to_u8()]),
    ];
    
    for (name, script) in operations {
        let tx = create_transaction_with_script(script);
        let engine = ApplicationEngine::new(
            TriggerType::Application,
            &tx,
            blockchain.clone(),
            None,
            10_000_000,
        );
        
        let _ = engine.execute().await;
        let gas = engine.get_gas_consumed();
        println!("{} operation consumed {} gas", name, gas);
        assert!(gas > 0 && gas < 1_000_000, "Unexpected gas for {}", name);
    }
}

/// Test concurrent transaction execution
#[tokio::test]
async fn test_concurrent_execution() {
    let _ = env_logger::builder().is_test(true).try_init();
    
    let blockchain = Arc::new(create_test_blockchain().await);
    
    // Create multiple independent transactions
    let mut handles = Vec::new();
    let tx_count = 10;
    
    for i in 0..tx_count {
        let blockchain_clone = blockchain.clone();
        let handle = tokio::spawn(async move {
            let tx = create_test_transaction(i);
            
            let engine = ApplicationEngine::new(
                TriggerType::Application,
                &tx,
                blockchain_clone,
                None,
                10_000_000,
            );
            
            engine.execute().await
        });
        handles.push(handle);
    }
    
    // Wait for all executions
    let mut results = Vec::new();
    for handle in handles {
        results.push(handle.await.unwrap());
    }
    
    // Verify all succeeded
    for (i, result) in results.iter().enumerate() {
        assert!(result.is_ok(), "Transaction {} failed: {:?}", i, result);
    }
}

// Helper functions

async fn create_test_blockchain() -> Blockchain {
    Blockchain::new(
        NetworkType::TestNet,
        "/tmp/neo-test-execution",
    ).await.unwrap()
}

fn create_test_account() -> UInt160 {
    UInt160::from_bytes(&rand::random::<[u8; 20]>()).unwrap()
}

fn create_test_transaction(nonce: u32) -> Transaction {
    Transaction {
        version: 0,
        nonce,
        system_fee: 100_000,
        network_fee: 100_000,
        valid_until_block: 10000,
        signers: vec![Signer {
            account: create_test_account(),
            scopes: WitnessScope::CalledByEntry,
        }],
        attributes: vec![],
        script: vec![OpCode::PUSH1.to_u8()], // Simple script
        witnesses: vec![create_test_witness(&create_test_account())],
    }
}

fn create_test_witness(account: &UInt160) -> Witness {
    Witness {
        invocation_script: vec![0x00; 64], // Dummy signature
        verification_script: vec![
            OpCode::PUSH1.to_u8(),
            OpCode::PUSHNULL.to_u8(),
            OpCode::SYSCALL.to_u8(),
        ],
    }
}

fn create_transfer_script(from: &UInt160, to: &UInt160, amount: i64) -> Vec<u8> {
    // NEP-17 transfer script
    let mut script = Vec::new();
    
    // Push amount
    script.extend(&amount.to_le_bytes());
    script.push(OpCode::PUSHINT64.to_u8());
    
    // Push to address
    script.push(OpCode::PUSHDATA1.to_u8());
    script.push(20);
    script.extend(to.as_bytes());
    
    // Push from address
    script.push(OpCode::PUSHDATA1.to_u8());
    script.push(20);
    script.extend(from.as_bytes());
    
    // Push method name "transfer"
    script.push(OpCode::PUSHDATA1.to_u8());
    script.push(8);
    script.extend(b"transfer");
    
    // Push token contract hash
    script.push(OpCode::PUSHDATA1.to_u8());
    script.push(20);
    script.extend(neo_token_hash().as_bytes());
    
    // Call contract
    script.push(OpCode::SYSCALL.to_u8());
    script.extend(&interop_hash("System.Contract.Call"));
    
    script
}

fn create_test_contract() -> (NefFile, ContractManifest) {
    // Simple add contract
    let script = vec![
        OpCode::LDARG1.to_u8(),  // Load second argument
        OpCode::LDARG0.to_u8(),  // Load first argument  
        OpCode::ADD.to_u8(),     // Add them
        OpCode::RET.to_u8(),     // Return result
    ];
    
    let nef = NefFile {
        magic: *b"NEF3",
        compiler: "neo-rs-test".to_string(),
        version: "1.0.0".to_string(),
        script,
        checksum: 0, // Would be calculated
    };
    
    let manifest = ContractManifest {
        name: "TestContract".to_string(),
        groups: vec![],
        features: Default::default(),
        supported_standards: vec![],
        abi: Default::default(),
        permissions: vec![],
        trusts: vec![],
        extra: None,
    };
    
    (nef, manifest)
}

fn create_deploy_script(nef: &NefFile, manifest: &ContractManifest) -> Vec<u8> {
    let mut script = Vec::new();
    
    // Push manifest
    let manifest_json = serde_json::to_string(manifest).unwrap();
    script.push(OpCode::PUSHDATA2.to_u8());
    script.extend(&(manifest_json.len() as u16).to_le_bytes());
    script.extend(manifest_json.as_bytes());
    
    // Push NEF
    let nef_bytes = nef.to_bytes();
    script.push(OpCode::PUSHDATA2.to_u8());
    script.extend(&(nef_bytes.len() as u16).to_le_bytes());
    script.extend(&nef_bytes);
    
    // Call deploy
    script.push(OpCode::PUSH2.to_u8()); // 2 arguments
    script.push(OpCode::PACK.to_u8());
    script.push(OpCode::PUSHDATA1.to_u8());
    script.push(6);
    script.extend(b"deploy");
    script.push(OpCode::PUSHDATA1.to_u8());
    script.push(20);
    script.extend(&contract_management_hash().as_bytes());
    script.push(OpCode::SYSCALL.to_u8());
    script.extend(&interop_hash("System.Contract.Call"));
    
    script
}

fn create_invoke_script(contract: &UInt160, method: &str, args: Vec<i32>) -> Vec<u8> {
    let mut script = Vec::new();
    
    // Push arguments
    for arg in args.iter().rev() {
        script.push(OpCode::PUSHINT32.to_u8());
        script.extend(&arg.to_le_bytes());
    }
    
    // Push argument count
    script.push(OpCode::PUSH2.to_u8());
    script.push(OpCode::PACK.to_u8());
    
    // Push method name
    script.push(OpCode::PUSHDATA1.to_u8());
    script.push(method.len() as u8);
    script.extend(method.as_bytes());
    
    // Push contract hash
    script.push(OpCode::PUSHDATA1.to_u8());
    script.push(20);
    script.extend(contract.as_bytes());
    
    // Call contract
    script.push(OpCode::SYSCALL.to_u8());
    script.extend(&interop_hash("System.Contract.Call"));
    
    script
}

fn create_transaction_with_script(script: Vec<u8>) -> Transaction {
    Transaction {
        version: 0,
        nonce: rand::random(),
        system_fee: 1_000_000,
        network_fee: 100_000,
        valid_until_block: 10000,
        signers: vec![Signer {
            account: create_test_account(),
            scopes: WitnessScope::None,
        }],
        attributes: vec![],
        script,
        witnesses: vec![create_test_witness(&create_test_account())],
    }
}

fn calculate_merkle_root(transactions: &[Transaction]) -> UInt256 {
    // Simplified merkle root calculation
    if transactions.is_empty() {
        return UInt256::zero();
    }
    
    let mut hashes: Vec<UInt256> = transactions
        .iter()
        .map(|tx| tx.hash().unwrap())
        .collect();
    
    while hashes.len() > 1 {
        let mut new_hashes = Vec::new();
        for i in (0..hashes.len()).step_by(2) {
            if i + 1 < hashes.len() {
                // Hash pair
                let combined = [hashes[i].as_bytes(), hashes[i + 1].as_bytes()].concat();
                new_hashes.push(UInt256::from_bytes(&sha256(&combined)).unwrap());
            } else {
                // Odd number, just copy
                new_hashes.push(hashes[i]);
            }
        }
        hashes = new_hashes;
    }
    
    hashes[0]
}

fn neo_token_hash() -> UInt160 {
    // NEO token contract hash on TestNet
    UInt160::from_str("0xef4073a0f2b305a38ec4050e4d3d28bc40ea63f5").unwrap()
}

fn contract_management_hash() -> UInt160 {
    // Contract Management contract hash
    UInt160::from_str("0xffffffffffffffffffffffffffffffffffffffff").unwrap()
}

fn initial_balance() -> i64 {
    100_000_00000000 // 100,000 NEO
}

fn interop_hash(method: &str) -> [u8; 4] {
    let hash = sha256(method.as_bytes());
    [hash[0], hash[1], hash[2], hash[3]]
}

fn sha256(data: &[u8]) -> [u8; 32] {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}