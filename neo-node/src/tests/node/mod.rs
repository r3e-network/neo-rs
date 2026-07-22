//! # neo-node::tests::node
//!
//! Test module grouping Daemon composition, CLI modes, and long-running node
//! startup. coverage for neo-node.
//!
//! ## Boundary
//!
//! This is test/benchmark-only code for neo-node; it may assemble fixtures but
//! must not introduce production behavior.
//!
//! ## Contents
//!
//! - `config_parsing`: node config parsing coverage.
//! - `config_validation`: node config validation coverage.
//! - `recovery`: local replay poison-marker and fail-stop coverage.
//! - `runtime`: Runtime flags, execution context state, and VM-facing support
//!   types.

use super::*;
use futures::{SinkExt, StreamExt};
use neo_network::{Message, MessageCodec, MessageCommand};
use neo_payloads::p2p_payloads::{GetBlockByIndexPayload, NodeCapability, VersionPayload};
use serde_json::Value;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_util::codec::Framed;

const TEST_TIMEOUT: Duration = Duration::from_secs(10);

type FakeFramed = Framed<TcpStream, MessageCodec>;

async fn fake_dial(port: u16) -> FakeFramed {
    let stream = tokio::time::timeout(TEST_TIMEOUT, TcpStream::connect(("127.0.0.1", port)))
        .await
        .expect("dial timed out")
        .expect("dial failed");
    Framed::new(stream, MessageCodec::new())
}

async fn recv_frame(framed: &mut FakeFramed) -> Message {
    match tokio::time::timeout(TEST_TIMEOUT, framed.next()).await {
        Ok(Some(Ok(message))) => message,
        Ok(Some(Err(err))) => panic!("frame decode failed: {err}"),
        Ok(None) => panic!("connection closed while waiting for frame"),
        Err(_) => panic!("timed out waiting for frame"),
    }
}

fn decode_payload<T: neo_io::Serializable>(message: &Message) -> T {
    let mut reader = neo_io::MemoryReader::new(&message.payload_raw);
    <T as neo_io::Serializable>::deserialize(&mut reader).expect("decode payload")
}

fn fake_peer_version_message(network: u32, nonce: u32, height: u32) -> Message {
    let payload = VersionPayload::create(
        network,
        nonce,
        "/fake-peer:0.0.1/".to_string(),
        height,
        vec![
            NodeCapability::full_node(height),
            NodeCapability::tcp_server(20333),
        ],
    );
    Message::create(MessageCommand::Version, Some(&payload), false).expect("encode version")
}

fn verack_message() -> Message {
    Message::from_payload_bytes(MessageCommand::Verack, Vec::new(), false).expect("encode verack")
}

async fn recv_getblockbyindex(fake: &mut FakeFramed) -> GetBlockByIndexPayload {
    loop {
        let frame = recv_frame(fake).await;
        if frame.command == MessageCommand::GetBlockByIndex {
            return decode_payload(&frame);
        }
    }
}

async fn recv_getheaders(fake: &mut FakeFramed) -> GetBlockByIndexPayload {
    loop {
        let frame = recv_frame(fake).await;
        if frame.command == MessageCommand::GetHeaders {
            return decode_payload(&frame);
        }
    }
}

fn empty_child_block(parent: &neo_payloads::Block, index: u32) -> neo_payloads::Block {
    let mut header = neo_payloads::Header::new();
    header.set_index(index);
    header.set_prev_hash(parent.hash());
    header.set_timestamp(parent.header.timestamp() + 15_000);
    header.set_next_consensus(*parent.header.next_consensus());
    header.witness =
        neo_payloads::Witness::new_with_scripts(Vec::new(), vec![neo_vm::OpCode::PUSH1.byte()]);
    neo_payloads::Block::from_parts(header, Vec::new())
}

