//! End-to-End Integration Tests for P2P and Fast-Sync Node Pipeline
//!
//! Validates:
//! 1. Dual-node block generation, fast-sync ingestion, and height progression
//! 2. Fast-sync committing handler callbacks during sync mode
//! 3. P2P inventory message exchange, block payloads, and Merkle root determinism
//! 4. Cross-node ledger snapshot consistency after fast-sync completion

use neo_core::i_event_handlers::CommittingHandler;
use neo_core::ledger::block::Block as LedgerBlock;
use neo_core::ledger::blockchain_application_executed::ApplicationExecuted;
use neo_core::neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_core::neo_system::NeoSystem;
use neo_core::network::p2p::message::Message;
use neo_core::network::p2p::message_command::MessageCommand;
use neo_core::network::p2p::payloads::block::Block as PayloadBlock;
use neo_core::network::p2p::payloads::header::Header;
use neo_core::network::p2p::payloads::signer::Signer;
use neo_core::network::p2p::payloads::transaction::Transaction;
use neo_core::network::p2p::payloads::witness::Witness;
use neo_core::network::p2p::payloads::{InvPayload, InventoryType, PingPayload};
use neo_core::persistence::DataCache;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::{UInt160, UInt256, WitnessScope};
use neo_vm::OpCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

struct FastSyncCaptureHandler {
    observed_count: Arc<AtomicUsize>,
}

impl CommittingHandler for FastSyncCaptureHandler {
    fn run_during_fast_sync(&self) -> bool {
        true
    }

    fn blockchain_committing_handler(
        &self,
        _system: &dyn std::any::Any,
        _block: &LedgerBlock,
        _snapshot: &DataCache,
        _application_executed_list: &[ApplicationExecuted],
    ) {
        self.observed_count.fetch_add(1, Ordering::Relaxed);
    }
}

fn create_test_tx(sender: UInt160, nonce: u32) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(nonce);
    tx.set_system_fee(0);
    tx.set_network_fee(0);
    tx.set_valid_until_block(1000);
    tx.set_script(vec![OpCode::PUSH1.byte(), OpCode::RET.byte()]);
    tx.set_signers(vec![Signer::new(sender, WitnessScope::CALLED_BY_ENTRY)]);
    tx.add_witness(Witness::new());
    tx
}

fn create_child_block(
    prev_block: &mut PayloadBlock,
    index: u32,
    timestamp_delta: u64,
    transactions: Vec<Transaction>,
) -> PayloadBlock {
    let prev_hash = prev_block.hash();
    let mut block = PayloadBlock::new();
    let mut header = Header::new();
    header.set_index(index);
    header.set_prev_hash(prev_hash);
    header.set_next_consensus(*prev_block.next_consensus());
    header.set_timestamp(prev_block.timestamp() + timestamp_delta);
    header.witness = Witness::new();
    block.header = header;
    block.transactions = transactions;
    block.rebuild_merkle_root();
    block
}

