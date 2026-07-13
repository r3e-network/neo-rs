//! Regression coverage for block-aware native persist hook gates.

use super::*;
use neo_config::Hardfork;
use neo_payloads::{NotaryAssisted, OracleResponse, Signer, TransactionAttribute, Witness};
use neo_primitives::{OracleResponseCode, WitnessScope};
use neo_storage::StorageItem;

const NOTARY_ASSISTED_FEE_PER_KEY: i64 = 1000_0000;
const ORACLE_PREFIX_REQUEST: u8 = 7;

fn transaction_block(index: u32, transaction: neo_payloads::Transaction) -> Arc<Block> {
    let mut header = Header::new();
    header.set_index(index);
    let mut block = Block::from_parts(header, vec![transaction]);
    block
        .try_rebuild_merkle_root()
        .expect("rebuild dynamic-hook test Merkle root");
    Arc::new(block)
}

#[test]
fn notary_assisted_block_runs_notary_on_persist_gate() {
    let mut settings = ProtocolSettings::default();
    settings.hardforks.insert(Hardfork::HfEchidna, 0);
    let snapshot = Arc::new(DataCache::new(false));
    let resources = standard_resources();
    let genesis = Arc::new(genesis_block(&settings).expect("genesis block"));
    persist_with_resources(Arc::clone(&snapshot), genesis, &settings, &resources)
        .expect("persist genesis");

    let sender = UInt160::from([0x71; 20]);
    let notary_fee = 2 * NOTARY_ASSISTED_FEE_PER_KEY;
    fund_gas(snapshot.as_ref(), &sender, notary_fee);

    let mut transaction = neo_payloads::Transaction::new();
    transaction.set_script(vec![neo_vm::OpCode::RET.byte()]);
    transaction.set_network_fee(notary_fee);
    transaction.set_signers(vec![Signer::new(sender, WitnessScope::NONE)]);
    transaction.set_witnesses(vec![Witness::empty()]);
    transaction.set_attributes(vec![TransactionAttribute::NotaryAssisted(
        NotaryAssisted::new(1),
    )]);

    let result = stage_block_natives_with_resources(
        Arc::clone(&snapshot),
        transaction_block(1, transaction),
        Arc::new(settings),
        NativePersistOptions::default(),
        &resources,
    );
    let error = match result {
        Ok(_) => panic!("Notary-assisted block skipped the Notary OnPersist hook"),
        Err(error) => error,
    };
    assert!(
        error
            .to_string()
            .contains("NotaryAssisted fees with no designated P2PNotary nodes"),
        "unexpected Notary hook error: {error}"
    );
}

#[test]
fn oracle_response_block_runs_oracle_post_persist_gate() {
    let settings = ProtocolSettings::default();
    let snapshot = Arc::new(DataCache::new(false));
    let resources = standard_resources();
    let genesis = Arc::new(genesis_block(&settings).expect("genesis block"));
    persist_with_resources(Arc::clone(&snapshot), genesis, &settings, &resources)
        .expect("persist genesis");

    let request_id = 17u64;
    let mut request_key = vec![ORACLE_PREFIX_REQUEST];
    request_key.extend_from_slice(&request_id.to_be_bytes());
    snapshot.add(
        StorageKey::new(neo_native_contracts::OracleContract::ID, request_key),
        StorageItem::from_bytes(vec![0xff]),
    );

    let sender = UInt160::from([0x72; 20]);
    fund_gas(snapshot.as_ref(), &sender, 1_0000_0000);
    let mut transaction = neo_payloads::Transaction::new();
    transaction.set_script(vec![neo_vm::OpCode::RET.byte()]);
    transaction.set_system_fee(1_0000_0000);
    transaction.set_signers(vec![Signer::new(sender, WitnessScope::NONE)]);
    transaction.set_witnesses(vec![Witness::empty()]);
    transaction.set_attributes(vec![TransactionAttribute::OracleResponse(
        OracleResponse::new(request_id, OracleResponseCode::Success, Vec::new()),
    )]);

    let result = stage_block_natives_with_resources(
        Arc::clone(&snapshot),
        transaction_block(1, transaction),
        Arc::new(settings),
        NativePersistOptions::default(),
        &resources,
    );
    let error = match result {
        Ok(_) => panic!("Oracle-response block skipped the Oracle PostPersist hook"),
        Err(error) => error,
    };
    assert!(
        error.to_string().contains("OracleContract"),
        "unexpected Oracle hook error: {error}"
    );
}