fn signed_empty_child_block(
    parent: &neo_payloads::Block,
    index: u32,
    network: u32,
    private_key: &[u8; 32],
    public_key: &neo_crypto::ECPoint,
) -> neo_payloads::Block {
    let mut block = empty_child_block(parent, index);
    let mut sign_data = Vec::with_capacity(36);
    sign_data.extend_from_slice(&network.to_le_bytes());
    sign_data.extend_from_slice(&block.hash().to_bytes());
    let signature =
        neo_crypto::Secp256r1Crypto::sign(&sign_data, private_key).expect("sign header");
    let verification =
        neo_vm::script_builder::redeem_script::RedeemScript::multi_sig_redeem_script_from_points(
            1,
            std::slice::from_ref(public_key),
        )
        .expect("single-validator redeem script");
    let mut invocation = vec![0x0C, 64];
    invocation.extend_from_slice(&signature);
    block.header.witness = neo_payloads::Witness::new_with_scripts(invocation, verification);
    block
}

fn seed_store_tip(
    backend: &str,
    path: &Path,
    chain_spec: &neo_config::NeoChainSpec,
    tip: u32,
) -> anyhow::Result<()> {
    use neo_storage::persistence::StoreCache;

    let settings = chain_spec.protocol_settings();
    let mut config = NodeConfig::default();
    config.storage.backend = Some(backend.to_string());
    let store = open_store(&config, Some(path))?;
    let mut store_cache = StoreCache::new_from_store(Arc::clone(&store), false);
    let snapshot = Arc::new(store_cache.data_cache().clone());
    let resources = neo_blockchain::NativePersistResources::from_provider(Arc::new(
        neo_native_contracts::StandardNativeProvider::new(),
    ));

    let mut parent = Arc::new(neo_blockchain::genesis_block(chain_spec)?);
    neo_blockchain::persist_block_natives_with_resources(
        Arc::clone(&snapshot),
        Arc::clone(&parent),
        Arc::new(settings.clone()),
        neo_blockchain::NativePersistOptions::default(),
        &resources,
    )?;

    for index in 1..=tip {
        let block = Arc::new(empty_child_block(parent.as_ref(), index));
        neo_blockchain::persist_block_natives_with_resources(
            Arc::clone(&snapshot),
            Arc::clone(&block),
            Arc::new(settings.clone()),
            neo_blockchain::NativePersistOptions::default(),
            &resources,
        )?;
        parent = block;
    }

    let current_index = neo_native_contracts::LedgerContract::new()
        .current_index(&snapshot)
        .expect("seeded ledger current index");
    assert_eq!(current_index, tip);
    store_cache
        .try_commit()
        .map_err(|err| anyhow::anyhow!("commit seeded {backend} store: {err}"))?;

    Ok(())
}

fn seed_mdbx_tip(
    path: &Path,
    chain_spec: &neo_config::NeoChainSpec,
    tip: u32,
) -> anyhow::Result<()> {
    seed_store_tip("mdbx", path, chain_spec, tip)
}

fn unused_local_rpc_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral RPC port");
    listener.local_addr().expect("local RPC address").port()
}

fn serve_rpc_once(expected_method: &'static str, result: Value) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test RPC");
    let url = format!("http://{}", listener.local_addr().expect("addr"));
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut request = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let read = stream.read(&mut buf).expect("read request");
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buf[..read]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        let text = String::from_utf8_lossy(&request);
        assert!(
            text.contains(&format!(r#""method":"{expected_method}""#))
                || text.contains(&format!(r#""method": "{expected_method}""#)),
            "unexpected request: {text}"
        );
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": result,
        })
        .to_string();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream
            .write_all(response.as_bytes())
            .expect("write response");
    });
    url
}

#[test]
fn inventory_relay_channel_absorbs_fast_sync_bursts() {
    const TARGET_BPS_WINDOW: usize = 1000;
    const MIN_PEER_WINDOWS: usize = 4;

    assert!(
        FAST_SYNC_BURST_CAPACITY >= TARGET_BPS_WINDOW * MIN_PEER_WINDOWS,
        "inventory relay queue must absorb several 1000-block sync windows"
    );
}

