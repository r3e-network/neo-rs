//! Smart Contract Helper Tests
//! Converted from C# Neo.UnitTests.SmartContract.UT_SmartContractHelper.cs
//! and C# Neo.UnitTests.SmartContract.UT_Helper.cs

use neo_core::ScriptBuilder;
use neo_core::neo_vm::execution_context::ExecutionContext;
use neo_core::network::p2p::helper::get_sign_data_vec;
use neo_core::network::p2p::payloads::signer::Signer;
use neo_core::network::p2p::payloads::witness::Witness;
use neo_core::persistence::DataCache;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::CallFlags;
use neo_core::smart_contract::TriggerType;
use neo_core::smart_contract::application_engine::{ApplicationEngine, TEST_MODE_GAS};
use neo_core::smart_contract::diagnostic::Diagnostic;
use neo_core::smart_contract::helper::Helper;
use neo_core::wallets::key_pair::KeyPair;
use neo_core::{Transaction, UInt160, WitnessScope};
use neo_vm::Instruction;
use neo_vm::OpCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};

#[derive(Debug)]
struct OpcodeDiagnostic {
    pre_exec_count: Arc<AtomicUsize>,
    opcode_units: Arc<AtomicI64>,
    context_loaded_count: Arc<AtomicUsize>,
}

impl Diagnostic for OpcodeDiagnostic {
    fn initialized(&mut self, _engine: &mut ApplicationEngine) {}

    fn disposed(&mut self) {}

    fn context_loaded(&mut self, _context: &ExecutionContext) {
        self.context_loaded_count.fetch_add(1, Ordering::Relaxed);
    }

    fn context_unloaded(&mut self, _context: &ExecutionContext) {}

    fn pre_execute_instruction(&mut self, instruction: &Instruction) {
        self.pre_exec_count.fetch_add(1, Ordering::Relaxed);
        let units = ApplicationEngine::get_opcode_price(instruction.opcode.byte());
        self.opcode_units.fetch_add(units, Ordering::Relaxed);
    }

    fn post_execute_instruction(&mut self, _instruction: &Instruction) {}
}

fn make_public_key(seed: u8) -> Vec<u8> {
    let key = KeyPair::from_private_key(&[seed; 32]).expect("key");
    key.compressed_public_key()
}

// ============================================================================
// Helper.IsSignatureContract tests (from C# UT_SmartContractHelper.cs)
// ============================================================================

