use neo_core::ledger::BlockHeader;
use neo_core::neo_io::{BinaryWriter, Serializable};
use neo_core::network::p2p::payloads::header::Header;
use neo_core::persistence::{DataCache, StorageItem, StorageKey};
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::ContractParameterType;
use neo_core::smart_contract::contract_basic_method::ContractBasicMethod;
use neo_core::smart_contract::contract_state::{ContractState, NefFile};
use neo_core::smart_contract::helper::Helper;
use neo_core::smart_contract::manifest::{
    ContractAbi, ContractManifest, ContractMethodDescriptor, ContractParameterDefinition,
};
use neo_core::smart_contract::native::trimmed_block::TrimmedBlock;
use neo_core::{IVerifiable, UInt160, UInt256, Witness};
use neo_vm::op_code::OpCode;

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

impl IVerifiable for ManualWitness {
    fn verify(&self) -> bool {
        true
    }

    fn hash(&self) -> neo_core::CoreResult<UInt256> {
        Ok(self.hash)
    }

    fn get_hash_data(&self) -> Vec<u8> {
        self.hash_data.clone()
    }

    fn get_script_hashes_for_verifying(&self, _snapshot: &DataCache) -> Vec<UInt160> {
        self.hashes.clone()
    }

    fn get_witnesses(&self) -> Vec<&Witness> {
        self.witnesses.iter().collect()
    }

    fn get_witnesses_mut(&mut self) -> Vec<&mut Witness> {
        self.witnesses.iter_mut().collect()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[test]
fn verify_witnesses_fails_without_previous_header() {
    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);
    let mut header = Header::new();
    header.set_prev_hash(UInt256::from([1u8; 32]));
    header.witness = Witness::empty();

    assert!(!Helper::verify_witnesses(
        &header, &settings, &snapshot, 100
    ));
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

    assert!(!Helper::verify_witnesses(
        &header, &settings, &snapshot, 100
    ));
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
    let contract = make_contract(next_consensus, vec![OpCode::PUSH1 as u8], manifest);
    store_contract(&snapshot, next_consensus, contract);

    let mut header = Header::new();
    header.set_prev_hash(trimmed.hash());
    header.witness = Witness::empty();

    assert!(!Helper::verify_witnesses(
        &header, &settings, &snapshot, 100
    ));
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
    let script = vec![OpCode::PUSH1 as u8, OpCode::RET as u8];
    let contract = make_contract(next_consensus, script, manifest);
    store_contract(&snapshot, next_consensus, contract);

    let mut header = Header::new();
    header.set_prev_hash(trimmed.hash());
    header.witness = Witness::empty();

    assert!(Helper::verify_witnesses(
        &header, &settings, &snapshot, 1_000_000
    ));
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
        vec![OpCode::PUSH1 as u8, OpCode::RET as u8],
        manifest,
    );
    store_contract(&snapshot, contract_hash, contract);

    let verifiable = ManualWitness::new(vec![contract_hash], vec![Witness::empty()]);
    assert!(Helper::verify_witnesses(
        &verifiable,
        &settings,
        &snapshot,
        1_000_000
    ));
}
