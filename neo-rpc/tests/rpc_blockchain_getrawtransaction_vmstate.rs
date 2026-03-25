use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use neo_core::ledger::{
    block::Block as LedgerBlock, block_header::BlockHeader as LedgerBlockHeader,
};
use neo_core::neo_io::{BinaryWriter, MemoryReader, Serializable, SerializableExt};
use neo_core::network::p2p::payloads::{
    signer::Signer, transaction::Transaction, witness::Witness as PayloadWitness,
};
use neo_core::smart_contract::native::{trimmed_block::TrimmedBlock, LedgerContract};
use neo_core::smart_contract::storage_key::StorageKey;
use neo_core::{UInt160, UInt256, Witness as LedgerWitness, WitnessScope};
use neo_rpc::server::{RpcHandler, RpcServer, RpcServerBlockchain, RpcServerConfig};
use neo_vm::vm_state::VMState;
use serde_json::Value;

fn find_handler<'a>(handlers: &'a [RpcHandler], name: &str) -> &'a RpcHandler {
    handlers
        .iter()
        .find(|handler| handler.descriptor().name == name)
        .expect("handler")
}

fn make_transaction(nonce: u32) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_nonce(nonce);
    tx.set_script(vec![0x51]);
    tx.set_signers(vec![Signer::new(
        UInt160::from_bytes(&[0x11; 20]).expect("account"),
        WitnessScope::CALLED_BY_ENTRY,
    )]);
    tx.set_witnesses(vec![PayloadWitness::empty()]);
    tx
}

fn make_ledger_block(
    store: &neo_core::persistence::StoreCache,
    index: u32,
    transactions: Vec<Transaction>,
) -> LedgerBlock {
    let ledger = LedgerContract::new();
    let prev_hash = if index == 0 {
        UInt256::zero()
    } else {
        ledger
            .get_block_hash_by_index(store, index - 1)
            .expect("previous hash lookup")
            .unwrap_or_else(UInt256::zero)
    };
    let hashes: Vec<UInt256> = transactions.iter().map(|tx| tx.hash()).collect();
    let merkle_root = if hashes.is_empty() {
        UInt256::zero()
    } else {
        neo_core::cryptography::MerkleTree::compute_root(&hashes).unwrap_or_else(UInt256::zero)
    };

    let header = LedgerBlockHeader {
        index,
        previous_hash: prev_hash,
        merkle_root,
        timestamp: 1,
        nonce: 0,
        primary_index: 0,
        next_consensus: UInt160::zero(),
        witnesses: vec![LedgerWitness::empty()],
        ..Default::default()
    };

    LedgerBlock::new(header, transactions)
}

fn store_block(store: &mut neo_core::persistence::StoreCache, block: &LedgerBlock) {
    const PREFIX_BLOCK: u8 = 0x05;
    const PREFIX_BLOCK_HASH: u8 = 0x09;
    const PREFIX_TRANSACTION: u8 = 0x0b;
    const PREFIX_CURRENT_BLOCK: u8 = 0x0c;
    const RECORD_KIND_TRANSACTION: u8 = 0x01;

    let hash = block.hash();
    let index = block.index();

    let mut hash_key_bytes = Vec::with_capacity(1 + 4);
    hash_key_bytes.push(PREFIX_BLOCK_HASH);
    hash_key_bytes.extend_from_slice(&index.to_le_bytes());
    let hash_key = StorageKey::new(LedgerContract::ID, hash_key_bytes);
    store.add(
        hash_key,
        neo_core::smart_contract::StorageItem::from_bytes(hash.to_bytes().to_vec()),
    );

    let trimmed = TrimmedBlock::from_block(block);
    let trimmed_bytes = trimmed.to_array().expect("serialize trimmed block");
    let mut block_key_bytes = Vec::with_capacity(1 + 32);
    block_key_bytes.push(PREFIX_BLOCK);
    block_key_bytes.extend_from_slice(&hash.to_bytes());
    let block_key = StorageKey::new(LedgerContract::ID, block_key_bytes);
    store.add(
        block_key,
        neo_core::smart_contract::StorageItem::from_bytes(trimmed_bytes),
    );

    for tx in &block.transactions {
        let mut writer = BinaryWriter::new();
        writer
            .write_u8(RECORD_KIND_TRANSACTION)
            .expect("record kind");
        writer.write_u32(index).expect("block index");
        writer.write_u8(VMState::HALT as u8).expect("vm state");
        writer.write_var_bytes(&tx.to_bytes()).expect("tx bytes");

        let mut tx_key_bytes = Vec::with_capacity(1 + 32);
        tx_key_bytes.push(PREFIX_TRANSACTION);
        tx_key_bytes.extend_from_slice(&tx.hash().to_bytes());
        let tx_key = StorageKey::new(LedgerContract::ID, tx_key_bytes);
        store.add(
            tx_key,
            neo_core::smart_contract::StorageItem::from_bytes(writer.into_bytes()),
        );
    }

    let mut current_bytes = Vec::with_capacity(36);
    current_bytes.extend_from_slice(&hash.to_bytes());
    current_bytes.extend_from_slice(&index.to_le_bytes());
    let current_key = StorageKey::new(LedgerContract::ID, vec![PREFIX_CURRENT_BLOCK]);
    store.add(
        current_key,
        neo_core::smart_contract::StorageItem::from_bytes(current_bytes),
    );
    store.commit();
}

#[tokio::test(flavor = "multi_thread")]
async fn get_raw_transaction_verbose_confirmed_includes_vmstate() {
    let system = neo_core::neo_system::NeoSystem::new(
        neo_core::protocol_settings::ProtocolSettings::default(),
        None,
        None,
    )
    .expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getrawtransaction");

    let tx = make_transaction(7);
    let block = make_ledger_block(&system.context().store_cache(), 1, vec![tx.clone()]);
    let block_hash = block.hash();
    let mut store = system.context().store_snapshot_cache();
    store_block(&mut store, &block);

    let params = [Value::String(tx.hash().to_string()), Value::Bool(true)];
    let result = (handler.callback())(&server, &params).expect("get raw tx verbose");
    let obj = result.as_object().expect("object");
    assert_eq!(
        obj.get("blockhash")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        block_hash.to_string()
    );
    assert_eq!(
        obj.get("vmstate")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        "HALT"
    );

    let bytes = BASE64_STANDARD
        .decode(
            (handler.callback())(
                &server,
                &[Value::String(tx.hash().to_string()), Value::Bool(false)],
            )
            .expect("base64 response")
            .as_str()
            .expect("base64"),
        )
        .expect("decode");
    let mut reader = MemoryReader::new(&bytes);
    let decoded = <Transaction as Serializable>::deserialize(&mut reader).expect("tx");
    assert_eq!(decoded.hash(), tx.hash());
}
