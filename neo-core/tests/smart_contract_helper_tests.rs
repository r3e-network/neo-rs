//! Smart Contract Helper Tests
//! Converted from C# Neo.UnitTests.SmartContract.UT_SmartContractHelper.cs
//! and C# Neo.UnitTests.SmartContract.UT_Helper.cs

use neo_core::network::p2p::helper::get_sign_data_vec;
use neo_core::network::p2p::payloads::signer::Signer;
use neo_core::network::p2p::payloads::witness::Witness;
use neo_core::persistence::DataCache;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::application_engine::{ApplicationEngine, TEST_MODE_GAS};
use neo_core::smart_contract::call_flags::CallFlags;
use neo_core::smart_contract::helper::Helper;
use neo_core::smart_contract::i_diagnostic::IDiagnostic;
use neo_core::smart_contract::native::policy_contract::PolicyContract;
use neo_core::smart_contract::trigger_type::TriggerType;
use neo_core::wallets::key_pair::KeyPair;
use neo_core::{Transaction, UInt160, WitnessScope};
use neo_vm::execution_context::ExecutionContext;
use neo_vm::instruction::Instruction;
use neo_vm::op_code::OpCode;
use neo_vm::ScriptBuilder;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Debug)]
struct OpcodeDiagnostic {
    pre_exec_count: Arc<AtomicUsize>,
    opcode_units: Arc<AtomicI64>,
    context_loaded_count: Arc<AtomicUsize>,
}

impl IDiagnostic for OpcodeDiagnostic {
    fn initialized(&mut self, _engine: &mut ApplicationEngine) {}

    fn disposed(&mut self) {}

    fn context_loaded(&mut self, _context: &ExecutionContext) {
        self.context_loaded_count.fetch_add(1, Ordering::Relaxed);
    }

    fn context_unloaded(&mut self, _context: &ExecutionContext) {}

    fn pre_execute_instruction(&mut self, instruction: &Instruction) {
        self.pre_exec_count.fetch_add(1, Ordering::Relaxed);
        let units = ApplicationEngine::get_opcode_price(instruction.opcode as u8);
        self.opcode_units.fetch_add(units, Ordering::Relaxed);
    }

    fn post_execute_instruction(&mut self, _instruction: &Instruction) {}
}

// ============================================================================
// Helper.IsSignatureContract tests (from C# UT_SmartContractHelper.cs)
// ============================================================================

/// Test converted from C# TestIsSignatureContract
#[test]
fn test_is_signature_contract() {
    // Create a valid signature contract script
    let public_key = vec![0u8; 33]; // 33-byte compressed public key
    let script = Helper::signature_redeem_script(&public_key);

    // Should be recognized as a signature contract
    assert!(Helper::is_signature_contract(&script));

    // Modify first byte - should no longer be recognized
    let mut modified_script = script.clone();
    modified_script[0] = 0x22; // Invalid opcode
    assert!(!Helper::is_signature_contract(&modified_script));
}

/// Test converted from C# TestIsStandardContract
#[test]
fn test_is_standard_contract() {
    // Test with signature contract
    let public_key = vec![0u8; 33];
    let signature_script = Helper::signature_redeem_script(&public_key);
    assert!(Helper::is_standard_contract(&signature_script));

    // Test with multi-sig contract (3-of-3)
    let public_keys = vec![vec![0u8; 33], vec![1u8; 33], vec![2u8; 33]];
    let multi_sig_script = Helper::multi_sig_redeem_script(3, &public_keys);
    assert!(Helper::is_standard_contract(&multi_sig_script));

    // Test with non-standard script
    let non_standard = vec![0x01, 0x02, 0x03];
    assert!(!Helper::is_standard_contract(&non_standard));
}

// ============================================================================
// Helper.IsMultiSigContract tests (from C# UT_SmartContractHelper.cs)
// ============================================================================

/// Test converted from C# TestIsMultiSigContract
#[test]
fn test_is_multi_sig_contract() {
    // Test 3-of-3 multi-sig
    let public_keys = vec![vec![0u8; 33], vec![1u8; 33], vec![2u8; 33]];
    let script = Helper::multi_sig_redeem_script(3, &public_keys);
    assert!(Helper::is_multi_sig_contract(&script));

    // Modify last byte - should no longer be recognized
    let mut modified_script = script.clone();
    let len = modified_script.len();
    modified_script[len - 1] = 0x00;
    assert!(!Helper::is_multi_sig_contract(&modified_script));
}

