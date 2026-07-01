//! # neo-node::tests::consensus
//!
//! Test module grouping Consensus-facing node adapters and startup helpers.
//! coverage for neo-node.
//!
//! ## Boundary
//!
//! This is test/benchmark-only code for neo-node; it may assemble fixtures but
//! must not introduce production behavior.
//!
//! ## Contents
//!
//! - `proposal`: consensus proposal construction helpers.

use super::*;
use neo_consensus::{
    ChangeViewMessage, ChangeViewReason, ConsensusContext, ConsensusMessageType,
    messages::PrepareRequestMessage,
};
use neo_crypto::signature::Secp256r1Crypto;
use neo_io::Serializable;
use neo_mempool::PoolItem;
use neo_payloads::{Signer, Transaction, TransactionAttribute, Witness};
use neo_primitives::{UInt160, VerifyResult, WitnessScope};
use neo_serialization::BinarySerializer;
use neo_storage::{StorageItem, StorageKey};
use neo_vm::StackItem;
use neo_vm_rs::ExecutionEngineLimits;
use neo_vm_rs::OpCode;

#[path = "proposal.rs"]
mod proposal;

/// The dBFT extensible codec round-trips a consensus payload: encode a
/// signed `ConsensusPayload` to an `ExtensiblePayload`, then decode it back
/// to the same fields (the inbound path authenticates the sender).
#[test]
fn extensible_codec_round_trips() {
    let settings = ProtocolSettings::default();
    let validators = build_consensus_validators(&settings);
    assert!(!validators.is_empty(), "default settings carry a committee");

    let validator_index = 0u8;
    let signature = vec![0xABu8; 64];
    let mut original = ConsensusPayload::new(
        settings.network,
        7, // block_index
        validator_index,
        0, // view_number
        neo_consensus::ConsensusMessageType::Commit,
        vec![0x01, 0x02, 0x03], // body
    );
    original.witness = signature.clone();

    let ext = consensus_to_extensible(&original, &validators).expect("encode");
    assert_eq!(ext.category, DBFT_CATEGORY);
    assert_eq!(ext.valid_block_end, 7);
    assert_eq!(ext.sender, validators[validator_index as usize].script_hash);

    let decoded = extensible_to_consensus(&ext, settings.network, &validators).expect("decode");
    assert_eq!(decoded.block_index, 7);
    assert_eq!(decoded.validator_index, validator_index);
    assert_eq!(
        decoded.message_type,
        neo_consensus::ConsensusMessageType::Commit
    );
    assert_eq!(decoded.data, vec![0x01, 0x02, 0x03]);
    assert_eq!(decoded.witness, signature);
}

/// A non-dBFT extensible is ignored by the consensus decoder.
#[test]
fn extensible_codec_rejects_non_dbft() {
    let settings = ProtocolSettings::default();
    let validators = build_consensus_validators(&settings);
    let mut ext = ExtensiblePayload::new();
    ext.category = "StateService".to_string();
    ext.valid_block_end = 1;
    assert!(extensible_to_consensus(&ext, settings.network, &validators).is_none());
}

/// The validator set is sorted ascending by public key (consensus-critical:
/// the index order drives primary selection + NextConsensus).
#[test]
fn validators_are_sorted_by_pubkey() {
    let settings = ProtocolSettings::default();
    let validators = build_consensus_validators(&settings);
    for pair in validators.windows(2) {
        assert!(
            pair[0].public_key <= pair[1].public_key,
            "validators must be sorted"
        );
    }
    for (i, v) in validators.iter().enumerate() {
        assert_eq!(v.index as usize, i);
    }
}

#[test]
fn prepare_request_ledger_guard_rejects_already_persisted_transaction_hash() {
    neo_native_contracts::install();
    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);
    let pool = MemoryPool::new(&settings);
    let tx = signed_zero_fee_tx(&settings, 0x40);
    seed_persisted_transaction(&snapshot, 7, &tx);

    let payload = prepare_request_payload(vec![tx.hash()]);

    assert!(
        !prepare_request_passes_ledger_guards(&payload, &snapshot, &pool, &settings),
        "C# OnPrepareRequestReceived returns before accepting a proposed on-chain tx"
    );
}

#[test]
fn prepare_request_ledger_guard_rejects_available_transaction_with_onchain_conflict() {
    neo_native_contracts::install();
    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);
    let pool = MemoryPool::new(&settings);
    seed_current_block(&snapshot, 0);
    set_zero_policy_fee(&snapshot, 10);
    set_zero_policy_fee(&snapshot, 18);

    let tx = signed_zero_fee_tx(&settings, 0x41);
    let hash = tx.hash();
    let signer = tx.signers().first().expect("signer").account;
    assert_eq!(pool.try_add(tx, &snapshot), VerifyResult::Succeed);
    seed_current_block(&snapshot, 100);
    seed_traceable_conflict(&snapshot, &hash, &signer, 95);

    let payload = prepare_request_payload(vec![hash]);

    assert!(
        !prepare_request_passes_ledger_guards(&payload, &snapshot, &pool, &settings),
        "C# OnPrepareRequestReceived rejects proposed txs with traceable on-chain conflicts"
    );
}

