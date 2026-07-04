use super::super::{OracleService, OracleServiceSettings};
use neo_crypto::{ECCurve, ECPoint, Secp256r1Crypto};
use neo_payloads::{OracleResponse, OracleResponseCode, Signer, Transaction, Witness};
use neo_storage::{DataCache, StorageKey};

use neo_config::ProtocolSettings;
use neo_io::Serializable;
use neo_native_contracts::{LedgerContract, OracleRequest};
use neo_primitives::{UInt160, UInt256, WitnessScope};
use neo_storage::StorageItem;
use neo_vm_rs::VmState as VMState;

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
    // Seed through the canonical ledger codec (the C# interoperable
    // `TransactionState` layout) so the fixture stays byte-identical
    // to what the persist pipeline writes.
    let record = LedgerContract::new()
        .serialize_persisted_transaction_state(block_index, VMState::NONE, tx)
        .expect("transaction state record");

    let mut key = Vec::with_capacity(1 + 32);
    key.push(11);
    key.extend_from_slice(&tx_hash.to_bytes());
    let storage_key = StorageKey::new(LedgerContract::ID, key);
    snapshot.add(storage_key, StorageItem::from_bytes(record));
}

/// Seed the persisted native Oracle contract record into the snapshot so
/// `verify` executes through `System.Contract.CallNative`, including the
/// C# `[ContractMethod(CpuFee = 1 << 15)]` charge.
fn seed_oracle_contract(snapshot: &DataCache, settings: &ProtocolSettings) {
    use neo_execution::native_contract::build_native_contract_state;
    use neo_native_contracts::{ContractManagement, OracleContract};

    let state = build_native_contract_state(&OracleContract::new(), settings, 0);

    let bytes = state
        .serialize_contract_record()
        .expect("serialize contract record");

    let mut key = Vec::with_capacity(1 + 20);
    key.push(8); // PREFIX_CONTRACT
    key.extend_from_slice(&state.hash.to_bytes());
    let storage_key = StorageKey::new(ContractManagement::ID, key);
    snapshot.add(storage_key, StorageItem::from_bytes(bytes));
}

#[test]
fn create_response_tx_matches_csharp_fee_math() {
    let settings = std::sync::Arc::new(ProtocolSettings::testnet());
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
    origin_tx.set_script(vec![neo_vm_rs::OpCode::RET.byte()]);
    origin_tx.set_witnesses(vec![Witness::empty()]);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let _guard = runtime.enter();
    let system = std::sync::Arc::new(
        neo_system::Node::new(std::sync::Arc::clone(&settings), None, None).expect("neo system"),
    );
    let oracle_settings = OracleServiceSettings {
        network: settings.network,
        ..Default::default()
    };
    let service =
        OracleService::new(oracle_settings, system.clone(), system.clone(), system.clone()).expect("oracle service");
    let snapshot = service.snapshot_cache();

    seed_transaction_state(&snapshot, &request.original_tx_id, &origin_tx, 1);
    seed_oracle_contract(&snapshot, settings.as_ref());

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
    // Native Oracle verify contributes `(1 << 15) * ExecFeeFactor`, matching C#.
    assert_eq!(2_198_650, tx.network_fee());
    assert_eq!(97_801_350, tx.system_fee());

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
    assert_eq!(2_197_650, tx.network_fee());
    assert_eq!(7_802_350, tx.system_fee());
}