/// Test case from C# - invalid multi-sig patterns
#[test]
fn test_is_multi_sig_contract_invalid_cases() {
    // Case 1: Invalid first byte (0 instead of PUSH1-PUSH16)
    let case1 = vec![
        0, 2, 12, 33, 221, 221, 221, 221, 221, 221, 221, 221, 221, 221, 221, 221, 221, 221, 221,
        221, 221, 221, 221, 221, 221, 221, 221, 221, 221, 221, 221, 221, 221, 221, 221, 221, 221,
        12, 33, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
        255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 0,
    ];
    assert!(!Helper::is_multi_sig_contract(&case1));

    // Case 2: Invalid ending (no SYSCALL)
    let case2 = vec![
        18, 12, 33, 2, 111, 240, 59, 148, 146, 65, 206, 29, 173, 212, 53, 25, 230, 150, 14, 10,
        133, 180, 26, 105, 160, 92, 50, 129, 3, 170, 43, 206, 21, 148, 202, 22, 12, 33, 2, 111,
        240, 59, 148, 146, 65, 206, 29, 173, 212, 53, 25, 230, 150, 14, 10, 133, 180, 26, 105, 160,
        92, 50, 129, 3, 170, 43, 206, 21, 148, 202, 22, 18,
    ];
    assert!(!Helper::is_multi_sig_contract(&case2));
}

// ============================================================================
// Helper.GetContractHash tests (from C# UT_Helper.cs)
// ============================================================================

/// Test converted from C# TestGetContractHash
#[test]
fn test_get_contract_hash() {
    // NEF checksum is computed from the contract
    // Using a simple script [1, 2, 3]
    let nef_checksum: u32 = 529571427; // Example checksum for script [1, 2, 3]

    // Test with zero sender
    let hash1 = Helper::get_contract_hash(&UInt160::zero(), nef_checksum, "");
    assert_ne!(hash1, UInt160::zero());

    // Test with non-zero sender
    let sender =
        UInt160::parse("0xa400ff00ff00ff00ff00ff00ff00ff00ff00ff01").expect("valid sender");
    let hash2 = Helper::get_contract_hash(&sender, nef_checksum, "");
    assert_ne!(hash2, UInt160::zero());

    // Different senders should produce different hashes
    assert_ne!(hash1, hash2);

    // Same sender, checksum, and name should produce same hash
    let hash3 = Helper::get_contract_hash(&sender, nef_checksum, "");
    assert_eq!(hash2, hash3);

    // Different name should produce different hash
    let hash4 = Helper::get_contract_hash(&sender, nef_checksum, "MyContract");
    assert_ne!(hash2, hash4);
}

// ============================================================================
// Helper.SignatureContractCost tests (from C# UT_Helper.cs)
// ============================================================================

/// Test converted from C# TestSignatureContractCost
#[test]
fn test_signature_contract_cost() {
    let cost = Helper::signature_contract_cost();
    // Cost should be positive and match C# calculation
    assert!(cost > 0, "Signature contract cost should be positive");

    // Expected formula: PUSHDATA1 * 2 + SYSCALL + CHECK_SIG_PRICE
    // From C# Neo: 1000512 datoshi (0.01 GAS with fee factor 30)
    // The cost should be reasonable for a signature verification
}

