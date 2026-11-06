use super::resolver::VerifiableExt;
use super::witness::verify_witnesses;
use neo_io::{BinaryWriter, Serializable};
use neo_native_contracts::{LedgerContract, trimmed_block::TrimmedBlock};
use neo_primitives::{
    ContractBasicMethod, ContractParameterType, SerializablePayload, UInt160, UInt256, Verifiable,
};
use neo_protocol::execution::BlockHeader;
use neo_protocol::payloads::{Block, Header, Transaction};
use neo_protocol::protocol_settings::ProtocolSettings;
use neo_protocol::witness::{Witness, WitnessData};
use neo_smart_contract::contract_state::{ContractState, NefFile};
use neo_smart_contract::manifest::{
    ContractAbi, ContractManifest, ContractMethodDescriptor, ContractParameterDefinition,
};
use neo_storage::persistence::DataCache;
use neo_storage::{StorageItem, StorageKey};
use neo_vm::OpCode;

const CONTRACT_MANAGEMENT_ID: i32 = -1;
const LEDGER_CONTRACT_ID: i32 = -4;
const PREFIX_CONTRACT: u8 = 8;
const PREFIX_BLOCK: u8 = 5;

fn contract_hash(seed: u8) -> UInt160 {
    UInt160::from_bytes(&[seed; 20]).expect("hash")
}

fn store_contract(snapshot: &DataCache, hash: UInt160, contract: ContractState) {
    let mut writer = BinaryWriter::new();
    contract.serialize(&mut writer).expect("serialize contract");
    let mut key = Vec::with_capacity(1 + UInt160::LENGTH);
    key.push(PREFIX_CONTRACT);
    key.extend_from_slice(&hash.to_bytes());
    snapshot.add(
        StorageKey::new(CONTRACT_MANAGEMENT_ID, key),
        StorageItem::from_bytes(writer.into_bytes()),
    );
}

fn store_trimmed_block(snapshot: &DataCache, trimmed: &TrimmedBlock) {
    let mut writer = BinaryWriter::new();
    trimmed.serialize(&mut writer).expect("serialize trimmed");
    let mut key = Vec::with_capacity(1 + UInt256::LENGTH);
    key.push(PREFIX_BLOCK);
    key.extend_from_slice(&trimmed.hash().to_bytes());
    snapshot.add(
        StorageKey::new(LEDGER_CONTRACT_ID, key),
        StorageItem::from_bytes(writer.into_bytes()),
    );
}

fn make_trimmed_block(next_consensus: UInt160) -> TrimmedBlock {
    let header = BlockHeader::new(
        0,
        UInt256::zero(),
        UInt256::zero(),
        1,
        0,
        1,
        0,
        next_consensus,
        vec![Witness::empty()],
    );
    TrimmedBlock::create(header, vec![UInt256::zero()])
}

fn make_contract_manifest(
    name: &str,
    method_name: &str,
    return_type: ContractParameterType,
) -> ContractManifest {
    let mut manifest = ContractManifest::new(name.to_string());
    let method = ContractMethodDescriptor::new(
        method_name.to_string(),
        Vec::<ContractParameterDefinition>::new(),
        return_type,
        0,
        false,
    )
    .expect("method");
    manifest.abi = ContractAbi::new(vec![method], Vec::new());
    manifest
}

fn make_contract(hash: UInt160, script: Vec<u8>, manifest: ContractManifest) -> ContractState {
    let nef = NefFile::new("test".to_string(), script);
    ContractState::new(1, hash, nef, manifest)
}

struct ManualWitness {
    hashes: Vec<UInt160>,
    witnesses: Vec<Witness>,
    hash: UInt256,
    hash_data: Vec<u8>,
}

impl ManualWitness {
    fn new(hashes: Vec<UInt160>, witnesses: Vec<Witness>) -> Self {
        let hash = UInt256::from([9u8; 32]);
        let hash_data = hash.to_bytes();
        Self {
            hashes,
            witnesses,
            hash,
            hash_data,
        }
    }
}