/// Test converted from C# TestIsSignatureContract
#[test]
fn test_is_signature_contract() {
    // Create a valid signature contract script
    let public_key = make_public_key(1);
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
    let public_key = make_public_key(1);
    let signature_script = Helper::signature_redeem_script(&public_key);
    assert!(Helper::is_standard_contract(&signature_script));

    // Test with multi-sig contract (3-of-3)
    let public_keys = vec![make_public_key(2), make_public_key(3), make_public_key(4)];
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
    let public_keys = vec![make_public_key(1), make_public_key(2), make_public_key(3)];
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

/// C# v3.10.2 `TestIsMultiSigContract_InvalidPublicKey`.
///
/// Prefix 0 and prefix 4 are not compressed secp256r1 points. Every
/// classification path must reject the script.
#[test]
fn test_is_multi_sig_contract_rejects_undecodable_public_key() {
    for prefix in [0u8, 4] {
        let script = multisig_script(1, &[undecodable_key(prefix)]);
        assert!(
            !Helper::is_multi_sig_contract(&script),
            "prefix {prefix:#x} must not be a multisig contract"
        );
        assert!(Helper::parse_multi_sig_contract(&script).is_none());
        assert!(!Helper::is_standard_contract(&script));
    }
}

/// C# v3.10.2 `TestIsMultiSigContract_WrongCurve`.
///
/// The first secp256k1 point has no secp256r1 compressed encoding. A multisig
/// script that includes it is not standard. The other three compressed points
/// also lie on secp256r1, so a script of just those three is standard.
#[test]
fn test_is_multi_sig_contract_rejects_secp256k1_only_point() {
    let keys: Vec<Vec<u8>> = K256_UNCOMPRESSED
        .iter()
        .map(|hex| compressed_secp256k1(hex))
        .collect();

    let bad = multisig_script(3, &keys);
    assert!(!Helper::is_multi_sig_contract(&bad));
    assert!(Helper::parse_multi_sig_contract(&bad).is_none());

    let good = multisig_script(3, &keys[1..]);
    let parsed = Helper::parse_multi_sig_contract(&good).expect("three shared points");
    assert_eq!(parsed.0, 3);
    assert_eq!(parsed.1.len(), 3);
    assert!(Helper::is_multi_sig_contract(&good));
}

/// C# `TestIsSignatureContract_WrongCurve`. Signature scripts are not decoded.
#[test]
fn test_is_signature_contract_does_not_decode_public_key() {
    let key = compressed_secp256k1(K256_UNCOMPRESSED[0]);
    let script = Helper::signature_redeem_script(&key);
    assert!(Helper::is_signature_contract(&script));
    assert!(Helper::is_standard_contract(&script));
}

/// C# v3.10.2 `Test_Verify_MalformedMultiSigWitness`.
///
/// The witness has the multisig shape and an all-zero public key. It is not a
/// standard witness, so state-independent verification accepts it and
/// state-dependent verification rejects it. That holds whether or not the
/// signer account equals the witness script hash.
#[test]
fn test_malformed_multisig_witness_is_not_standard() {
    use neo_core::ledger::VerifyResult;

    let verification = multisig_script(1, &[undecodable_key(0)]);
    let mut invocation = ScriptBuilder::new();
    invocation.emit_push(&[0u8; 64]);
    let invocation = invocation.to_array();
    let witness = Witness::new_with_scripts(invocation, verification);
    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);

    for matching_script_hash in [true, false] {
        let account = if matching_script_hash {
            witness.script_hash()
        } else {
            UInt160::from_script(&[1, 2, 3])
        };
        let mut tx = Transaction::new();
        tx.set_nonce(123_456);
        tx.set_system_fee(0);
        tx.set_network_fee(1_0000_0000);
        tx.set_valid_until_block(10);
        tx.set_script(vec![OpCode::PUSH1.byte()]);
        tx.set_signers(vec![Signer::new(account, WitnessScope::GLOBAL)]);
        tx.set_witnesses(vec![witness.clone()]);

        assert_eq!(
            matching_script_hash,
            account == tx.witnesses()[0].script_hash()
        );
        assert_eq!(
            tx.verify_state_independent(&settings),
            VerifyResult::Succeed
        );
        assert_eq!(
            tx.verify_state_dependent(&settings, &snapshot, None, &[]),
            VerifyResult::Invalid
        );
        assert_eq!(
            tx.verify(&settings, &snapshot, None, &[]),
            VerifyResult::Invalid
        );
    }
}

/// Uncompressed secp256k1 points from C# `TestIsMultiSigContract_WrongCurve`.
const K256_UNCOMPRESSED: [&str; 4] = [
    "047b4e72ae854b6a0955b3e02d92651ab7fa641a936066776ad438f95bb674a269a63ff98544691663d91a6cfcd215831f01bfb7a226363a6c5c67ef14541dba07",
    "040486468683c112125978ffe876245b2006bfe739aca8539b67335079262cb27ad0dedc9e5583f99b61c6f46bf80b97eaec3654b87add0e5bd7106c69922a229d",
    "040d26fc2ad3b1aae20f040b5f83380670f8ef5c2b2ac921ba3bdd79fd0af0525177715fd4370b1012ddd10579698d186ab342c223da3e884ece9cab9b6638c7bb",
    "04a114d72fe2997cdac67427b6f39ea08ed46213c8bb6a461bbac2a6212cf43fb510f8adf59b0b087a7859f96d0288e5e94800eab8388f30f03f92b2e4d807dfce",
];

fn undecodable_key(prefix: u8) -> Vec<u8> {
    let mut key = vec![0u8; 33];
    key[0] = prefix;
    key
}

fn compressed_secp256k1(uncompressed_hex: &str) -> Vec<u8> {
    use neo_core::cryptography::{ECCurve, ECPoint, Hex};

    let encoded = Hex::decode(uncompressed_hex).expect("hex");
    ECPoint::from_bytes_with_curve(ECCurve::Secp256k1, &encoded)
        .expect("secp256k1 point")
        .to_bytes()
}

