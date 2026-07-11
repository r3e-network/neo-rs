//! Hot-row pruning regressions for configured static Ledger reads.

use super::*;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_blockchain::{BlockProvider, OptionalStaticLedgerProvider, StaticLedgerArchiveFactory};
use neo_io::{MemoryReader, Serializable};
use neo_native_contracts::ledger_contract::storage::{
    PREFIX_BLOCK, PREFIX_BLOCK_HASH, PREFIX_TRANSACTION,
};

static ARCHIVE_FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

struct ArchiveFixtureDir(PathBuf);

impl ArchiveFixtureDir {
    fn new() -> Self {
        let id = ARCHIVE_FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "neo-rpc-ledger-archive-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create archive fixture directory");
        Self(path)
    }
}

impl Drop for ArchiveFixtureDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct ArchivedBlockFixture {
    system: Arc<crate::server::NodeContext>,
    block: neo_payloads::Block,
    _directory: ArchiveFixtureDir,
}

fn archived_block_fixture(nonce: u32) -> ArchivedBlockFixture {
    let hot_system = crate::server::test_support::test_system(ProtocolSettings::default());
    let block = make_ledger_block(&hot_system.store_cache(), 1, vec![make_transaction(nonce)]);
    let mut store = hot_system.store_cache();
    store_block(&mut store, &block);

    let genesis = hot_system
        .ledger_provider(store.data_cache())
        .block_by_index(0)
        .expect("load genesis")
        .expect("genesis block");
    let directory = ArchiveFixtureDir::new();
    let archive = StaticLedgerArchiveFactory::default()
        .open(directory.0.join("ledger.static"))
        .expect("open Ledger archive");
    archive
        .append_block(store.data_cache(), &genesis)
        .expect("archive genesis");
    archive
        .append_block(store.data_cache(), &block)
        .expect("archive block");

    store.delete(StorageKey::create_with_uint32(
        LedgerContract::ID,
        PREFIX_BLOCK_HASH,
        block.index(),
    ));
    store.delete(StorageKey::create_with_uint256(
        LedgerContract::ID,
        PREFIX_BLOCK,
        &block.hash(),
    ));
    for transaction in &block.transactions {
        store.delete(StorageKey::create_with_uint256(
            LedgerContract::ID,
            PREFIX_TRANSACTION,
            &transaction.hash(),
        ));
    }
    store.commit();

    let cold = OptionalStaticLedgerProvider::from_option(Some(archive.provider()));
    drop(archive);
    let system =
        crate::server::test_support::test_system_with_cold_ledger_provider(&hot_system, cold);
    ArchivedBlockFixture {
        system,
        block,
        _directory: directory,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn get_block_hash_reads_configured_archive_after_hot_row_pruning() {
    let fixture = archived_block_fixture(8_101);
    let server = RpcServer::new(fixture.system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getblockhash");

    let result = (handler.callback())(&server, &[Value::Number(1u32.into())])
        .expect("get archived block hash");
    let expected = fixture.block.hash().to_string();
    assert_eq!(result.as_str(), Some(expected.as_str()));
}

#[tokio::test(flavor = "multi_thread")]
async fn get_block_reads_configured_archive_after_hot_row_pruning() {
    let fixture = archived_block_fixture(8_102);
    let server = RpcServer::new(fixture.system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getblock");

    let result =
        (handler.callback())(&server, &[Value::Number(1u32.into())]).expect("get archived block");
    let bytes = BASE64_STANDARD
        .decode(result.as_str().expect("base64"))
        .expect("decode archived block");
    let mut reader = MemoryReader::new(&bytes);
    let decoded =
        <neo_payloads::Block as Serializable>::deserialize(&mut reader).expect("decode block");
    assert_eq!(decoded.hash(), fixture.block.hash());
    assert_eq!(
        decoded.transactions[0].hash(),
        fixture.block.transactions[0].hash()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn verbose_block_and_header_keep_context_when_body_is_archive_only() {
    let fixture = archived_block_fixture(8_103);
    let successor = make_ledger_block(&fixture.system.store_cache(), 2, Vec::new());
    let mut store = fixture.system.store_cache();
    store_block(&mut store, &successor);
    let server = RpcServer::new(fixture.system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let successor_hash = successor.hash().to_string();

    for method in ["getblock", "getblockheader"] {
        let handler = find_handler(&handlers, method);
        let result =
            (handler.callback())(&server, &[Value::Number(1u32.into()), Value::Bool(true)])
                .unwrap_or_else(|error| panic!("{method} archived verbose read: {error}"));
        let object = result.as_object().expect("verbose object");
        assert_eq!(object.get("confirmations").and_then(Value::as_u64), Some(2));
        assert_eq!(
            object.get("nextblockhash").and_then(Value::as_str),
            Some(successor_hash.as_str())
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn get_raw_transaction_reads_configured_archive_after_hot_row_pruning() {
    let fixture = archived_block_fixture(8_201);
    let transaction = fixture.block.transactions[0].clone();
    let server = RpcServer::new(fixture.system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getrawtransaction");

    let params = [
        Value::String(transaction.hash().to_string()),
        Value::Bool(false),
    ];
    let result = (handler.callback())(&server, &params).expect("get archived transaction");
    let bytes = BASE64_STANDARD
        .decode(result.as_str().expect("base64"))
        .expect("decode archived transaction");
    let mut reader = MemoryReader::new(&bytes);
    let decoded = <Transaction as Serializable>::deserialize(&mut reader)
        .expect("deserialize archived transaction");
    assert_eq!(decoded.hash(), transaction.hash());
}

#[tokio::test(flavor = "multi_thread")]
async fn verbose_transaction_keeps_block_context_when_transaction_is_archive_only() {
    let fixture = archived_block_fixture(8_203);
    let successor = make_ledger_block(&fixture.system.store_cache(), 2, Vec::new());
    let mut store = fixture.system.store_cache();
    store_block(&mut store, &successor);
    let transaction = &fixture.block.transactions[0];
    let server = RpcServer::new(fixture.system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getrawtransaction");
    let block_hash = fixture.block.hash().to_string();

    let result = (handler.callback())(
        &server,
        &[
            Value::String(transaction.hash().to_string()),
            Value::Bool(true),
        ],
    )
    .expect("get verbose archived transaction");
    let object = result.as_object().expect("verbose transaction object");
    assert_eq!(
        object.get("blockhash").and_then(Value::as_str),
        Some(block_hash.as_str())
    );
    assert_eq!(object.get("confirmations").and_then(Value::as_u64), Some(2));
    assert_eq!(object.get("blocktime").and_then(Value::as_u64), Some(1));
}

#[tokio::test(flavor = "multi_thread")]
async fn get_transaction_height_reads_configured_archive_after_hot_row_pruning() {
    let fixture = archived_block_fixture(8_202);
    let transaction = &fixture.block.transactions[0];
    let server = RpcServer::new(fixture.system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "gettransactionheight");

    let result = (handler.callback())(&server, &[Value::String(transaction.hash().to_string())])
        .expect("get archived transaction height");
    assert_eq!(result.as_u64(), Some(1));
}
