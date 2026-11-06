//! Response-transaction fee/witness parity test for the neo-core
//! [`CoreResponseTxBuilder`] adapter (pins the C# fee math).

use crate::network::p2p::payloads::{OracleResponseCode, Signer, Transaction, Witness};
use crate::oracle_service::adapters::{CoreResponseTxBuilder, CoreSnapshot};
use neo_core::cryptography::{ECCurve, ECPoint, Secp256r1Crypto};
use neo_core::persistence::StoreCache;
use neo_core::persistence::storage_key::StorageKey;
use neo_core::smart_contract::StorageItem;
use neo_core::smart_contract::native::LedgerContract;
use neo_io_crate::{BinaryWriter, Serializable};
use neo_oracle::OracleRequest;
use neo_oracle::ports::ResponseTxBuilder;
use neo_primitives::{UInt160, UInt256, WitnessScope};
use neo_protocol::protocol_settings::ProtocolSettings;
use neo_vm::VmState as VMState;

fn sample_point(byte: u8) -> ECPoint {
    let mut private_key = [0u8; 32];
    private_key[31] = byte.max(1);
    let public_key = Secp256r1Crypto::derive_public_key(&private_key).expect("derive test key");
    ECPoint::decode_compressed_with_curve(ECCurve::secp256r1(), &public_key)
        .expect("static test key")
}

fn seed_transaction_state(
    snapshot: &neo_core::persistence::DataCache,
    tx_hash: &UInt256,
    tx: &Transaction,
    block_index: u32,
) {
    let mut writer = BinaryWriter::new();
    writer.write_u8(0x01).expect("transaction record marker");
    writer.write_u32(block_index).expect("block index");
    writer.write_u8(VMState::NONE.to_byte()).expect("vm state");
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
    origin_tx.set_script(vec![neo_vm::OpCode::RET.byte()]);
    origin_tx.set_witnesses(vec![Witness::empty()]);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let _guard = runtime.enter();
    let system =
        crate::neo_system::NeoSystem::new(settings.clone(), None, None).expect("neo system");
    // Same snapshot dance as the old `snapshot_cache` helper.
    let store_snapshot = system.store().snapshot();
    let store_cache = StoreCache::new_from_snapshot(store_snapshot);
    let data_cache = store_cache.data_cache().clone();

    seed_transaction_state(&data_cache, &request.original_tx_id, &origin_tx, 1);
    let snapshot = CoreSnapshot::new(data_cache, settings.clone());

    let builder = CoreResponseTxBuilder::new(system);
    let oracle_nodes = vec![sample_point(0x01)];
    let (tx, code) = builder
        .build_response_tx(
            &snapshot,
            &request,
            1,
            OracleResponseCode::Success,
            vec![0x00],
            &oracle_nodes,
            settings.network,
            false,
        )
        .expect("response tx");

    assert_eq!(166, tx.size());
    // Post-fix values (commit 4f599eb2 corrected 30× cpu_fee undercharge to match C#).
    assert_eq!(2_198_650, tx.network_fee());
    assert_eq!(97_801_350, tx.system_fee());
    assert_eq!(code, OracleResponseCode::Success);

    request.gas_for_response = 10_000_000;
    let (tx, code) = builder
        .build_response_tx(
            &snapshot,
            &request,
            1,
            OracleResponseCode::Success,
            vec![0u8; 10_250],
            &oracle_nodes,
            settings.network,
            false,
        )
        .expect("response tx");

    assert_eq!(165, tx.size());
    assert_eq!(code, OracleResponseCode::InsufficientFunds);
    assert_eq!(2_197_650, tx.network_fee());
    assert_eq!(7_802_350, tx.system_fee());
}
