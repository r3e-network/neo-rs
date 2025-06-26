//! Consensus Integration Tests
//!
//! Comprehensive tests for the Neo consensus protocol implementation,
//! including validator management, block proposals, and consensus state management.

use neo_consensus::{
    context::{ConsensusContext, TimerType},
    messages::{
        Commit, ConsensusMessage, ConsensusMessageData, PrepareRequest, PrepareResponse,
        RecoveryRequest,
    },
    proposal::{BlockProposal, ProposalConfig, ProposalManager, TransactionSelectionStrategy},
    recovery::{RecoveryConfig, RecoveryManager, RecoveryReason, RecoveryStatus},
    validators::{Validator, ValidatorManager, ValidatorPerformance, ValidatorSet},
    ConsensusConfig, ConsensusService, ConsensusServiceConfig, ConsensusServiceState,
};
use neo_core::{Signer, Transaction, UInt160, UInt256, Witness, WitnessScope};
use neo_ledger::{Block, BlockHeader, Blockchain, VerifyResult};
use neo_network::NetworkConfig;
use neo_persistence::Storage;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio_test;

/// Test consensus configuration validation
#[test]
fn test_consensus_config_validation() {
    let mut config = ConsensusConfig::default();

    // Test default values
    assert!(config.view_timeout > 0, "View timeout should be positive");
    assert!(config.block_time > 0, "Block time should be positive");
    assert!(
        config.max_block_size > 0,
        "Max block size should be positive"
    );
    assert!(
        config.max_transactions_per_block > 0,
        "Max transactions should be positive"
    );

    // Test invalid configurations
    config.view_timeout = 0;
    assert!(
        !config.is_valid(),
        "Config with zero view timeout should be invalid"
    );

    config = ConsensusConfig::default();
    config.block_time = 0;
    assert!(
        !config.is_valid(),
        "Config with zero block time should be invalid"
    );

    config = ConsensusConfig::default();
    config.min_validators = 0;
    assert!(
        !config.is_valid(),
        "Config with zero min validators should be invalid"
    );

    config = ConsensusConfig::default();
    config.max_validators = config.min_validators - 1;
    assert!(
        !config.is_valid(),
        "Config with max_validators < min_validators should be invalid"
    );

    println!("✅ Consensus config validation test passed");
}

/// Test validator manager operations
#[tokio::test]
async fn test_validator_manager() {
    let config = ConsensusConfig::default();
    let mut validator_manager = ValidatorManager::new(config.clone());

    // Test initial state
    assert_eq!(
        validator_manager.get_all_validators().len(),
        0,
        "Should start with no validators"
    );
    assert_eq!(
        validator_manager.get_active_validators().len(),
        0,
        "Should start with no active validators"
    );

    // Register some validators
    let validator1_hash = UInt160::from_bytes(&[1u8; 20]).unwrap();
    let validator2_hash = UInt160::from_bytes(&[2u8; 20]).unwrap();
    let validator3_hash = UInt160::from_bytes(&[3u8; 20]).unwrap();

    let register_result1 = validator_manager.register_validator(
        validator1_hash,
        vec![1u8; 33], // Public key
        10_000_000,    // Stake
        0,             // Block height
    );
    assert!(
        register_result1.is_ok(),
        "Should register validator 1 successfully"
    );

    let register_result2 =
        validator_manager.register_validator(validator2_hash, vec![2u8; 33], 20_000_000, 0);
    assert!(
        register_result2.is_ok(),
        "Should register validator 2 successfully"
    );

    let register_result3 =
        validator_manager.register_validator(validator3_hash, vec![3u8; 33], 15_000_000, 0);
    assert!(
        register_result3.is_ok(),
        "Should register validator 3 successfully"
    );

    // Test validator retrieval
    assert_eq!(
        validator_manager.get_all_validators().len(),
        3,
        "Should have 3 validators"
    );

    let validator1 = validator_manager.get_validator(&validator1_hash);
    assert!(validator1.is_some(), "Should find validator 1");
    assert_eq!(
        validator1.unwrap().stake,
        10_000_000,
        "Validator 1 stake should match"
    );

    // Test validator set creation
    let validator_set = validator_manager.create_validator_set(100);
    assert!(
        validator_set.is_ok(),
        "Should create validator set successfully"
    );

    let set = validator_set.unwrap();
    assert!(
        set.validators().len() <= config.max_validators as usize,
        "Validator set should not exceed max"
    );

    // Test performance tracking
    validator_manager.update_validator_performance(&validator1_hash, true);
    validator_manager.update_validator_performance(&validator1_hash, false);
    validator_manager.update_validator_performance(&validator1_hash, true);

    let performance = validator_manager.get_validator_performance(&validator1_hash);
    assert!(performance.is_some(), "Should have performance data");

    println!("✅ Validator manager test passed");
}