/// Test converted from C# TestSignatureContractCost (engine fee consumption).
#[test]
fn test_signature_contract_engine_fee_consumed() {
    let snapshot = DataCache::new(false);
    let settings = ProtocolSettings::default();
    let key = KeyPair::from_private_key(&[1u8; 32]).expect("key");
    let verification_script = key.get_verification_script();
    let script_hash = UInt160::from_script(&verification_script);
    let pre_exec_count = Arc::new(AtomicUsize::new(0));
    let opcode_units = Arc::new(AtomicI64::new(0));
    let context_loaded_count = Arc::new(AtomicUsize::new(0));

    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(1);
    tx.set_system_fee(0);
    tx.set_network_fee(0);
    tx.set_valid_until_block(1);
    tx.set_script(vec![OpCode::PUSH1 as u8]);
    tx.set_signers(vec![Signer::new(script_hash, WitnessScope::GLOBAL)]);
    tx.set_attributes(Vec::new());
    tx.set_witnesses(Vec::new());

    let sign_data = get_sign_data_vec(&tx, settings.network).expect("sign data");
    let signature = key.sign(&sign_data).expect("sign");
    let mut builder = ScriptBuilder::new();
    builder.emit_push(&signature);
    let invocation_script = builder.to_array();
    tx.set_witnesses(vec![Witness::new_with_scripts(
        invocation_script.clone(),
        verification_script.clone(),
    )]);

    let diagnostic = OpcodeDiagnostic {
        pre_exec_count: Arc::clone(&pre_exec_count),
        opcode_units: Arc::clone(&opcode_units),
        context_loaded_count: Arc::clone(&context_loaded_count),
    };
    let mut engine = ApplicationEngine::new(
        TriggerType::Verification,
        Some(Arc::new(tx)),
        Arc::new(snapshot.clone_cache()),
        None,
        settings,
        TEST_MODE_GAS,
        Some(Box::new(diagnostic)),
    )
    .expect("engine");
    engine
        .load_script(verification_script, CallFlags::READ_ONLY, Some(script_hash))
        .expect("load verification");
    engine
        .load_script(invocation_script, CallFlags::NONE, None)
        .expect("load invocation");
    engine.execute().expect("execute");

    assert!(engine
        .result_stack()
        .peek(0)
        .expect("result")
        .get_boolean()
        .unwrap_or(false));

    let expected_opcode_units = ApplicationEngine::get_opcode_price(OpCode::PUSHDATA1 as u8) * 2
        + ApplicationEngine::get_opcode_price(OpCode::SYSCALL as u8);
    assert!(context_loaded_count.load(Ordering::Relaxed) > 0);
    assert!(pre_exec_count.load(Ordering::Relaxed) > 0);
    assert_eq!(opcode_units.load(Ordering::Relaxed), expected_opcode_units);

    let expected_fee =
        Helper::signature_contract_cost() * PolicyContract::DEFAULT_EXEC_FEE_FACTOR as i64;
    assert_eq!(engine.fee_consumed(), expected_fee);
}

/// Test converted from C# TestMultiSignatureContractCost
#[test]
fn test_multi_signature_contract_cost() {
    // Test 1-of-1 multi-sig
    let cost_1_1 = Helper::multi_signature_contract_cost(1, 1);
    assert!(cost_1_1 > 0, "1-of-1 multi-sig cost should be positive");

    // Test 2-of-3 multi-sig
    let cost_2_3 = Helper::multi_signature_contract_cost(2, 3);
    assert!(cost_2_3 > cost_1_1, "2-of-3 should cost more than 1-of-1");

    // Test 3-of-5 multi-sig
    let cost_3_5 = Helper::multi_signature_contract_cost(3, 5);
    assert!(cost_3_5 > cost_2_3, "3-of-5 should cost more than 2-of-3");

    // Cost should scale with number of signers
    let cost_1_3 = Helper::multi_signature_contract_cost(1, 3);
    let cost_1_5 = Helper::multi_signature_contract_cost(1, 5);
    assert!(
        cost_1_5 > cost_1_3,
        "More signers should mean higher verification cost"
    );
}

