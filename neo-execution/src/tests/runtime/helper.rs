use super::*;
use crate::ContractState;
use crate::native_contract_provider::{NativeContractProvider, NoNativeContract};
use neo_crypto::Secp256r1Crypto;
use neo_manifest::{
    ContractAbi, ContractManifest, ContractMethodDescriptor, ContractPermission, ManifestFeatures,
    NefFile, WildCardContainer,
};
use neo_payloads::{Block, Header, OracleResponse, Signer, Transaction, TransactionAttribute};
use neo_primitives::{
    ContractBasicMethod, ContractParameterType, OracleResponseCode, WitnessScope,
};

struct EmptyNativeProvider;

impl NativeContractProvider for EmptyNativeProvider {
    type Contract = NoNativeContract;
}

struct ContractStateProvider {
    contract: ContractState,
}

impl NativeContractProvider for ContractStateProvider {
    type Contract = NoNativeContract;

    fn contract_state<B: neo_storage::CacheRead>(
        &self,
        _snapshot: &neo_storage::DataCache<B>,
        hash: &UInt160,
    ) -> CoreResult<Option<ContractState>> {
        Ok((hash == &self.contract.hash).then(|| self.contract.clone()))
    }
}

fn empty_provider() -> Arc<EmptyNativeProvider> {
    Arc::new(EmptyNativeProvider)
}

fn build_verify_contract(hash: UInt160) -> ContractState {
    let nef = NefFile::new(
        "test".to_string(),
        vec![OpCode::PUSH1.byte(), OpCode::RET.byte()],
    );
    let verify = ContractMethodDescriptor::new(
        ContractBasicMethod::VERIFY.to_string(),
        Vec::new(),
        ContractParameterType::Boolean,
        0,
        true,
    )
    .expect("verify descriptor");
    let manifest = ContractManifest {
        name: "VerifyContract".to_string(),
        groups: Vec::new(),
        features: ManifestFeatures::empty(),
        supported_standards: Vec::new(),
        abi: ContractAbi::new(vec![verify], Vec::new()),
        permissions: vec![ContractPermission::default_wildcard()],
        trusts: WildCardContainer::default(),
        extra: None,
    };
    ContractState::new(7, hash, nef, manifest)
}

fn signed_standard_transaction(valid_signature: bool) -> (Transaction, Witness, Vec<u8>) {
    let private_key = [23u8; 32];
    let public_key = Secp256r1Crypto::derive_public_key(&private_key).expect("derive public key");
    let verification_script = Helper::signature_redeem_script(&public_key);

    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(0x5566_7788);
    tx.set_system_fee(1);
    tx.set_network_fee(1);
    tx.set_valid_until_block(42);
    tx.set_script(vec![OpCode::RET.byte()]);
    tx.set_signers(vec![Signer::new(
        UInt160::from_script(&verification_script),
        WitnessScope::NONE,
    )]);

    let settings = ProtocolSettings::default();
    let sign_data = neo_payloads::get_sign_data_vec(&tx, settings.network)
        .expect("canonical transaction sign data");
    let signing_key = if valid_signature {
        private_key
    } else {
        [24u8; 32]
    };
    let signature = Secp256r1Crypto::sign(&sign_data, &signing_key).expect("sign transaction");
    let mut invocation = ScriptBuilder::new();
    invocation.emit_push(&signature);
    let witness = Witness::new_with_scripts(invocation.to_array(), verification_script);
    tx.set_witnesses(vec![witness.clone()]);
    (tx, witness, sign_data)
}