#[test]
fn import_tip_reaches_stop_height_after_chain_acc_import() {
    assert!(import_tip_reaches_stop_height(99_999, Some(99_999)));
    assert!(import_tip_reaches_stop_height(100_000, Some(99_999)));
    assert!(!import_tip_reaches_stop_height(99_998, Some(99_999)));
    assert!(!import_tip_reaches_stop_height(99_999, None));
}

#[test]
fn remote_ledger_mode_rejects_local_import_sources() {
    let cli = NodeCli {
        config: PathBuf::from("neo_testnet_node.toml"),
        network_magic: None,
        storage_path: None,
        check_config: false,
        check_storage: false,
        check_all: false,
        import_chain: Some(PathBuf::from("chain.acc")),
        verify_import_chain: false,
        fast_sync: true,
        fast_sync_cache: None,
        fast_sync_reference_rpc: None,
        fast_sync_report: None,
        fast_sync_expected_sha256: None,
        stop_at_height: None,
        enable_stateroot: false,
        stateroot: None,
        remote_ledger_rpc: Some("https://rpc.example.invalid".to_string()),
    };

    let err = validate_cli_mode(&cli).expect_err("remote ledger mode rejects local import");
    assert!(
        err.to_string().contains("--remote-ledger-rpc"),
        "operator error should identify the conflicting remote-ledger flag: {err}"
    );
    assert!(
        err.to_string().contains("--import-chain"),
        "operator error should identify chain.acc as a local-ledger path: {err}"
    );
    assert!(
        err.to_string().contains("--fast-sync"),
        "operator error should identify fast sync as a local-ledger path: {err}"
    );
}

#[test]
fn remote_ledger_mode_rejects_local_state_root_service() {
    let cli = NodeCli::try_parse_from([
        "neo-node",
        "--remote-ledger-rpc",
        "https://rpc.example.invalid",
        "--enable-stateroot",
    ])
    .expect("parse remote StateRoot request");
    let error = validate_cli_mode(&cli).expect_err("remote mode cannot run local StateRoot");
    assert!(
        error.to_string().contains("requires a local ledger"),
        "{error}"
    );
}

#[test]
fn fast_sync_reference_rpc_is_allowed_without_remote_ledger_mode() {
    let cli = NodeCli {
        config: PathBuf::from("neo_testnet_node.toml"),
        network_magic: None,
        storage_path: None,
        check_config: false,
        check_storage: false,
        check_all: false,
        import_chain: None,
        verify_import_chain: false,
        fast_sync: true,
        fast_sync_cache: None,
        fast_sync_reference_rpc: Some("https://rpc.example.invalid".to_string()),
        fast_sync_report: Some(PathBuf::from("fast-sync-report.json")),
        fast_sync_expected_sha256: None,
        stop_at_height: None,
        enable_stateroot: false,
        stateroot: None,
        remote_ledger_rpc: None,
    };

    validate_cli_mode(&cli)
        .expect("--fast-sync-reference-rpc should validate a local fast-sync import without enabling remote-ledger mode");
}

#[test]
fn remote_ledger_preflight_skips_local_storage_validation() {
    let cli = NodeCli {
        config: PathBuf::from("neo_mainnet_node.toml"),
        network_magic: None,
        storage_path: Some(PathBuf::from("/tmp/neo-remote-ledger-unused")),
        check_config: false,
        check_storage: false,
        check_all: true,
        import_chain: None,
        verify_import_chain: false,
        fast_sync: false,
        fast_sync_cache: None,
        fast_sync_reference_rpc: None,
        fast_sync_report: None,
        fast_sync_expected_sha256: None,
        stop_at_height: None,
        enable_stateroot: false,
        stateroot: None,
        remote_ledger_rpc: Some("https://rpc.example.invalid".to_string()),
    };

    assert_eq!(
        storage_preflight_mode(&cli, LedgerMode::from_cli(&cli)),
        StoragePreflightMode::SkipRemoteLedger,
        "remote-ledger preflight must not open or create the configured local chain store"
    );
}

