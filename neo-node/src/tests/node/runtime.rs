use super::*;

/// `commit_to_store` flushes the writes accumulated in the shared snapshot
/// (as a block's native-persist pipeline does) through to the durable store,
/// so a fresh cache over the same store reads them. Without this, synced
/// blocks stay in-memory and the on-disk tip is stuck at genesis.
#[test]
fn commit_to_store_flushes_snapshot_writes_to_durable_store() {
    use neo_blockchain::service_context::SystemContext;
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_storage::persistence::{StoreCache, store::Store};
    use neo_storage::{StorageItem, StorageKey};

    let store: Arc<dyn Store> = Arc::new(MemoryStore::new());
    let store_cache = StoreCache::new_from_store(Arc::clone(&store), false);
    let snapshot = Arc::new(store_cache.data_cache().clone());
    let ctx = DaemonContext::new(
        Arc::new(ProtocolSettings::default()),
        Arc::clone(&snapshot),
        store_cache,
        None,
        None,
        None,
    );

    // Stage a write into the shared snapshot (the blockchain persist path).
    let key = StorageKey::new(-1, vec![0xAB, 0xCD]);
    snapshot.add(key.clone(), StorageItem::from_bytes(vec![0x01, 0x02, 0x03]));

    // Not durable yet: a fresh cache over the same store cannot see it.
    let before = StoreCache::new_from_store(Arc::clone(&store), false);
    assert!(
        before.data_cache().get(&key).is_none(),
        "write must not reach the store before commit_to_store"
    );

    // Flush, then a fresh cache over the same store reads the write.
    ctx.commit_to_store();
    let after = StoreCache::new_from_store(Arc::clone(&store), false);
    assert!(
        after.data_cache().get(&key).is_some(),
        "commit_to_store must flush the snapshot write through to the store"
    );
}

#[test]
fn daemon_context_indexes_application_executed_notifications() {
    use neo_blockchain::service_context::SystemContext;
    use neo_payloads::{ApplicationExecuted, Block, Header, NotifyEventArgs, Signer, Transaction};
    use neo_primitives::{TriggerType, UInt160, WitnessScope};
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_storage::persistence::{StoreCache, store::Store};
    use neo_vm_rs::VmState as VMState;

    let store: Arc<dyn Store> = Arc::new(MemoryStore::new());
    let store_cache = StoreCache::new_from_store(Arc::clone(&store), false);
    let snapshot = Arc::new(store_cache.data_cache().clone());
    let indexer = Arc::new(neo_indexer::IndexerService::new());
    let ctx = DaemonContext::new(
        Arc::new(ProtocolSettings::default()),
        Arc::clone(&snapshot),
        store_cache,
        None,
        Some(Arc::clone(&indexer)),
        None,
    );

    let signer = UInt160::from_bytes(&[1; UInt160::LENGTH]).expect("signer");
    let contract = UInt160::from_bytes(&[2; UInt160::LENGTH]).expect("contract");
    let mut tx = Transaction::new();
    tx.set_nonce(91);
    tx.set_script(vec![0x51]);
    tx.set_signers(vec![Signer::new(signer, WitnessScope::CALLED_BY_ENTRY)]);
    let tx_hash = tx.try_hash().expect("tx hash");

    let mut header = Header::new();
    header.set_index(5);
    let mut block = Block::from_parts(header, vec![tx.clone()]);
    block.try_rebuild_merkle_root().expect("merkle root");

    let mut executed = ApplicationExecuted::new(
        Some(tx),
        TriggerType::APPLICATION,
        VMState::HALT,
        None,
        0,
        Vec::new(),
    );
    executed
        .notifications
        .push(NotifyEventArgs::new_with_optional_container(
            None,
            contract,
            "Transfer".to_string(),
            Vec::new(),
        ));

    assert!(ctx.block_committing(&block, &snapshot, &[executed]));

    let records = indexer.notifications_for_transaction(&tx_hash, 0, 10);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].contract_hash, contract);
    assert_eq!(records[0].event_name, "Transfer");
    assert_eq!(records[0].block_height, 5);
}

#[test]
fn daemon_context_dispatches_application_logs_handlers() {
    use neo_blockchain::service_context::SystemContext;
    use neo_payloads::{ApplicationExecuted, Block, Header, NotifyEventArgs, Signer, Transaction};
    use neo_primitives::{TriggerType, UInt160, WitnessScope};
    use neo_rpc::application_logs::{ApplicationLogsService, ApplicationLogsSettings};
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_storage::persistence::{StoreCache, store::Store};
    use neo_vm_rs::VmState as VMState;

    let settings = Arc::new(ProtocolSettings::default());
    let chain_store: Arc<dyn Store> = Arc::new(MemoryStore::new());
    let store_cache = StoreCache::new_from_store(Arc::clone(&chain_store), false);
    let snapshot = Arc::new(store_cache.data_cache().clone());

    let mut logs_settings = ApplicationLogsSettings::default();
    logs_settings.enabled = true;
    logs_settings.network = settings.network;
    let logs_service = Arc::new(ApplicationLogsService::new(
        logs_settings,
        Arc::new(MemoryStore::new()),
    ));

    let ctx = DaemonContext::new(
        Arc::clone(&settings),
        Arc::clone(&snapshot),
        store_cache,
        None,
        None,
        Some(Arc::clone(&logs_service)),
    );
    let node = Arc::new(neo_system::Node::new(settings, None, None).expect("node"));
    ctx.set_node(node);

    let signer = UInt160::from_bytes(&[5; UInt160::LENGTH]).expect("signer");
    let contract = UInt160::from_bytes(&[6; UInt160::LENGTH]).expect("contract");
    let mut tx = Transaction::new();
    tx.set_nonce(117);
    tx.set_script(vec![0x51]);
    tx.set_signers(vec![Signer::new(signer, WitnessScope::CALLED_BY_ENTRY)]);
    let tx_hash = tx.try_hash().expect("tx hash");

    let mut header = Header::new();
    header.set_index(7);
    let mut block = Block::from_parts(header, vec![tx.clone()]);
    block.try_rebuild_merkle_root().expect("merkle root");

    let mut executed = ApplicationExecuted::new(
        Some(tx),
        TriggerType::APPLICATION,
        VMState::HALT,
        None,
        10,
        Vec::new(),
    );
    executed
        .notifications
        .push(NotifyEventArgs::new_with_optional_container(
            None,
            contract,
            "Transfer".to_string(),
            Vec::new(),
        ));

    assert!(ctx.block_committing(&block, &snapshot, &[executed]));
    ctx.block_committed(&block);

    let tx_log = logs_service
        .get_transaction_log(&tx_hash)
        .expect("transaction application log");
    assert_eq!(tx_log["txid"], tx_hash.to_string());
    assert_eq!(tx_log["executions"][0]["trigger"], "Application");
    assert_eq!(
        tx_log["executions"][0]["notifications"][0]["eventname"],
        "Transfer"
    );
}