fn signed_standard_multisig_transaction() -> (Transaction, Witness, Vec<u8>) {
    let key_pairs = [[31u8; 32], [32u8; 32], [33u8; 32], [34u8; 32]]
        .into_iter()
        .map(|private_key| {
            let public_key = Secp256r1Crypto::derive_public_key(&private_key)
                .expect("derive multisig public key");
            (public_key, private_key)
        })
        .collect::<Vec<_>>();
    let public_keys = key_pairs
        .iter()
        .map(|(public_key, _)| public_key.clone())
        .collect::<Vec<_>>();
    let verification_script =
        neo_vm::script_builder::redeem_script::RedeemScript::multi_sig_redeem_script_from_keys(
            2,
            &public_keys,
        )
        .expect("build canonical 2-of-4 script");
    let (_, sorted_public_keys) =
        neo_vm::script_builder::redeem_script::RedeemScript::parse_multi_sig_contract(
            &verification_script,
        )
        .expect("parse canonical multisig script");

    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(0x6677_8899);
    tx.set_system_fee(1);
    tx.set_network_fee(1);
    tx.set_valid_until_block(42);
    tx.set_script(vec![OpCode::RET.byte()]);
    tx.set_signers(vec![Signer::new(
        UInt160::from_script(&verification_script),
        WitnessScope::NONE,
    )]);

    let settings = ProtocolSettings::default();
    let sign_data = neo_payloads::get_sign_data_vec(&tx, settings.network)
        .expect("canonical transaction sign data");
    let mut invocation = ScriptBuilder::new();
    for key_index in [0usize, 2] {
        let public_key = &sorted_public_keys[key_index];
        let private_key = key_pairs
            .iter()
            .find_map(|(candidate, private_key)| (candidate == public_key).then_some(private_key))
            .expect("private key for sorted multisig public key");
        let signature =
            Secp256r1Crypto::sign(&sign_data, private_key).expect("sign multisig transaction");
        invocation.emit_push(&signature);
    }
    let witness = Witness::new_with_scripts(invocation.to_array(), verification_script);
    tx.set_witnesses(vec![witness.clone()]);
    (tx, witness, sign_data)
}

#[test]
fn verify_witnesses_uses_transaction_witnesses_for_count_check() {
    let witness = Witness::new_with_scripts(Vec::new(), vec![OpCode::PUSH1.byte()]);
    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(0x0102_0304);
    tx.set_system_fee(1);
    tx.set_network_fee(1);
    tx.set_valid_until_block(42);
    tx.set_signers(vec![Signer::new(witness.script_hash(), WitnessScope::NONE)]);
    tx.set_script(vec![OpCode::RET.byte()]);
    tx.set_witnesses(vec![witness]);

    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);

    assert!(
        Helper::verify_witnesses_with_native_provider(
            &tx,
            &settings,
            &snapshot,
            Helper::MAX_VERIFICATION_GAS,
            empty_provider(),
        ),
        "transactions must expose their witnesses before the engine executes, matching C# Transaction.Witnesses"
    );
}

#[test]
fn verify_witnesses_uses_genesis_header_witnesses() {
    let witness = Witness::new_with_scripts(Vec::new(), vec![OpCode::PUSH1.byte()]);
    let mut header = Header::new();
    header.set_prev_hash(UInt256::zero());
    header.witness = witness;

    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);

    assert!(
        Helper::verify_witnesses_with_native_provider(
            &header,
            &settings,
            &snapshot,
            Helper::MAX_VERIFICATION_GAS,
            empty_provider(),
        ),
        "C# Header.IVerifiable exposes exactly one witness and uses Witness.ScriptHash for genesis headers"
    );
}

#[test]
fn verify_witnesses_uses_genesis_block_header_witnesses() {
    let witness = Witness::new_with_scripts(Vec::new(), vec![OpCode::PUSH1.byte()]);
    let mut block = Block::new();
    block.header.set_prev_hash(UInt256::zero());
    block.header.witness = witness;

    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);

    assert!(
        Helper::verify_witnesses_with_native_provider(
            &block,
            &settings,
            &snapshot,
            Helper::MAX_VERIFICATION_GAS,
            empty_provider(),
        ),
        "C# Block.IVerifiable delegates witnesses and verifying hashes to Header"
    );
}

#[test]
fn verify_witness_uses_verifiable_container_hook() {
    let source = include_str!("../../runtime/helper.rs");
    let start = source
        .find("pub fn verify_witness_with_native_provider<V, P, B>")
        .expect("verify_witness function exists");
    let end = source[start..]
        .find("ApplicationEngine::new_with_shared_block_and_native_contract_provider")
        .map(|offset| start + offset)
        .expect("engine construction exists");
    let setup = &source[start..end];

    assert!(
        setup.contains("to_verifiable_container()"),
        "Helper.VerifyWitness should install the actual C# IVerifiable-equivalent payload through VerifiableExt"
    );
    assert!(
        !setup.contains("as_transaction()"),
        "verification container selection must not regress to a transaction-only special case"
    );
}