#[test]
fn prepare_request_ledger_guard_uses_dynamic_max_traceable_blocks() {
    neo_native_contracts::install();
    let mut settings = ProtocolSettings::default();
    settings
        .hardforks
        .insert(neo_config::Hardfork::HfEchidna, 0);
    let snapshot = DataCache::new(false);
    let pool = MemoryPool::new(&settings);
    seed_current_block(&snapshot, 0);
    set_zero_policy_fee(&snapshot, 10);
    set_zero_policy_fee(&snapshot, 18);

    let tx = signed_zero_fee_tx(&settings, 0x42);
    let hash = tx.hash();
    let signer = tx.signers().first().expect("signer").account;
    assert_eq!(pool.try_add(tx, &snapshot), VerifyResult::Succeed);
    seed_current_block(&snapshot, 100);
    set_policy_u32(&snapshot, 23, 3);
    seed_traceable_conflict(&snapshot, &hash, &signer, 95);

    let payload = prepare_request_payload(vec![hash]);

    assert!(
        prepare_request_passes_ledger_guards(&payload, &snapshot, &pool, &settings),
        "Policy MaxTraceableBlocks=3 makes a block-95 conflict untraceable at height 100"
    );
}

/// `build_consensus_setup` is a no-op when disabled and errors when enabled
/// without a key.
#[test]
fn setup_gating() {
    let settings = ProtocolSettings::default();
    assert!(
        build_consensus_setup(&settings, false, None, None)
            .unwrap()
            .is_none()
    );
    assert!(build_consensus_setup(&settings, true, None, None).is_err());
    assert!(build_consensus_setup(&settings, true, Some("zz"), None).is_err());

    let non_validator_key = hex::encode([0x11u8; 32]);
    let setup = build_consensus_setup(&settings, true, Some(&non_validator_key), None)
        .unwrap()
        .expect("consensus configured");
    assert!(
        setup.my_index.is_none(),
        "key is not in the startup validator set"
    );
    assert!(!setup.validators.is_empty());
}

fn set_zero_policy_fee(snapshot: &DataCache, prefix: u8) {
    snapshot.add(
        StorageKey::new(neo_native_contracts::PolicyContract::ID, vec![prefix]),
        StorageItem::from_bytes(Vec::new()),
    );
}

fn set_policy_u32(snapshot: &DataCache, prefix: u8, value: u32) {
    snapshot.add(
        StorageKey::new(neo_native_contracts::PolicyContract::ID, vec![prefix]),
        StorageItem::from_bytes(u32_to_native_storage_bytes(value)),
    );
}

fn u32_to_native_storage_bytes(value: u32) -> Vec<u8> {
    if value == 0 {
        return Vec::new();
    }

    let mut bytes = value.to_le_bytes().to_vec();
    while bytes.len() > 1 {
        let last = *bytes.last().expect("non-empty");
        let next = bytes[bytes.len() - 2];
        if last != 0 || next & 0x80 != 0 {
            break;
        }
        bytes.pop();
    }
    if bytes.last().expect("non-empty") & 0x80 != 0 {
        bytes.push(0);
    }
    bytes
}

fn prepare_request_payload(transaction_hashes: Vec<UInt256>) -> ConsensusPayload {
    let message =
        PrepareRequestMessage::new(1, 0, 0, 0, UInt256::default(), 1, 42, transaction_hashes);
    ConsensusPayload::new(
        ProtocolSettings::default().network,
        1,
        0,
        0,
        ConsensusMessageType::PrepareRequest,
        message.serialize(),
    )
}

fn seed_current_block(snapshot: &DataCache, index: u32) {
    let ledger = LedgerContract::new();
    snapshot.update(
        StorageKey::new(LedgerContract::ID, vec![12]),
        StorageItem::from_bytes(
            ledger
                .serialize_hash_index_state(&UInt256::from_bytes(&[0x11; 32]).unwrap(), index)
                .unwrap(),
        ),
    );
}

fn seed_persisted_transaction(snapshot: &DataCache, block_index: u32, tx: &Transaction) {
    let mut key = Vec::with_capacity(33);
    key.push(11);
    key.extend_from_slice(&tx.hash().to_bytes());
    snapshot.add(
        StorageKey::new(LedgerContract::ID, key),
        StorageItem::from_bytes(
            LedgerContract::new()
                .serialize_persisted_transaction_state(block_index, neo_vm_rs::VmState::HALT, tx)
                .unwrap(),
        ),
    );
}