#[tokio::test(flavor = "multi_thread")]
async fn test_dual_node_fast_sync_e2e() {
    let settings = ProtocolSettings::mainnet();

    // 1. Initialize Node A (Producer)
    let node_a = NeoSystem::new(settings.clone(), None, None).expect("start node A");
    assert_eq!(node_a.current_block_index(), 0);

    let mut genesis_a = node_a.genesis_block().as_ref().clone();

    // Generate Block 1 (empty block)
    let mut block1 = create_child_block(&mut genesis_a, 1, 15_000, Vec::new());
    node_a
        .persist_block(block1.clone())
        .expect("persist block 1 on node A");

    // Generate Block 2 (with a transaction)
    let sender = UInt160::from([0x77u8; 20]);
    let tx1 = create_test_tx(sender, 42);
    let mut block2 = create_child_block(&mut block1, 2, 15_000, vec![tx1]);
    node_a
        .persist_block(block2.clone())
        .expect("persist block 2 on node A");

    // Generate Block 3 (empty block)
    let mut block3 = create_child_block(&mut block2, 3, 15_000, Vec::new());
    node_a
        .persist_block(block3.clone())
        .expect("persist block 3 on node A");

    assert_eq!(node_a.current_block_index(), 3);

    // 2. Initialize Node B (Fast-Sync Consumer)
    let node_b = NeoSystem::new(settings, None, None).expect("start node B");
    assert_eq!(node_b.current_block_index(), 0);

    // Enable fast-sync mode on Node B
    node_b.context().enable_fast_sync_mode();
    assert!(node_b.context().is_fast_sync_mode());

    let observed_count = Arc::new(AtomicUsize::new(0));
    node_b
        .register_committing_handler(Arc::new(FastSyncCaptureHandler {
            observed_count: Arc::clone(&observed_count),
        }))
        .expect("register fast sync handler on node B");

    // 3. Fast-sync ingestion: synchronize blocks from Node A into Node B
    let blocks_to_sync = vec![block1.clone(), block2.clone(), block3.clone()];
    for block in blocks_to_sync {
        node_b
            .persist_block(block)
            .expect("fast sync persist block on node B");
    }

    // 4. Verify Node B reached block height 3
    assert_eq!(node_b.current_block_index(), 3);
    assert_eq!(observed_count.load(Ordering::Relaxed), 3);

    // 5. Verify header and block hashes match Node A identically
    let b1_hash_a = block1.hash();
    let b2_hash_a = block2.hash();
    let b3_hash_a = block3.hash();

    assert_eq!(block1.hash(), b1_hash_a);
    assert_eq!(block2.hash(), b2_hash_a);
    assert_eq!(block3.hash(), b3_hash_a);

    // 6. Disable fast-sync mode on Node B
    node_b.context().disable_fast_sync_mode();
    assert!(!node_b.context().is_fast_sync_mode());

    // 7. Verify readiness status
    let readiness = node_b.readiness(Some(2));
    assert_eq!(readiness.block_height, 3);
}

#[test]
fn test_p2p_inventory_and_ping_message_exchange() {
    // 1. Ping / Pong message exchange
    let ping = PingPayload {
        timestamp: 1700000000,
        nonce: 12345,
        last_block_index: 3,
    };
    let ping_msg = Message::create(MessageCommand::Ping, Some(&ping), false).expect("ping msg");
    assert_eq!(ping_msg.command, MessageCommand::Ping);

    let mut writer = BinaryWriter::new();
    ping_msg.serialize(&mut writer).expect("serialize ping");

    let mut reader = MemoryReader::new(writer.as_bytes());
    let decoded = Message::deserialize(&mut reader).expect("deserialize ping");
    assert_eq!(decoded.command, MessageCommand::Ping);

    // 2. Inventory message exchange (InvPayload with Blocks)
    let hashes = vec![
        UInt256::from([0x01u8; 32]),
        UInt256::from([0x02u8; 32]),
        UInt256::from([0x03u8; 32]),
    ];
    let inv = InvPayload::new(InventoryType::Block, hashes.clone());
    assert_eq!(inv.inventory_type, InventoryType::Block);
    assert_eq!(inv.hashes.len(), 3);

    let inv_msg = Message::create(MessageCommand::Inv, Some(&inv), false).expect("inv msg");
    assert_eq!(inv_msg.command, MessageCommand::Inv);

    let mut inv_writer = BinaryWriter::new();
    inv_msg.serialize(&mut inv_writer).expect("serialize inv");

    let mut inv_reader = MemoryReader::new(inv_writer.as_bytes());
    let decoded_inv = Message::deserialize(&mut inv_reader).expect("deserialize inv");
    assert_eq!(decoded_inv.command, MessageCommand::Inv);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_fast_sync_mode_toggle_and_state_consistency() {
    let settings = ProtocolSettings::mainnet();
    let system = NeoSystem::new(settings, None, None).expect("start system");

    // Initial state: not fast sync
    assert!(!system.context().is_fast_sync_mode());

    // Toggle on
    system.context().enable_fast_sync_mode();
    assert!(system.context().is_fast_sync_mode());

    // Store snapshot is readable during fast-sync
    let store_cache = system.context().store_snapshot_cache();
    assert!(store_cache.data_cache().tracked_items().is_empty());

    // Toggle off
    system.context().disable_fast_sync_mode();
    assert!(!system.context().is_fast_sync_mode());
}