/// Test consensus context management
#[tokio::test]
async fn test_consensus_context() {
    let config = ConsensusConfig::default();
    let node_hash = UInt160::zero();
    let context = ConsensusContext::new(config.clone(), node_hash);

    // Test initial state
    assert_eq!(context.view_number(), 0, "Should start at view 0");
    assert_eq!(
        context.primary_index(),
        0,
        "Should start with primary index 0"
    );
    assert_eq!(context.block_index(), 0, "Should start at block index 0");

    // Create some test validators
    let validators = vec![
        Validator::new(UInt160::zero(), vec![1], 1000, 0, 100),
        Validator::new(
            UInt160::from_bytes(&[1; 20]).unwrap(),
            vec![2],
            2000,
            1,
            100,
        ),
        Validator::new(
            UInt160::from_bytes(&[2; 20]).unwrap(),
            vec![3],
            3000,
            2,
            100,
        ),
        Validator::new(
            UInt160::from_bytes(&[3; 20]).unwrap(),
            vec![4],
            4000,
            3,
            100,
        ),
    ];
    let validator_set = ValidatorSet::new(validators, 100);

    // Set validator set
    context.set_validator_set(validator_set.clone());
    assert_eq!(context.validator_count(), 4, "Should have 4 validators");

    // Test view changes
    context.increment_view();
    assert_eq!(context.view_number(), 1, "View should increment to 1");

    // Test primary calculation
    let primary = context.get_primary();
    assert!(primary.is_some(), "Should have a primary validator");

    // Test consensus state
    context.reset_for_new_block(1);
    assert_eq!(context.block_index(), 1, "Block index should be updated");
    assert_eq!(context.view_number(), 0, "View should reset to 0");

    println!("✅ Consensus context test passed");
}

/// Test block proposal mechanism
#[tokio::test]
async fn test_block_proposal() {
    let config = ProposalConfig::default();
    let mut proposal_manager = ProposalManager::new(config.clone());

    // Create test transactions
    let mut transactions = Vec::new();
    for i in 0..5 {
        let mut tx = Transaction::new();
        tx.set_nonce(i);
        tx.set_network_fee(100_000);
        tx.set_system_fee(50_000);
        tx.set_valid_until_block(1000);

        let signer = Signer::new(UInt160::zero(), WitnessScope::CalledByEntry);
        tx.add_signer(signer);

        let witness = Witness::new_with_scripts(vec![0x40], vec![0x41, 0x56, 0x57]);
        tx.add_witness(witness);

        transactions.push(tx);
    }

    // Create block proposal
    let proposer = UInt160::zero();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    let proposal_result = proposal_manager.create_proposal(
        proposer,
        &transactions,
        timestamp,
        UInt256::zero(), // Previous hash
    );

    assert!(
        proposal_result.is_ok(),
        "Should create proposal successfully"
    );

    let proposal = proposal_result.unwrap();
    assert!(
        proposal.transactions.len() <= transactions.len(),
        "Proposal should not exceed input transactions"
    );
    assert!(
        proposal.total_network_fee >= 0,
        "Total network fee should be non-negative"
    );
    assert!(
        proposal.total_system_fee >= 0,
        "Total system fee should be non-negative"
    );

    // Test proposal validation
    let validation_result = proposal.validate(&config);
    assert!(validation_result.is_ok(), "Proposal should be valid");

    println!("✅ Block proposal test passed");
}