#[test]
fn verify_witness_uses_transaction_container_for_check_witness() {
    let delegated_signer = UInt160::parse("0x0102030405060708090a0b0c0d0e0f1011121314").unwrap();

    let mut builder = ScriptBuilder::new();
    builder.emit_push(&delegated_signer.to_array());
    builder
        .emit_syscall("System.Runtime.CheckWitness")
        .expect("CheckWitness syscall");
    let witness = Witness::new_with_scripts(Vec::new(), builder.to_array());

    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(0x0102_0304);
    tx.set_system_fee(1);
    tx.set_network_fee(1);
    tx.set_valid_until_block(42);
    tx.set_script(vec![OpCode::RET.byte()]);
    tx.set_signers(vec![
        Signer::new(witness.script_hash(), WitnessScope::GLOBAL),
        Signer::new(delegated_signer, WitnessScope::GLOBAL),
    ]);
    tx.set_witnesses(vec![witness.clone(), Witness::empty()]);

    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);

    Helper::verify_witness_with_native_provider(
        &tx,
        &settings,
        &snapshot,
        &witness.script_hash(),
        &witness,
        Helper::MAX_VERIFICATION_GAS,
        empty_provider(),
    )
    .expect(
        "CheckWitness inside transaction witness verification must see the real Transaction container",
    );
}

#[test]
fn verify_witness_uses_explicit_native_provider_for_contract_verification() {
    let contract_hash =
        UInt160::parse("0xa1b2c3d4e5f60718293a4b5c6d7e8f0102030526").expect("contract hash");
    let provider = Arc::new(ContractStateProvider {
        contract: build_verify_contract(contract_hash),
    });

    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(0x0102_0304);
    tx.set_system_fee(1);
    tx.set_network_fee(1);
    tx.set_valid_until_block(42);
    tx.set_script(vec![OpCode::RET.byte()]);
    tx.set_signers(vec![Signer::new(contract_hash, WitnessScope::NONE)]);
    let witness = Witness::new_with_scripts(Vec::new(), Vec::new());
    tx.set_witnesses(vec![witness.clone()]);

    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);

    Helper::verify_witness_with_native_provider(
        &tx,
        &settings,
        &snapshot,
        &contract_hash,
        &witness,
        Helper::MAX_VERIFICATION_GAS,
        provider,
    )
    .expect("explicit provider should resolve ContractManagement after global replacement");
}

#[test]
fn cache_aware_witness_helper_still_executes_full_neovm_with_identical_fee() {
    let (tx, witness, sign_data) = signed_standard_transaction(true);
    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);
    let cache = crate::preverify_standard_witness_signatures(&sign_data, &witness)
        .expect("canonical signature cache");

    let uncached = Helper::verify_witness_with_native_provider(
        &tx,
        &settings,
        &snapshot,
        &witness.script_hash(),
        &witness,
        Helper::MAX_VERIFICATION_GAS,
        empty_provider(),
    )
    .expect("ordinary NeoVM witness verification");
    let cached = Helper::verify_witness_with_native_provider_and_signature_cache(
        &tx,
        &settings,
        &snapshot,
        &witness.script_hash(),
        &witness,
        Helper::MAX_VERIFICATION_GAS,
        empty_provider(),
        cache,
    )
    .expect("cache-aware NeoVM witness verification");

    assert_eq!(
        cached, uncached,
        "cache must not change NeoVM fee accounting"
    );
    assert!(cached > 0, "the verification script must still execute");
}