fn seed_traceable_conflict(
    snapshot: &DataCache,
    hash: &UInt256,
    signer: &UInt160,
    block_index: u32,
) {
    let ledger = LedgerContract::new();
    let stub = ledger.serialize_conflict_stub(block_index).unwrap();

    let mut bare_key = Vec::with_capacity(33);
    bare_key.push(11);
    bare_key.extend_from_slice(&hash.to_bytes());
    snapshot.add(
        StorageKey::new(LedgerContract::ID, bare_key),
        StorageItem::from_bytes(stub.clone()),
    );

    let mut signer_key = Vec::with_capacity(53);
    signer_key.push(11);
    signer_key.extend_from_slice(&hash.to_bytes());
    signer_key.extend_from_slice(&signer.to_bytes());
    snapshot.add(
        StorageKey::new(LedgerContract::ID, signer_key),
        StorageItem::from_bytes(stub),
    );
}

fn seed_gas_balance(snapshot: &DataCache, account: &UInt160, datoshi: i64) {
    let item = StackItem::from_struct(vec![StackItem::from_int(datoshi)]);
    let bytes = BinarySerializer::serialize(&item, &ExecutionEngineLimits::default()).unwrap();
    let mut key = vec![20u8];
    key.extend_from_slice(&account.to_bytes());
    snapshot.update(
        StorageKey::new(neo_native_contracts::GasToken::ID, key),
        StorageItem::from_bytes(bytes),
    );
}

fn signed_zero_fee_tx(settings: &ProtocolSettings, seed: u8) -> Transaction {
    let private = [seed; 32];
    let public = Secp256r1Crypto::derive_public_key(&private).expect("pubkey");
    let verification =
        neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(&public);
    let account = UInt160::from_script(&verification);

    let mut tx = Transaction::new();
    tx.set_nonce(u32::from(seed));
    tx.set_valid_until_block(1);
    tx.set_script(vec![OpCode::PUSH1.byte()]);
    tx.set_signers(vec![Signer::new(account, WitnessScope::NONE)]);

    let hash = tx.try_hash().expect("tx hash");
    let mut sign_data = settings.network.to_le_bytes().to_vec();
    sign_data.extend_from_slice(&hash.to_bytes());
    let signature = Secp256r1Crypto::sign(&sign_data, &private).expect("sign");

    let mut invocation = vec![OpCode::PUSHDATA1.byte(), 64];
    invocation.extend_from_slice(&signature);
    tx.set_witnesses(vec![Witness::new_with_scripts(invocation, verification)]);
    tx
}

fn signing_account(seed: u8) -> ([u8; 32], Vec<u8>, UInt160) {
    let private = [seed; 32];
    let public = Secp256r1Crypto::derive_public_key(&private).expect("pubkey");
    let verification =
        neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(&public);
    let account = UInt160::from_script(&verification);
    (private, public, account)
}

fn consensus_test_validators(count: usize) -> (Vec<ValidatorInfo>, Vec<[u8; 32]>) {
    let mut validators = Vec::with_capacity(count);
    let mut private_keys = Vec::with_capacity(count);

    for index in 0..count {
        let private = [index as u8 + 1; 32];
        let public = Secp256r1Crypto::derive_public_key(&private).expect("pubkey");
        let public_key = ECPoint::from_bytes(&public).expect("ecpoint");
        let verification =
            neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(&public);
        validators.push(ValidatorInfo {
            index: index as u8,
            public_key,
            script_hash: UInt160::from_script(&verification),
        });
        private_keys.push(private);
    }

    (validators, private_keys)
}

#[allow(clippy::too_many_arguments)]
fn signed_tx_with_fees(
    settings: &ProtocolSettings,
    private: &[u8; 32],
    public: &[u8],
    account: UInt160,
    nonce: u32,
    system_fee: i64,
    network_fee: i64,
    attributes: Vec<TransactionAttribute>,
) -> Transaction {
    let verification =
        neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(public);

    let mut tx = Transaction::new();
    tx.set_nonce(nonce);
    tx.set_system_fee(system_fee);
    tx.set_network_fee(network_fee);
    tx.set_valid_until_block(1);
    tx.set_script(vec![OpCode::PUSH1.byte()]);
    tx.set_attributes(attributes);
    tx.set_signers(vec![Signer::new(account, WitnessScope::NONE)]);

    let hash = tx.try_hash().expect("tx hash");
    let mut sign_data = settings.network.to_le_bytes().to_vec();
    sign_data.extend_from_slice(&hash.to_bytes());
    let signature = Secp256r1Crypto::sign(&sign_data, private).expect("sign");

    let mut invocation = vec![OpCode::PUSHDATA1.byte(), 64];
    invocation.extend_from_slice(&signature);
    tx.set_witnesses(vec![Witness::new_with_scripts(invocation, verification)]);
    tx
}