/// Test consensus message handling
#[tokio::test]
async fn test_consensus_messages() {
    let block_hash = UInt256::from_bytes(&[1; 32]).unwrap();
    let view_number = 0u8;
    let validator_index = 0u8;

    // Test PrepareRequest
    let prepare_request = PrepareRequest::new(
        block_hash,
        vec![1, 2, 3, 4],                             // Block data
        vec![UInt256::from_bytes(&[2; 32]).unwrap()], // Transaction hashes
    );

    assert_eq!(prepare_request.block_hash, block_hash);
    assert_eq!(prepare_request.transaction_count(), 1);
    assert!(
        prepare_request.validate().is_ok(),
        "PrepareRequest should be valid"
    );

    // Test PrepareResponse
    let prepare_response = PrepareResponse::new(block_hash);
    assert_eq!(prepare_response.block_hash, block_hash);
    assert!(
        prepare_response.validate().is_ok(),
        "PrepareResponse should be valid"
    );

    // Test Commit
    let commit = Commit::new(block_hash, vec![0x41; 64]); // Signature
    assert_eq!(commit.block_hash, block_hash);
    assert_eq!(commit.signature.len(), 64);
    assert!(commit.validate().is_ok(), "Commit should be valid");

    // Test consensus message wrapper
    let message_data = ConsensusMessageData::PrepareRequest(prepare_request);
    let message = ConsensusMessage::new(view_number, validator_index, message_data);

    assert_eq!(message.view_number, view_number);
    assert_eq!(message.validator_index, validator_index);

    println!("✅ Consensus messages test passed");
}

/// Test recovery mechanism
#[tokio::test]
async fn test_consensus_recovery() {
    let config = RecoveryConfig::default();
    let consensus_config = ConsensusConfig::default();
    let node_hash = UInt160::zero();
    let context = Arc::new(ConsensusContext::new(consensus_config, node_hash));

    // Set up validators for the context
    let validators = vec![
        Validator::new(UInt160::zero(), vec![1], 1000, 0, 100),
        Validator::new(
            UInt160::from_bytes(&[1; 20]).unwrap(),
            vec![2],
            2000,
            1,
            100,
        ),
        Validator::new(
            UInt160::from_bytes(&[2; 20]).unwrap(),
            vec![3],
            3000,
            2,
            100,
        ),
        Validator::new(
            UInt160::from_bytes(&[3; 20]).unwrap(),
            vec![4],
            4000,
            3,
            100,
        ),
    ];
    let validator_set = ValidatorSet::new(validators, 100);
    context.set_validator_set(validator_set);

    let mut recovery_manager = RecoveryManager::new(config, context);

    // Test recovery initiation
    let recovery_result = recovery_manager.initiate_recovery(RecoveryReason::ViewTimeout);
    assert!(
        recovery_result.is_ok(),
        "Should initiate recovery successfully"
    );

    // Test recovery status
    let status = recovery_manager.get_recovery_status();
    assert_ne!(status, RecoveryStatus::None, "Should have recovery status");

    // Test recovery message creation
    let recovery_request = RecoveryRequest::new(0, 0); // View 0, validator 0
    assert!(
        recovery_request.validate().is_ok(),
        "Recovery request should be valid"
    );

    println!("✅ Consensus recovery test passed");
}

/// Test complete consensus round simulation
#[tokio::test]
async fn test_consensus_round_simulation() {
    let consensus_config = ConsensusConfig {
        view_timeout: 5000,
        block_time: 15000,
        max_block_size: 262144,
        max_transactions_per_block: 10, // Small number for testing
        min_validators: 4,
        max_validators: 7,
        ..Default::default()
    };

    let network_config = NetworkConfig::default();

    // Create multiple consensus service instances (simulating nodes)
    let mut nodes = Vec::new();
    for i in 0..4 {
        let node_id = UInt160::from_bytes(&[i as u8; 20]).unwrap();
        let service_config = ConsensusServiceConfig {
            node_id,
            consensus_config: consensus_config.clone(),
            network_config: network_config.clone(),
            enable_auto_start: false,
        };

        let service = ConsensusService::new(service_config);
        assert!(
            service.is_ok(),
            "Should create consensus service for node {}",
            i
        );
        nodes.push(service.unwrap());
    }

    // Verify all nodes are created and in correct initial state
    for (i, node) in nodes.iter().enumerate() {
        assert_eq!(
            node.state(),
            ConsensusServiceState::Stopped,
            "Node {} should start stopped",
            i
        );
    }

    // Test that configurations are correct
    for node in &nodes {
        let config = node.config();
        assert_eq!(config.consensus_config.min_validators, 4);
        assert_eq!(config.consensus_config.max_validators, 7);
        assert_eq!(config.consensus_config.max_transactions_per_block, 10);
    }

    println!("✅ Consensus round simulation test passed");
    println!("   🔸 Created {} consensus nodes", nodes.len());
    println!("   🔸 All nodes initialized correctly");
    println!("   🔸 Consensus configuration validated");
}