#[test]
fn rejected_pre_execution_guard_does_not_mark_cache_consumed() {
    let (tx, witness, sign_data) = signed_standard_transaction(true);
    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);
    let cache = crate::preverify_standard_witness_signatures(&sign_data, &witness)
        .expect("canonical signature cache");
    let wrong_script_hash = UInt160::zero();
    assert_ne!(wrong_script_hash, witness.script_hash());

    assert!(
        Helper::verify_witness_with_native_provider_and_signature_cache(
            &tx,
            &settings,
            &snapshot,
            &wrong_script_hash,
            &witness,
            Helper::MAX_VERIFICATION_GAS,
            empty_provider(),
            Arc::clone(&cache),
        )
        .is_err()
    );
    assert_eq!(
        cache.metrics_snapshot(),
        crate::PreverifiedSignatureCacheMetricsSnapshot::default()
    );
}

#[test]
fn cache_aware_multisig_witness_matches_full_neovm_fee_and_result() {
    let (tx, witness, sign_data) = signed_standard_multisig_transaction();
    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);
    let cache = crate::preverify_standard_witness_signatures(&sign_data, &witness)
        .expect("canonical multisig cache");
    assert_eq!(
        cache.operation_count(),
        4,
        "2-of-4 traversal includes two canonical false scan pairs"
    );

    let uncached = Helper::verify_witness_with_native_provider(
        &tx,
        &settings,
        &snapshot,
        &witness.script_hash(),
        &witness,
        Helper::MAX_VERIFICATION_GAS,
        empty_provider(),
    )
    .expect("ordinary multisig NeoVM witness verification");
    let cached = Helper::verify_witness_with_native_provider_and_signature_cache(
        &tx,
        &settings,
        &snapshot,
        &witness.script_hash(),
        &witness,
        Helper::MAX_VERIFICATION_GAS,
        empty_provider(),
        Arc::clone(&cache),
    )
    .expect("cache-aware multisig NeoVM witness verification");

    assert_eq!(
        cached, uncached,
        "cache must preserve multisig gas accounting"
    );
    assert!(cached > 0, "the multisig verification script must execute");
    assert_eq!(
        cache.metrics_snapshot(),
        crate::PreverifiedSignatureCacheMetricsSnapshot {
            canonical_uses: 1,
            lookups: 4,
            hits: 4,
            misses: 0,
        },
        "every canonical CheckMultisig pair, including false scans, should hit"
    );
}

#[test]
fn cached_false_outcome_matches_ordinary_witness_failure() {
    let (tx, witness, sign_data) = signed_standard_transaction(false);
    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);
    let cache = crate::preverify_standard_witness_signatures(&sign_data, &witness)
        .expect("canonical false signature cache");

    assert!(
        Helper::verify_witness_with_native_provider(
            &tx,
            &settings,
            &snapshot,
            &witness.script_hash(),
            &witness,
            Helper::MAX_VERIFICATION_GAS,
            empty_provider(),
        )
        .is_err()
    );
    assert!(
        Helper::verify_witness_with_native_provider_and_signature_cache(
            &tx,
            &settings,
            &snapshot,
            &witness.script_hash(),
            &witness,
            Helper::MAX_VERIFICATION_GAS,
            empty_provider(),
            cache,
        )
        .is_err()
    );
}

#[test]
fn verify_witness_uses_transaction_container_for_current_signers() {
    let second_signer = UInt160::parse("0x14131211100f0e0d0c0b0a090807060504030201").unwrap();

    let mut builder = ScriptBuilder::new();
    builder
        .emit_syscall("System.Runtime.CurrentSigners")
        .expect("CurrentSigners syscall");
    builder.emit_opcode(OpCode::SIZE);
    builder.emit_opcode(OpCode::PUSH2);
    builder.emit_opcode(OpCode::NUMEQUAL);
    let witness = Witness::new_with_scripts(Vec::new(), builder.to_array());

    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(0x0102_0304);
    tx.set_system_fee(1);
    tx.set_network_fee(1);
    tx.set_valid_until_block(42);
    tx.set_script(vec![OpCode::RET.byte()]);
    tx.set_signers(vec![
        Signer::new(witness.script_hash(), WitnessScope::NONE),
        Signer::new(second_signer, WitnessScope::GLOBAL),
    ]);
    tx.set_witnesses(vec![witness.clone(), Witness::empty()]);

    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);

    Helper::verify_witness_with_native_provider(
        &tx,
        &settings,
        &snapshot,
        &witness.script_hash(),
        &witness,
        Helper::MAX_VERIFICATION_GAS,
        empty_provider(),
    )
    .expect(
        "CurrentSigners inside transaction witness verification must see the real Transaction container",
    );
}