#[test]
fn local_preflight_still_validates_local_storage() {
    let cli = NodeCli {
        config: PathBuf::from("neo_mainnet_node.toml"),
        network_magic: None,
        storage_path: Some(PathBuf::from("/tmp/neo-local-ledger")),
        check_config: false,
        check_storage: true,
        check_all: false,
        import_chain: None,
        verify_import_chain: false,
        fast_sync: false,
        fast_sync_cache: None,
        fast_sync_reference_rpc: None,
        fast_sync_report: None,
        fast_sync_expected_sha256: None,
        stop_at_height: None,
        enable_stateroot: false,
        stateroot: None,
        remote_ledger_rpc: None,
    };

    assert_eq!(
        storage_preflight_mode(&cli, LedgerMode::from_cli(&cli)),
        StoragePreflightMode::ValidateLocal,
        "local-node storage preflight must keep validating the configured ledger store"
    );
}

#[test]
fn shutdown_flush_reports_failed_state_service_worker() {
    use neo_state_service::{StateStore, commit_handlers::StateServiceCommitHandlers};
    use neo_storage::{DataCache, StorageItem, StorageKey};

    let state_store = Arc::new(StateStore::with_mpt(true));
    let state_service = Arc::new(StateServiceCommitHandlers::new_async_with_capacity(
        Arc::clone(&state_store),
        1,
    ));
    let services = services::NodeServiceHandles::new(
        None,
        Some(Arc::clone(&state_service)),
        None,
        None,
        None,
        None,
    );

    let snapshot = DataCache::new(false);
    snapshot.add(
        StorageKey::new(5, vec![0xAA]),
        StorageItem::from_bytes(vec![0x01]),
    );
    assert!(
        state_service.on_committing_deferred(5, &snapshot),
        "enqueue should succeed before the worker observes the non-contiguous block"
    );

    let err = flush_state_service_for_shutdown(&services)
        .expect_err("shutdown must fail when StateService worker failed");
    assert!(
        err.to_string().contains("state service MPT worker failed"),
        "unexpected shutdown error: {err}"
    );
}

#[tokio::test]
async fn inventory_block_batch_flush_sends_one_batch_command_and_clears_buffer() {
    let (handle, mut cmd_rx, _event_tx) = neo_blockchain::BlockchainHandle::channel(4, 4);
    let import_queue = Arc::new(neo_runtime::BlockImportQueue::new(
        Arc::new(handle.clone()),
        2,
    ));
    let live_import = neo_system::LiveBlockImportPipeline::new(handle, import_queue);
    let block = Arc::new(neo_payloads::Block::new());
    let mut pending = vec![Arc::clone(&block)];

    flush_inventory_block_batch(&live_import, &mut pending).await;

    assert!(pending.is_empty(), "flushed blocks must leave the buffer");
    match cmd_rx.recv().await.expect("batch command") {
        neo_blockchain::BlockchainCommand::CheckedInventoryBlocks { checked, relay } => {
            let (blocks, rejected) = checked.into_parts();
            assert_eq!(blocks.len(), 1);
            assert!(Arc::ptr_eq(&blocks[0], &block));
            assert!(rejected.is_empty());
            assert!(relay);
        }
        other => panic!("expected CheckedInventoryBlocks command, got {other:?}"),
    }
}

async fn rpc_post_json(port: u16, request: serde_json::Value) -> serde_json::Value {
    let client = reqwest::Client::new();
    let response = tokio::time::timeout(
        TEST_TIMEOUT,
        client
            .post(format!("http://127.0.0.1:{port}/"))
            .json(&request)
            .send(),
    )
    .await
    .expect("RPC request timed out")
    .expect("RPC request failed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    response
        .json::<serde_json::Value>()
        .await
        .expect("parse RPC response JSON")
}

#[path = "application.rs"]
mod application;
#[path = "config_parsing.rs"]
mod config_parsing;
#[path = "config_validation.rs"]
mod config_validation;
#[path = "recovery.rs"]
mod recovery;
#[path = "runtime.rs"]
mod runtime;