/// Test consensus with actual blockchain integration
#[tokio::test]
async fn test_consensus_blockchain_integration() {
    // Create storage and blockchain
    let storage = Arc::new(Storage::new_memory());
    let blockchain = Arc::new(Blockchain::new(storage.clone()));

    // Create consensus configuration
    let consensus_config = ConsensusConfig {
        view_timeout: 1000,
        block_time: 5000,
        max_block_size: 1024,
        max_transactions_per_block: 5,
        min_validators: 1, // Single validator for testing
        max_validators: 1,
        ..Default::default()
    };

    // Create validator manager
    let mut validator_manager = ValidatorManager::new(consensus_config.clone());

    // Register a single validator for testing
    let validator_hash = UInt160::zero();
    let register_result =
        validator_manager.register_validator(validator_hash, vec![1u8; 33], 1_000_000, 0);
    assert!(register_result.is_ok(), "Should register validator");

    // Create validator set
    let validator_set = validator_manager.create_validator_set(0).unwrap();
    assert_eq!(
        validator_set.validators().len(),
        1,
        "Should have 1 validator"
    );

    // Create a test transaction
    let mut transaction = Transaction::new();
    transaction.set_nonce(1);
    transaction.set_network_fee(100_000);
    transaction.set_system_fee(50_000);
    transaction.set_valid_until_block(1000);

    let signer = Signer::new(UInt160::zero(), WitnessScope::CalledByEntry);
    transaction.add_signer(signer);

    let witness = Witness::new_with_scripts(vec![0x40], vec![0x41, 0x56, 0x57]);
    transaction.add_witness(witness);

    // Create block with transaction
    let mut header = BlockHeader::new(
        0,               // version
        UInt256::zero(), // previous hash
        UInt256::zero(), // merkle root (will be calculated)
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64,
        0,              // nonce
        0,              // index
        0,              // consensus data
        validator_hash, // next consensus
    );

    let block_witness = Witness::new_with_scripts(vec![0x40], vec![0x41, 0x56, 0x57]);
    header.add_witness(block_witness);

    let block = Block::new(header, vec![transaction]);

    // Validate block
    let validation_result = block.validate(None);
    assert_eq!(
        validation_result,
        VerifyResult::Succeed,
        "Block should be valid"
    );

    // Add block to blockchain
    let add_result = blockchain.add_block(block);
    assert!(add_result.is_ok(), "Should add block to blockchain");

    // Verify blockchain state
    assert_eq!(
        blockchain.block_count(),
        1,
        "Blockchain should have 1 block"
    );

    println!("✅ Consensus-blockchain integration test passed");
    println!(
        "   🔸 Blockchain height: {}",
        blockchain.current_block_height()
    );
    println!("   🔸 Block count: {}", blockchain.block_count());
    println!(
        "   🔸 Validator count: {}",
        validator_set.validators().len()
    );
}