impl Verifiable for ManualWitness {
    fn verify(&self) -> bool {
        true
    }
    fn hash(&self) -> neo_primitives::PrimitiveResult<UInt256> {
        Ok(self.hash)
    }
    fn hash_data(&self) -> Vec<u8> {
        self.hash_data.clone()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl VerifiableExt for ManualWitness {
    fn script_hashes_for_verifying(&self, _snapshot: &DataCache) -> Vec<UInt160> {
        self.hashes.clone()
    }
}

impl WitnessData for ManualWitness {
    fn witnesses(&self) -> Vec<&Witness> {
        self.witnesses.iter().collect()
    }
    fn witnesses_mut(&mut self) -> Vec<&mut Witness> {
        self.witnesses.iter_mut().collect()
    }
}

#[test]
fn header_genesis_verification_hash_is_its_witness_script_hash() {
    let snapshot = DataCache::new(false);
    let unrelated = make_trimmed_block(contract_hash(0x31));
    store_trimmed_block(&snapshot, &unrelated);
    let mut header = Header::new();
    header.set_next_consensus(contract_hash(0x32));
    header.witness =
        Witness::new_with_scripts(Vec::new(), vec![OpCode::PUSHT.byte(), OpCode::RET.byte()]);
    let witness_hash = header.witness.script_hash();
    assert_eq!(*header.prev_hash(), UInt256::zero());
    assert_ne!(witness_hash, *header.next_consensus());
    assert_ne!(witness_hash, unrelated.header.next_consensus);
    assert_eq!(
        header.script_hashes_for_verifying(&snapshot),
        vec![witness_hash]
    );
}

#[test]
fn header_verification_hash_uses_persisted_previous_consensus() {
    let snapshot = DataCache::new(false);
    let previous = make_trimmed_block(contract_hash(0x41));
    let unrelated = make_trimmed_block(contract_hash(0x42));
    store_trimmed_block(&snapshot, &previous);
    store_trimmed_block(&snapshot, &unrelated);
    let mut header = Header::new();
    header.set_prev_hash(previous.hash());
    header.set_index(previous.index() + 1);
    header.set_next_consensus(contract_hash(0x43));
    header.witness =
        Witness::new_with_scripts(Vec::new(), vec![OpCode::PUSHT.byte(), OpCode::RET.byte()]);
    let persisted = LedgerContract::new()
        .get_trimmed_block(&snapshot, header.prev_hash())
        .expect("previous block lookup")
        .expect("persisted previous block");
    assert_eq!(persisted.header.next_consensus, contract_hash(0x41));
    assert_ne!(previous.hash(), unrelated.hash());
    assert_ne!(persisted.header.next_consensus, *header.next_consensus());
    assert_ne!(
        persisted.header.next_consensus,
        unrelated.header.next_consensus
    );
    assert_ne!(
        persisted.header.next_consensus,
        header.witness.script_hash()
    );
    assert_eq!(
        header.script_hashes_for_verifying(&snapshot),
        vec![persisted.header.next_consensus]
    );
}

#[test]
fn header_verification_hashes_are_empty_when_previous_block_is_missing() {
    let snapshot = DataCache::new(false);
    let unrelated = make_trimmed_block(contract_hash(0x51));
    store_trimmed_block(&snapshot, &unrelated);
    let mut header = Header::new();
    header.set_prev_hash(UInt256::from([0x52; 32]));
    header.set_next_consensus(contract_hash(0x53));
    header.witness = Witness::new_with_scripts(Vec::new(), vec![OpCode::PUSHT.byte()]);
    assert_ne!(*header.prev_hash(), unrelated.hash());
    assert!(
        LedgerContract::new()
            .get_trimmed_block(&snapshot, header.prev_hash())
            .expect("missing previous block lookup")
            .is_none()
    );
    assert!(header.script_hashes_for_verifying(&snapshot).is_empty());
}

#[test]
fn header_verification_hashes_are_empty_when_previous_block_decode_fails() {
    let snapshot = DataCache::new(false);
    let mut header = Header::new();
    header.set_prev_hash(UInt256::from([0x61; 32]));
    header.set_next_consensus(contract_hash(0x62));
    header.witness = Witness::new_with_scripts(Vec::new(), vec![OpCode::PUSHT.byte()]);
    let mut key = vec![PREFIX_BLOCK];
    key.extend_from_slice(&header.prev_hash().to_bytes());
    snapshot.add(
        StorageKey::new(LEDGER_CONTRACT_ID, key),
        StorageItem::from_bytes(vec![0xff]),
    );
    assert!(
        LedgerContract::new()
            .get_trimmed_block(&snapshot, header.prev_hash())
            .is_err()
    );
    assert!(header.script_hashes_for_verifying(&snapshot).is_empty());
}

#[test]
fn block_witness_data_borrows_and_mutates_only_the_header_witness() {
    let mut block = Block::new();
    block.header.witness = Witness::new_with_scripts(vec![0x70], vec![0x71]);
    let first_witnesses = vec![
        Witness::new_with_scripts(vec![0x72], vec![0x73]),
        Witness::new_with_scripts(vec![0x74], vec![0x75]),
    ];
    let second_witnesses = vec![Witness::new_with_scripts(vec![0x76], vec![0x77])];
    let mut first = Transaction::new();
    first.set_witnesses(first_witnesses.clone());
    let mut second = Transaction::new();
    second.set_witnesses(second_witnesses.clone());
    block.transactions = vec![first, second];
    {
        let witnesses = WitnessData::witnesses(&block);
        assert_eq!(witnesses.len(), 1);
        assert!(std::ptr::eq(witnesses[0], &block.header.witness));
        assert_eq!(witnesses[0].invocation_script, vec![0x70]);
    }
    assert_eq!(SerializablePayload::witness_count(&block), 4);
    assert_eq!(SerializablePayload::invocation_script(&block, 1), &[0x72]);
    assert_eq!(SerializablePayload::invocation_script(&block, 2), &[0x74]);
    assert_eq!(SerializablePayload::invocation_script(&block, 3), &[0x76]);
    {
        let mut witnesses = WitnessData::witnesses_mut(&mut block);
        assert_eq!(witnesses.len(), 1);
        witnesses[0].invocation_script = vec![0x78, 0x79];
        witnesses[0].verification_script = vec![0x7a];
    }
    assert_eq!(block.header.witness.invocation_script, vec![0x78, 0x79]);
    assert_eq!(block.header.witness.verification_script, vec![0x7a]);
    assert_eq!(
        block.transactions[0].witnesses(),
        first_witnesses.as_slice()
    );
    assert_eq!(
        block.transactions[1].witnesses(),
        second_witnesses.as_slice()
    );
    assert_eq!(WitnessData::witnesses(&block).len(), 1);
    assert_eq!(SerializablePayload::witness_count(&block), 4);
    assert_eq!(
        SerializablePayload::invocation_script(&block, 0),
        &[0x78, 0x79]
    );
}

#[test]
fn verify_witnesses_fails_without_previous_header() {
    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);
    let mut header = Header::new();
    header.set_prev_hash(UInt256::from([1u8; 32]));
    header.witness = Witness::empty();
    assert!(!verify_witnesses(&header, &settings, &snapshot, 100));
}

#[test]
fn verify_witnesses_fails_when_contract_missing() {
    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);
    let next_consensus = contract_hash(1);
    let trimmed = make_trimmed_block(next_consensus);
    store_trimmed_block(&snapshot, &trimmed);
    let mut header = Header::new();
    header.set_prev_hash(trimmed.hash());
    header.witness = Witness::empty();
    assert!(!verify_witnesses(&header, &settings, &snapshot, 100));
}