fn multisig_script(m: i64, keys: &[Vec<u8>]) -> Vec<u8> {
    let mut builder = ScriptBuilder::new();
    builder.emit_push_int(m);
    for key in keys {
        builder.emit_push(key);
    }
    builder.emit_push_int(keys.len() as i64);
    builder
        .emit_syscall("System.Crypto.CheckMultisig")
        .expect("CheckMultisig syscall");
    builder.to_array()
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
    tx.set_script(vec![OpCode::PUSH1.byte()]);
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

    assert!(
        engine
            .result_stack()
            .peek(0)
            .expect("result")
            .as_boolean()
            .unwrap_or(false)
    );

    let expected_opcode_units = ApplicationEngine::get_opcode_price(OpCode::PUSHDATA1.byte()) * 2
        + ApplicationEngine::get_opcode_price(OpCode::SYSCALL.byte());
    assert!(context_loaded_count.load(Ordering::Relaxed) > 0);
    assert!(pre_exec_count.load(Ordering::Relaxed) > 0);
    assert_eq!(opcode_units.load(Ordering::Relaxed), expected_opcode_units);

    // Helper::signature_contract_cost() returns raw fee units; engine.fee_consumed()
    // is in datoshi (= units * ExecFeeFactor). Test pre-dated commit 4f599eb2
    // which corrected the 30× cpu_fee undercharge.
    let exec_fee_factor =
        neo_core::smart_contract::native::PolicyContract::DEFAULT_EXEC_FEE_FACTOR as i64;
    let expected_fee = Helper::signature_contract_cost() * exec_fee_factor;
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
    tx.set_script(vec![OpCode::PUSH1.byte()]);
    tx.set_signers(vec![Signer::new(script_hash, WitnessScope::GLOBAL)]);
    tx.set_attributes(Vec::new());
    tx.set_witnesses(Vec::new());

    let sign_data = get_sign_data_vec(&tx, settings.network).expect("sign data");
    let mut ordered = [
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

    assert!(
        engine
            .result_stack()
            .peek(0)
            .expect("result")
            .as_boolean()
            .unwrap_or(false)
    );

    let push_cost = ApplicationEngine::get_opcode_price(OpCode::PUSHDATA1.byte());
    let m_opcode = ApplicationEngine::get_opcode_price(OpCode::PUSH2.byte());
    let n_opcode = ApplicationEngine::get_opcode_price(OpCode::PUSH2.byte());
    let syscall_cost = ApplicationEngine::get_opcode_price(OpCode::SYSCALL.byte());
    let expected_opcode_units = push_cost * 4 + m_opcode + n_opcode + syscall_cost;
    assert!(context_loaded_count.load(Ordering::Relaxed) > 0);
    assert!(pre_exec_count.load(Ordering::Relaxed) > 0);
    assert_eq!(opcode_units.load(Ordering::Relaxed), expected_opcode_units);

    // Helper::multi_signature_contract_cost returns raw fee units; engine.fee_consumed()
    // is in datoshi (= units * ExecFeeFactor). Test pre-dated commit 4f599eb2.
    let exec_fee_factor =
        neo_core::smart_contract::native::PolicyContract::DEFAULT_EXEC_FEE_FACTOR as i64;
    let expected_fee =
        Helper::multi_signature_contract_cost(2, public_keys.len() as i32) * exec_fee_factor;
    assert_eq!(engine.fee_consumed(), expected_fee);
}

// ============================================================================
// Helper.ParseMultiSigContract tests
// ============================================================================

/// Test parse_multi_sig_contract with valid scripts
#[test]
fn test_parse_multi_sig_contract_valid() {
    // Create a 2-of-3 multi-sig script
    let public_keys = vec![make_public_key(1), make_public_key(2), make_public_key(3)];
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
    let public_key = make_public_key(1);
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
    let public_keys = vec![make_public_key(1), make_public_key(2), make_public_key(3)];
    let script = Helper::multi_sig_redeem_script(2, &public_keys);

    // Script should start with PUSH2 (Neo VM opcode 0x12)
    assert_eq!(script[0], OpCode::PUSH2.byte());

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
    let public_keys = vec![make_public_key(1)];
    let _script = Helper::multi_sig_redeem_script(0, &public_keys);
}

#[test]
#[should_panic(expected = "Invalid multi-sig parameters")]
fn test_multi_sig_redeem_script_invalid_m_greater_than_n() {
    let public_keys = vec![make_public_key(1)];
    let _script = Helper::multi_sig_redeem_script(2, &public_keys); // m=2 > n=1
}

#[test]
#[should_panic(expected = "Invalid multi-sig parameters")]
fn test_multi_sig_redeem_script_invalid_n_greater_than_1024() {
    // C# allows up to 1024 public keys in a multi-sig contract.
    let public_keys: Vec<Vec<u8>> = (1..=1025)
        .map(|i| make_public_key(((i % 16) + 1) as u8))
        .collect();
    let _script = Helper::multi_sig_redeem_script(1, &public_keys);
}

#[test]
fn test_multi_sig_redeem_script_accepts_n_up_to_1024() {
    // n=17 used to be rejected by the legacy 16-key limit; C# allows it.
    let public_keys: Vec<Vec<u8>> = (1..=17).map(make_public_key).collect();
    let script = Helper::multi_sig_redeem_script(1, &public_keys);
    assert!(Helper::is_multi_sig_contract(&script));
}