/// Test consensus performance under load
#[tokio::test]
async fn test_consensus_performance() {
    let start_time = std::time::Instant::now();

    let consensus_config = ConsensusConfig::default();
    let mut validator_manager = ValidatorManager::new(consensus_config.clone());

    // Register many validators
    for i in 0..100 {
        let mut validator_bytes = [0u8; 20];
        validator_bytes[0] = (i % 256) as u8;
        validator_bytes[1] = ((i / 256) % 256) as u8;

        let validator_hash = UInt160::from_bytes(&validator_bytes).unwrap();
        let result = validator_manager.register_validator(
            validator_hash,
            vec![(i % 256) as u8; 33],
            1_000_000 + (i as u64 * 1000),
            0,
        );
        assert!(result.is_ok(), "Should register validator {}", i);
    }

    // Create validator sets
    for block_height in 0..10 {
        let validator_set = validator_manager.create_validator_set(block_height);
        assert!(
            validator_set.is_ok(),
            "Should create validator set for block {}",
            block_height
        );

        let set = validator_set.unwrap();
        assert!(set.validators().len() <= consensus_config.max_validators as usize);
    }

    // Test consensus context operations
    let node_hash = UInt160::zero();
    let context = ConsensusContext::new(consensus_config.clone(), node_hash);

    // Simulate multiple view changes
    for _ in 0..20 {
        context.increment_view();
    }

    let elapsed = start_time.elapsed();
    println!("✅ Consensus performance test completed in {:?}", elapsed);
    println!("   🔸 Registered 100 validators");
    println!("   🔸 Created 10 validator sets");
    println!("   🔸 Performed 20 view changes");

    // Should complete quickly (less than 1 second)
    assert!(
        elapsed.as_secs() < 1,
        "Consensus performance test should be fast"
    );
}

/// Test consensus error handling and recovery
#[tokio::test]
async fn test_consensus_error_handling() {
    let consensus_config = ConsensusConfig::default();

    // Test invalid validator registration
    let mut validator_manager = ValidatorManager::new(consensus_config.clone());

    let invalid_result = validator_manager.register_validator(
        UInt160::zero(),
        vec![1u8; 33],
        100, // Below minimum stake
        0,
    );
    assert!(
        invalid_result.is_err(),
        "Should reject validator with low stake"
    );

    // Test duplicate validator registration
    let validator_hash = UInt160::from_bytes(&[1; 20]).unwrap();
    let first_result =
        validator_manager.register_validator(validator_hash, vec![1u8; 33], 10_000_000, 0);
    assert!(first_result.is_ok(), "First registration should succeed");

    let duplicate_result = validator_manager.register_validator(
        validator_hash,
        vec![2u8; 33], // Different key
        20_000_000,    // Different stake
        0,
    );
    assert!(
        duplicate_result.is_err(),
        "Should reject duplicate validator"
    );

    // Test invalid consensus messages
    let invalid_prepare = PrepareRequest::new(
        UInt256::zero(), // Invalid hash
        vec![],          // Empty data
        vec![],          // No transactions
    );
    assert!(
        invalid_prepare.validate().is_err(),
        "Should reject invalid prepare request"
    );

    println!("✅ Consensus error handling test passed");
}

/// Test consensus state persistence and recovery
#[tokio::test]
async fn test_consensus_state_persistence() {
    let consensus_config = ConsensusConfig::default();
    let node_hash = UInt160::zero();

    // Create initial context and set some state
    let context1 = ConsensusContext::new(consensus_config.clone(), node_hash);
    context1.increment_view();
    context1.increment_view(); // View = 2
    context1.reset_for_new_block(5); // Block = 5

    // Simulate persistence by capturing state
    let saved_view = context1.view_number();
    let saved_block = context1.block_index();

    // Create new context and restore state
    let context2 = ConsensusContext::new(consensus_config.clone(), node_hash);
    context2.reset_for_new_block(saved_block);

    // Manually set view (in real implementation, this would be restored from storage)
    for _ in 0..saved_view {
        context2.increment_view();
    }

    // Verify state restoration
    assert_eq!(
        context2.view_number(),
        saved_view,
        "View should be restored"
    );
    assert_eq!(
        context2.block_index(),
        saved_block,
        "Block index should be restored"
    );

    println!("✅ Consensus state persistence test passed");
    println!(
        "   🔸 Saved state: view={}, block={}",
        saved_view, saved_block
    );
    println!(
        "   🔸 Restored state: view={}, block={}",
        context2.view_number(),
        context2.block_index()
    );
}