/// Test converted from C# TestMultiSignatureContractCost (engine fee consumption).
#[test]
fn test_multi_signature_contract_engine_fee_consumed() {
    let snapshot = DataCache::new(false);
    let settings = ProtocolSettings::default();
    let key1 = KeyPair::from_private_key(&[2u8; 32]).expect("key1");
    let key2 = KeyPair::from_private_key(&[3u8; 32]).expect("key2");
    let public_keys = vec![key1.compressed_public_key(), key2.compressed_public_key()];
    let verification_script = Helper::multi_sig_redeem_script(2, &public_keys);
    let script_hash = UInt160::from_script(&verification_script);
    let pre_exec_count = Arc::new(AtomicUsize::new(0));
    let opcode_units = Arc::new(AtomicI64::new(0));
    let context_loaded_count = Arc::new(AtomicUsize::new(0));

    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(2);
    tx.set_system_fee(0);
    tx.set_network_fee(0);
    tx.set_valid_until_block(1);
    tx.set_script(vec![OpCode::PUSH1 as u8]);
    tx.set_signers(vec![Signer::new(script_hash, WitnessScope::GLOBAL)]);
    tx.set_attributes(Vec::new());
    tx.set_witnesses(Vec::new());

    let sign_data = get_sign_data_vec(&tx, settings.network).expect("sign data");
    let mut ordered = vec![
        (key1.compressed_public_key(), key1.clone()),
        (key2.compressed_public_key(), key2.clone()),
    ];
    ordered.sort_by(|a, b| a.0.cmp(&b.0));
    let signatures: Vec<Vec<u8>> = ordered
        .iter()
        .map(|(_, key)| key.sign(&sign_data).expect("sign"))
        .collect();

    let mut builder = ScriptBuilder::new();
    for signature in &signatures {
        builder.emit_push(signature);
    }
    let invocation_script = builder.to_array();
    tx.set_witnesses(vec![Witness::new_with_scripts(
        invocation_script.clone(),
        verification_script.clone(),
    )]);

    let diagnostic = OpcodeDiagnostic {
        pre_exec_count: Arc::clone(&pre_exec_count),
        opcode_units: Arc::clone(&opcode_units),
        context_loaded_count: Arc::clone(&context_loaded_count),
    };
    let mut engine = ApplicationEngine::new(
        TriggerType::Verification,
        Some(Arc::new(tx)),
        Arc::new(snapshot.clone_cache()),
        None,
        settings,
        TEST_MODE_GAS,
        Some(Box::new(diagnostic)),
    )
    .expect("engine");
    engine
        .load_script(verification_script, CallFlags::READ_ONLY, Some(script_hash))
        .expect("load verification");
    engine
        .load_script(invocation_script, CallFlags::NONE, None)
        .expect("load invocation");
    engine.execute().expect("execute");

    assert!(engine
        .result_stack()
        .peek(0)
        .expect("result")
        .get_boolean()
        .unwrap_or(false));

    let push_cost = ApplicationEngine::get_opcode_price(OpCode::PUSHDATA1 as u8);
    let m_opcode = ApplicationEngine::get_opcode_price(OpCode::PUSH2 as u8);
    let n_opcode = ApplicationEngine::get_opcode_price(OpCode::PUSH2 as u8);
    let syscall_cost = ApplicationEngine::get_opcode_price(OpCode::SYSCALL as u8);
    let expected_opcode_units = push_cost * 4 + m_opcode + n_opcode + syscall_cost;
    assert!(context_loaded_count.load(Ordering::Relaxed) > 0);
    assert!(pre_exec_count.load(Ordering::Relaxed) > 0);
    assert_eq!(opcode_units.load(Ordering::Relaxed), expected_opcode_units);

    let expected_fee =
        Helper::multi_signature_contract_cost(2, public_keys.len() as i32)
            * PolicyContract::DEFAULT_EXEC_FEE_FACTOR as i64;
    assert_eq!(engine.fee_consumed(), expected_fee);
}

// ============================================================================
// Helper.ParseMultiSigContract tests
// ============================================================================

/// Test parse_multi_sig_contract with valid scripts
#[test]
fn test_parse_multi_sig_contract_valid() {
    // Create a 2-of-3 multi-sig script
    let public_keys = vec![vec![0u8; 33], vec![1u8; 33], vec![2u8; 33]];
    let script = Helper::multi_sig_redeem_script(2, &public_keys);

    // Parse it
    let result = Helper::parse_multi_sig_contract(&script);
    assert!(result.is_some(), "Should parse valid multi-sig script");

    let (m, parsed_keys) = result.unwrap();
    assert_eq!(m, 2, "Required signatures should be 2");
    assert_eq!(parsed_keys.len(), 3, "Should have 3 public keys");
}

/// Test parse_multi_sig_contract with invalid scripts
#[test]
fn test_parse_multi_sig_contract_invalid() {
    // Too short
    let short_script = vec![0x51, 0x0C, 0x21];
    assert!(Helper::parse_multi_sig_contract(&short_script).is_none());

    // Invalid first byte
    let invalid_first = vec![0x00; 50];
    assert!(Helper::parse_multi_sig_contract(&invalid_first).is_none());

    // Empty
    assert!(Helper::parse_multi_sig_contract(&[]).is_none());
}

// ============================================================================
// Helper.ParseMultiSigInvocation tests
// ============================================================================