#[test]
fn verify_witnesses_fails_with_invalid_verify_method() {
    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);
    let next_consensus = contract_hash(2);
    let trimmed = make_trimmed_block(next_consensus);
    store_trimmed_block(&snapshot, &trimmed);
    let manifest = make_contract_manifest(
        "invalid",
        ContractBasicMethod::VERIFY,
        ContractParameterType::Integer,
    );
    let contract = make_contract(next_consensus, vec![OpCode::PUSH1.byte()], manifest);
    store_contract(&snapshot, next_consensus, contract);
    let mut header = Header::new();
    header.set_prev_hash(trimmed.hash());
    header.witness = Witness::empty();
    assert!(!verify_witnesses(&header, &settings, &snapshot, 100));
}

#[test]
fn verify_witnesses_succeeds_with_verify_method() {
    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);
    let next_consensus = contract_hash(3);
    let trimmed = make_trimmed_block(next_consensus);
    store_trimmed_block(&snapshot, &trimmed);
    let manifest = make_contract_manifest(
        "valid",
        ContractBasicMethod::VERIFY,
        ContractParameterType::Boolean,
    );
    let script = vec![OpCode::PUSH1.byte(), OpCode::RET.byte()];
    let contract = make_contract(next_consensus, script, manifest);
    store_contract(&snapshot, next_consensus, contract);
    let mut header = Header::new();
    header.set_prev_hash(trimmed.hash());
    header.witness = Witness::empty();
    assert!(verify_witnesses(&header, &settings, &snapshot, 1_000_000));
}

#[test]
fn verify_witnesses_succeeds_with_manual_verifiable() {
    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);
    let contract_hash = contract_hash(4);
    let parameters = vec![
        ContractParameterDefinition::new("signature".to_string(), ContractParameterType::Signature)
            .expect("parameter"),
    ];
    let method = ContractMethodDescriptor::new(
        ContractBasicMethod::VERIFY.to_string(),
        parameters,
        ContractParameterType::Boolean,
        0,
        false,
    )
    .expect("method");
    let mut manifest = ContractManifest::new("verify".to_string());
    manifest.abi = ContractAbi::new(vec![method], Vec::new());
    let contract = make_contract(
        contract_hash,
        vec![OpCode::PUSH1.byte(), OpCode::RET.byte()],
        manifest,
    );
    store_contract(&snapshot, contract_hash, contract);
    let verifiable = ManualWitness::new(vec![contract_hash], vec![Witness::empty()]);
    assert!(verify_witnesses(
        &verifiable,
        &settings,
        &snapshot,
        1_000_000
    ));
}