/// Full daemon restart smoke test: when the durable RocksDB store already
/// contains a ledger tip, `build_node` must read it before P2P starts,
/// advertise it in `version`, and request blocks from `tip + 1`.
#[tokio::test]
async fn build_node_restarts_from_durable_rocksdb_tip_and_resumes_sync_cursor() {
    const DURABLE_TIP: u32 = 1;
    const PEER_HEIGHT: u32 = 3;

    let temp = tempfile::tempdir().expect("temp RocksDB root");
    let storage_path = temp.path().join("chain");
    let settings = Arc::new(ProtocolSettings::default());
    seed_rocksdb_tip(&storage_path, settings.as_ref(), DURABLE_TIP)
        .expect("seed durable RocksDB tip");

    let config = NodeConfig::default();
    let running = build_node(Arc::clone(&settings), &config, Some(&storage_path), None)
        .await
        .expect("build node over durable store");

    running
        .network
        .start("127.0.0.1:0".parse().unwrap())
        .await
        .expect("start P2P listener");
    let port = running.network.local_node_info().port();
    assert_ne!(port, 0);

    let mut fake = fake_dial(port).await;
    let node_version = recv_frame(&mut fake).await;
    assert_eq!(node_version.command, MessageCommand::Version);
    let node_version: VersionPayload = decode_payload(&node_version);
    assert!(
        node_version.capabilities.iter().any(|capability| matches!(
            capability,
            NodeCapability::FullNode {
                start_height: DURABLE_TIP
            }
        )),
        "restarted daemon must advertise the durable ledger tip"
    );

    fake.send(fake_peer_version_message(
        settings.network,
        0xfa4e_00d0,
        PEER_HEIGHT,
    ))
    .await
    .expect("send peer version");
    let verack = recv_frame(&mut fake).await;
    assert_eq!(verack.command, MessageCommand::Verack);
    fake.send(verack_message()).await.expect("send verack");

    let request = recv_getblockbyindex(&mut fake).await;
    assert_eq!(
        request.index_start,
        DURABLE_TIP + 1,
        "restart sync cursor resumes just after the durable tip"
    );
    assert_eq!(request.count, (PEER_HEIGHT - DURABLE_TIP) as i16);

    running.network.shutdown().await.expect("shutdown network");
    for handle in running.handles {
        handle.abort();
        let _ = handle.await;
    }
    drop(running.node);
    drop(running.network);
}

/// Operator-facing RPC smoke test: a daemon rebuilt over a durable RocksDB
/// ledger must expose the recovered chain height through JSON-RPC.
///
/// Runs on a multi-thread runtime to match the production daemon
/// (`#[tokio::main]`): the JSON-RPC relay path (`sendrawtransaction` /
/// `submitblock`) uses `block_in_place`, which requires a multi-thread
/// runtime. `getblockcount` itself does not, but the multi-thread flavor
/// keeps this end-to-end smoke test representative of the real daemon.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rpc_getblockcount_reads_restarted_durable_rocksdb_tip() {
    const DURABLE_TIP: u32 = 1;

    let temp = tempfile::tempdir().expect("temp RocksDB root");
    let storage_path = temp.path().join("chain");
    let settings = Arc::new(ProtocolSettings::default());
    seed_rocksdb_tip(&storage_path, settings.as_ref(), DURABLE_TIP)
        .expect("seed durable RocksDB tip");

    let rpc_port = unused_local_rpc_port();
    let mut config = NodeConfig::default();
    config.rpc.enabled = true;
    config.rpc.port = Some(rpc_port);
    config.rpc.bind_address = Some("127.0.0.1".to_string());

    let running = build_node(Arc::clone(&settings), &config, Some(&storage_path), None)
        .await
        .expect("build node over durable store");
    let server =
        start_rpc_server(&running.node, &config, settings.network).expect("start JSON-RPC server");
    assert!(server.read().is_started(), "JSON-RPC server must bind");

    let response = rpc_post_json(
        rpc_port,
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": "getblockcount",
            "params": [],
            "id": 1
        }),
    )
    .await;
    assert_eq!(response.get("error"), None);
    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 1);
    assert_eq!(response["result"], serde_json::json!(DURABLE_TIP + 1));

    server.write().stop_rpc_server();
    drop(server);
    for handle in running.handles {
        handle.abort();
        let _ = handle.await;
    }
    drop(running.node);
    drop(running.network);
}
