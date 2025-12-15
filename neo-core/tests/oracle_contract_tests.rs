use neo_core::cryptography::{ECCurve, ECPoint, NeoHash, Secp256r1Crypto};
use neo_core::ledger::{block::Block, block_header::BlockHeader};
use neo_core::network::p2p::payloads::{
    oracle_response::OracleResponse, oracle_response_code::OracleResponseCode,
    transaction::Transaction, transaction_attribute::TransactionAttribute,
};
use neo_core::persistence::DataCache;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::application_engine::ApplicationEngine;
use neo_core::smart_contract::native::{
    GasToken, LedgerContract, NativeContract, OracleContract, Role, RoleManagement,
};
use neo_core::smart_contract::storage_item::StorageItem;
use neo_core::smart_contract::storage_key::StorageKey;
use neo_core::smart_contract::trigger_type::TriggerType;
use neo_core::smart_contract::Contract;
use neo_core::{IVerifiable, UInt160};
use num_bigint::BigInt;
use std::sync::Arc;

fn sample_point(byte: u8) -> ECPoint {
    let private_key = {
        let mut bytes = [0u8; 32];
        bytes[31] = byte.max(1);
        bytes
    };
    let public_key = Secp256r1Crypto::derive_public_key(&private_key).expect("derive test key");
    ECPoint::decode_compressed_with_curve(ECCurve::secp256r1(), &public_key)
        .expect("static test key")
}

fn serialize_nodes(nodes: &[ECPoint]) -> Vec<u8> {
    let mut buffer = Vec::with_capacity(4 + nodes.len() * 33);
    buffer.extend_from_slice(&(nodes.len() as u32).to_le_bytes());
    for node in nodes {
        let encoded = node.encode_compressed().expect("compressible");
        buffer.extend_from_slice(&encoded);
    }
    buffer
}

fn setup_engine(snapshot: Arc<DataCache>, block: Block) -> ApplicationEngine {
    let script_container: Arc<dyn IVerifiable> = Arc::new(Transaction::new());
    ApplicationEngine::new(
        TriggerType::PostPersist,
        Some(script_container),
        snapshot,
        Some(block),
        ProtocolSettings::default_settings(),
        200_000_000,
        None,
    )
    .expect("engine")
}

#[test]
fn oracle_post_persist_mints_gas_for_designated_nodes() {
    let snapshot = Arc::new(DataCache::new(false));
    let header = BlockHeader {
        index: 7,
        timestamp: 1_700_000_000,
        ..Default::default()
    };

    let mut tx = Transaction::new();
    tx.set_attributes(vec![TransactionAttribute::OracleResponse(OracleResponse {
        id: 42,
        code: OracleResponseCode::Success,
        result: Vec::new(),
    })]);

    let block = Block::new(header, vec![tx]);
    let engine_snapshot = Arc::clone(&snapshot);
    let mut engine = setup_engine(engine_snapshot, block);

    // Seed RoleManagement storage with a single oracle node designation at height 7.
    let role_contract = RoleManagement::new();
    let oracle_point = sample_point(0xAB);
    let mut suffix = vec![Role::Oracle as u8];
    suffix.extend_from_slice(&7u32.to_be_bytes());
    let key = StorageKey::new(role_contract.id(), suffix);
    let serialized = serialize_nodes(std::slice::from_ref(&oracle_point));
    snapshot.add(key, StorageItem::from_bytes(serialized));
    seed_ledger_current_index(&snapshot, 7);

    let oracle = OracleContract::new();
    engine.set_current_script_hash(Some(oracle.hash()));
    oracle
        .post_persist(&mut engine)
        .expect("post persist succeeds");
    engine.set_current_script_hash(None);

    // GAS should be minted to the script hash derived from the designated node.
    let script = Contract::create_signature_redeem_script(oracle_point);
    let account =
        UInt160::from_bytes(&NeoHash::hash160(&script)).expect("convert designated account");
    let gas = GasToken::new();
    let balance = gas.balance_of_snapshot(snapshot.as_ref(), &account);
    let expected = BigInt::from(oracle.get_price(snapshot.as_ref()));

    assert_eq!(balance, expected, "designated node should receive reward");
}
fn seed_ledger_current_index(snapshot: &Arc<DataCache>, index: u32) {
    const PREFIX_CURRENT_BLOCK: u8 = 12;
    let ledger = LedgerContract::new();
    let key = StorageKey::new(ledger.id(), vec![PREFIX_CURRENT_BLOCK]);
    let mut bytes = vec![0u8; 32];
    bytes.extend_from_slice(&index.to_le_bytes());
    snapshot.add(key, StorageItem::from_bytes(bytes));
}