#[test]
fn verify_witness_uses_transaction_container_for_get_script_container() {
    let mut builder = ScriptBuilder::new();
    builder
        .emit_syscall("System.Runtime.GetScriptContainer")
        .expect("GetScriptContainer syscall");
    builder.emit_push_int(2);
    builder.emit_opcode(OpCode::PICKITEM);
    builder.emit_push_int(0x0102_0304);
    builder.emit_opcode(OpCode::NUMEQUAL);
    let witness = Witness::new_with_scripts(Vec::new(), builder.to_array());

    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(0x0102_0304);
    tx.set_system_fee(1);
    tx.set_network_fee(1);
    tx.set_valid_until_block(42);
    tx.set_script(vec![OpCode::RET.byte()]);
    tx.set_signers(vec![Signer::new(witness.script_hash(), WitnessScope::NONE)]);
    tx.set_witnesses(vec![witness.clone()]);

    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);

    Helper::verify_witness_with_native_provider(
        &tx,
        &settings,
        &snapshot,
        &witness.script_hash(),
        &witness,
        Helper::MAX_VERIFICATION_GAS,
        empty_provider(),
    )
    .expect("GetScriptContainer inside transaction witness verification must expose the real Transaction");
}

#[test]
fn oracle_response_check_witness_faults_when_request_is_missing() {
    let delegated_signer = UInt160::parse("0x0102030405060708090a0b0c0d0e0f1011121314").unwrap();

    let mut builder = ScriptBuilder::new();
    builder.emit_push(&delegated_signer.to_array());
    builder
        .emit_syscall("System.Runtime.CheckWitness")
        .expect("CheckWitness syscall");
    builder.emit_opcode(OpCode::NOT);
    let witness = Witness::new_with_scripts(Vec::new(), builder.to_array());

    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(0x0102_0304);
    tx.set_system_fee(1);
    tx.set_network_fee(1);
    tx.set_valid_until_block(42);
    tx.set_script(vec![OpCode::RET.byte()]);
    tx.set_attributes(vec![TransactionAttribute::OracleResponse(
        OracleResponse::new(7, OracleResponseCode::Success, Vec::new()),
    )]);
    tx.set_signers(vec![Signer::new(witness.script_hash(), WitnessScope::NONE)]);
    tx.set_witnesses(vec![witness.clone()]);

    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);

    assert!(
        Helper::verify_witness_with_native_provider(
            &tx,
            &settings,
            &snapshot,
            &witness.script_hash(),
            &witness,
            Helper::MAX_VERIFICATION_GAS,
            empty_provider(),
        )
        .is_err(),
        "C# CheckWitnessInternal faults when an OracleResponse request lookup is missing"
    );
}

#[test]
fn verify_witness_rejects_strictly_invalid_verification_script_before_execution() {
    let verification_script = vec![
        OpCode::PUSH1.byte(),
        OpCode::RET.byte(),
        OpCode::JMP.byte(),
        0x7f,
    ];
    let witness = Witness::new_with_scripts(Vec::new(), verification_script);

    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(0x0102_0304);
    tx.set_system_fee(1);
    tx.set_network_fee(1);
    tx.set_valid_until_block(42);
    tx.set_script(vec![OpCode::RET.byte()]);
    tx.set_signers(vec![Signer::new(witness.script_hash(), WitnessScope::NONE)]);
    tx.set_witnesses(vec![witness.clone()]);

    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);

    assert!(
        Helper::verify_witness_with_native_provider(
            &tx,
            &settings,
            &snapshot,
            &witness.script_hash(),
            &witness,
            Helper::MAX_VERIFICATION_GAS,
            empty_provider(),
        )
        .is_err(),
        "C# Helper.VerifyWitness constructs Script(verification, strict: true) before execution"
    );
}