/// Test parse_multi_sig_invocation with valid invocation scripts
#[test]
fn test_parse_multi_sig_invocation_valid() {
    // Create an invocation script with 2 signatures (64 bytes each)
    let mut invocation = Vec::new();
    for _ in 0..2 {
        invocation.push(0x0C); // PUSHDATA1
        invocation.push(64); // 64 bytes
        invocation.extend(vec![0xAB; 64]); // signature placeholder
    }

    let result = Helper::parse_multi_sig_invocation(&invocation, 2);
    assert!(result.is_some(), "Should parse valid invocation script");

    let signatures = result.unwrap();
    assert_eq!(signatures.len(), 2, "Should have 2 signatures");
    assert_eq!(signatures[0].len(), 64, "Each signature should be 64 bytes");
}

/// Test parse_multi_sig_invocation with invalid scripts
#[test]
fn test_parse_multi_sig_invocation_invalid() {
    // Zero required signatures
    assert!(Helper::parse_multi_sig_invocation(&[], 0).is_none());

    // Wrong signature count
    let mut invocation = Vec::new();
    invocation.push(0x0C); // PUSHDATA1
    invocation.push(64); // 64 bytes
    invocation.extend(vec![0xAB; 64]);

    // Expect 2 but only 1 provided
    assert!(Helper::parse_multi_sig_invocation(&invocation, 2).is_none());

    // Invalid opcode (not PUSHDATA1)
    let invalid_opcode = vec![0x00, 64];
    assert!(Helper::parse_multi_sig_invocation(&invalid_opcode, 1).is_none());

    // Invalid signature length (not 64)
    let mut invalid_len = Vec::new();
    invalid_len.push(0x0C); // PUSHDATA1
    invalid_len.push(32); // Wrong length
    invalid_len.extend(vec![0xAB; 32]);
    assert!(Helper::parse_multi_sig_invocation(&invalid_len, 1).is_none());
}

// ============================================================================
// Script Creation tests
// ============================================================================

/// Test signature_redeem_script creation
#[test]
fn test_signature_redeem_script_creation() {
    let public_key = vec![0x02; 33]; // Compressed public key format
    let script = Helper::signature_redeem_script(&public_key);

    // Script should be exactly 40 bytes
    assert_eq!(script.len(), 40);

    // First byte should be PUSHDATA1 (0x0C)
    assert_eq!(script[0], 0x0C);

    // Second byte should be 33 (public key length)
    assert_eq!(script[1], 33);

    // Public key should be at bytes 2-34
    assert_eq!(&script[2..35], &public_key[..]);

    // Byte 35 should be SYSCALL (0x41)
    assert_eq!(script[35], 0x41);

    // Script should be recognized as signature contract
    assert!(Helper::is_signature_contract(&script));
}

/// Test multi_sig_redeem_script creation
#[test]
fn test_multi_sig_redeem_script_creation() {
    // Test with 2-of-3
    let public_keys = vec![vec![0x02; 33], vec![0x03; 33], vec![0x04; 33]];
    let script = Helper::multi_sig_redeem_script(2, &public_keys);

    // Script should start with PUSH2 (Neo VM opcode 0x12)
    assert_eq!(script[0], OpCode::PUSH2 as u8);

    // Should end with SYSCALL and CheckMultisig hash
    let len = script.len();
    assert_eq!(script[len - 5], 0x41); // SYSCALL

    // Script should be recognized as multi-sig contract
    assert!(Helper::is_multi_sig_contract(&script));
}

/// Test multi_sig_redeem_script panics on invalid parameters
#[test]
#[should_panic(expected = "Invalid multi-sig parameters")]
fn test_multi_sig_redeem_script_invalid_m_zero() {
    let public_keys = vec![vec![0x02; 33]];
    let _script = Helper::multi_sig_redeem_script(0, &public_keys);
}

#[test]
#[should_panic(expected = "Invalid multi-sig parameters")]
fn test_multi_sig_redeem_script_invalid_m_greater_than_n() {
    let public_keys = vec![vec![0x02; 33]];
    let _script = Helper::multi_sig_redeem_script(2, &public_keys); // m=2 > n=1
}

#[test]
#[should_panic(expected = "Invalid multi-sig parameters")]
fn test_multi_sig_redeem_script_invalid_n_greater_than_16() {
    let public_keys: Vec<Vec<u8>> = (0..17).map(|i| vec![i as u8; 33]).collect();
    let _script = Helper::multi_sig_redeem_script(1, &public_keys); // n=17 > 16
}
