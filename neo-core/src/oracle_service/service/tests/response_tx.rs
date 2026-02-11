use super::super::{OracleService, OracleServiceSettings};
use crate::cryptography::{ECCurve, ECPoint, Secp256r1Crypto};
use crate::neo_io::{BinaryWriter, Serializable};
use crate::network::p2p::payloads::{
    OracleResponse, OracleResponseCode, Signer, Transaction, Witness,
};
use crate::persistence::storage_key::StorageKey;
use crate::persistence::DataCache;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::native::{LedgerContract, OracleRequest};
use crate::smart_contract::StorageItem;
use crate::{UInt160, UInt256, WitnessScope};
use neo_vm::VMState;

fn sample_point(byte: u8) -> ECPoint {
    let mut private_key = [0u8; 32];
    private_key[31] = byte.max(1);
    let public_key = Secp256r1Crypto::derive_public_key(&private_key).expect("derive test key");
    ECPoint::decode_compressed_with_curve(ECCurve::secp256r1(), &public_key)
        .expect("static test key")
}

fn seed_transaction_state(
    snapshot: &DataCache,
    tx_hash: &UInt256,
    tx: &Transaction,
    block_index: u32,
) {
    let mut writer = BinaryWriter::new();
    writer.write_u8(0x01).expect("transaction record marker");
    writer.write_u32(block_index).expect("block index");
    writer.write_u8(VMState::NONE as u8).expect("vm state");
    let mut tx_writer = BinaryWriter::new();
    tx.serialize(&mut tx_writer).expect("serialize tx");
    writer
        .write_var_bytes(&tx_writer.into_bytes())
        .expect("tx bytes");

    let mut key = Vec::with_capacity(1 + 32);
    key.push(11);
    key.extend_from_slice(&tx_hash.to_bytes());
    let storage_key = StorageKey::new(LedgerContract::ID, key);
    snapshot.add(storage_key, StorageItem::from_bytes(writer.into_bytes()));
}

#[test]
fn create_response_tx_matches_csharp_fee_math() {
    let settings = ProtocolSettings::testnet();
    let mut request = OracleRequest::new(
        UInt256::zero(),
        100_000_000,
        "https://127.0.0.1/test".to_string(),
        Some(String::new()),
        UInt160::zero(),
        "callback".to_string(),
        Vec::new(),
    );

    let mut origin_tx = Transaction::new();
    origin_tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::NONE)]);
    origin_tx.set_attributes(Vec::new());
    origin_tx.set_valid_until_block(1);
    origin_tx.set_script(vec![neo_vm::op_code::OpCode::RET as u8]);
    origin_tx.set_witnesses(vec![Witness::empty()]);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let _guard = runtime.enter();
    let system =
        crate::neo_system::NeoSystem::new(settings.clone(), None, None).expect("neo system");
    let oracle_settings = OracleServiceSettings { network: settings.network, ..Default::default() };
    let service = OracleService::new(oracle_settings, system).expect("oracle service");
    let snapshot = service.snapshot_cache();

    seed_transaction_state(&snapshot, &request.original_tx_id, &origin_tx, 1);

    let oracle_nodes = vec![sample_point(0x01)];
    let mut response = OracleResponse::new(1, OracleResponseCode::Success, vec![0x00]);
    let tx = service
        .create_response_tx(
            &snapshot,
            &request,
            &mut response,
            &oracle_nodes,
            &settings,
            false,
        )
        .expect("response tx");

    assert_eq!(166, tx.size());
    assert_eq!(1_248_349, tx.network_fee());
    assert_eq!(98_751_651, tx.system_fee());

    request.gas_for_response = 10_000_000;
    response.result = vec![0u8; 10_250];
    let tx = service
        .create_response_tx(
            &snapshot,
            &request,
            &mut response,
            &oracle_nodes,
            &settings,
            false,
        )
        .expect("response tx");

    assert_eq!(165, tx.size());
    assert_eq!(OracleResponseCode::InsufficientFunds, response.code);
    assert_eq!(1_247_349, tx.network_fee());
    assert_eq!(8_752_651, tx.system_fee());
}
