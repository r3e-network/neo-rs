#![cfg(feature = "server")]

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_io::{MemoryReader, Serializable, SerializableExtensions};
use neo_native_contracts::LedgerContract;
use neo_payloads::TrimmedBlock;
use neo_payloads::Witness as LedgerWitness;
use neo_payloads::{Block as LedgerBlock, BlockHeader as LedgerBlockHeader};
use neo_payloads::{signer::Signer, transaction::Transaction, witness::Witness as PayloadWitness};
use neo_primitives::{UInt160, UInt256, WitnessScope};
use neo_rpc::server::{RpcHandler, RpcServer, RpcServerBlockchain, RpcServerConfig};
use neo_storage::StorageKey;
use neo_vm_rs::VmState as VMState;
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
    store: &neo_storage::persistence::StoreCache,
    index: u32,
    transactions: Vec<Transaction>,
) -> LedgerBlock {
    let ledger = LedgerContract::new();
    let prev_hash = if index == 0 {
        UInt256::zero()
    } else {
        ledger
            .get_block_hash(store.data_cache(), index - 1)
            .expect("previous hash lookup")
            .unwrap_or_else(UInt256::zero)
    };
    let hashes: Vec<UInt256> = transactions.iter().map(|tx| tx.hash()).collect();
    let merkle_root = if hashes.is_empty() {
        UInt256::zero()
    } else {
        neo_crypto::MerkleTree::compute_root(&hashes).unwrap_or_else(UInt256::zero)
    };

    let header = LedgerBlockHeader::new_with_witnesses(
        0,
        prev_hash,
        merkle_root,
        1,
        0,
        index,
        0,
        UInt160::zero(),
        vec![LedgerWitness::empty()],
    );

    LedgerBlock::from_parts(header, transactions)
}

fn store_block(store: &mut neo_storage::persistence::StoreCache, block: &LedgerBlock) {
    const PREFIX_BLOCK: u8 = 0x05;
    const PREFIX_BLOCK_HASH: u8 = 0x09;
    const PREFIX_TRANSACTION: u8 = 0x0b;
    const PREFIX_CURRENT_BLOCK: u8 = 0x0c;

    let hash = block.hash();
    let index = block.index();

    // C# `CreateStorageKey(Prefix_BlockHash, uint)` uses `KeyBuilder.AddBigEndian`
    // — the index is stored big-endian so the server's lookup matches.
    let mut hash_key_bytes = Vec::with_capacity(1 + 4);
    hash_key_bytes.push(PREFIX_BLOCK_HASH);
    hash_key_bytes.extend_from_slice(&index.to_be_bytes());
    let hash_key = StorageKey::new(LedgerContract::ID, hash_key_bytes);
    store.add(
        hash_key,
        neo_storage::StorageItem::from_bytes(hash.to_bytes().to_vec()),
    );

    let trimmed = TrimmedBlock::from_block(block).expect("trim block");
    let trimmed_bytes = trimmed.to_array().expect("serialize trimmed block");
    let mut block_key_bytes = Vec::with_capacity(1 + 32);
    block_key_bytes.push(PREFIX_BLOCK);
    block_key_bytes.extend_from_slice(&hash.to_bytes());
    let block_key = StorageKey::new(LedgerContract::ID, block_key_bytes);
    store.add(
        block_key,
        neo_storage::StorageItem::from_bytes(trimmed_bytes),
    );

    for tx in &block.transactions {
        // C# `TransactionState.ToStackItem`: the persisted record is the
        // interoperable `Struct[Integer(BlockIndex), ByteString(tx),
        // Integer((byte)State)]` serialized with `BinarySerializer` — NOT a
        // hand-rolled `kind/index/state/varbytes` layout. Use the canonical
        // writer so the fixture matches what the server's reader expects.
        let record = neo_native_contracts::ledger_contract::serialize_persisted_transaction_state(
            index,
            VMState::HALT,
            tx,
        )
        .expect("serialize transaction state");

        let mut tx_key_bytes = Vec::with_capacity(1 + 32);
        tx_key_bytes.push(PREFIX_TRANSACTION);
        tx_key_bytes.extend_from_slice(&tx.hash().to_bytes());
        let tx_key = StorageKey::new(LedgerContract::ID, tx_key_bytes);
        store.add(tx_key, neo_storage::StorageItem::from_bytes(record));
    }

    // C# `HashIndexState.ToStackItem`: the Prefix_CurrentBlock value is the
    // interoperable `Struct[ByteString(hash), Integer(index)]` serialized with
    // `BinarySerializer`, not a raw 32+4-byte concatenation.
    let current_bytes =
        neo_native_contracts::ledger_contract::serialize_hash_index_state(&hash, index)
            .expect("serialize hash index state");
    let current_key = StorageKey::new(LedgerContract::ID, vec![PREFIX_CURRENT_BLOCK]);
    store.add(
        current_key,
        neo_storage::StorageItem::from_bytes(current_bytes),
    );
    store.commit();
}

#[tokio::test(flavor = "multi_thread")]
async fn get_raw_transaction_verbose_omits_vmstate() {
    let system = std::sync::Arc::new(
        neo_system::Node::new(
            std::sync::Arc::new(neo_config::ProtocolSettings::default()),
            None,
            None,
        )
        .expect("system to start"),
    );
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getrawtransaction");

    let tx = make_transaction(7);
    let block = make_ledger_block(&system.store_cache(), 1, vec![tx.clone()]);
    let block_hash = block.hash();
    let mut store = system.store_cache();
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
    // C# GetRawTransaction verbose adds only blockhash, confirmations and blocktime
    // (RpcServer.Blockchain.cs:373-381) — NOT vmstate, which belongs to
    // getapplicationlog. Guard against re-introducing the non-C# field.
    assert!(
        obj.get("vmstate").is_none(),
        "getrawtransaction verbose must not include a vmstate field (C# parity)"
    );
    assert!(obj.get("confirmations").is_some());
    assert!(obj.get("blocktime").is_some());

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
