#![cfg(feature = "runtime")]

use neo_core::i_event_handlers::i_committing_handler::ICommittingHandler;
use neo_core::ledger::block::Block as LedgerBlock;
use neo_core::ledger::blockchain_application_executed::ApplicationExecuted;
use neo_core::neo_system::NeoSystem;
use neo_core::network::p2p::payloads::block::Block as PayloadBlock;
use neo_core::network::p2p::payloads::header::Header;
use neo_core::network::p2p::payloads::witness::Witness;
use neo_core::persistence::DataCache;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::{UInt160, UInt256};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

struct FastSyncCaptureHandler {
    observed_len: Arc<AtomicUsize>,
}

impl ICommittingHandler for FastSyncCaptureHandler {
    fn run_during_fast_sync(&self) -> bool {
        true
    }

    fn blockchain_committing_handler(
        &self,
        _system: &dyn std::any::Any,
        _block: &LedgerBlock,
        _snapshot: &DataCache,
        application_executed_list: &[ApplicationExecuted],
    ) {
        self.observed_len
            .store(application_executed_list.len(), Ordering::Relaxed);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn fast_sync_opt_in_handler_receives_application_executed_data() {
    let system = NeoSystem::new(ProtocolSettings::mainnet(), None, None).expect("system");
    system.context().enable_fast_sync_mode();

    let observed_len = Arc::new(AtomicUsize::new(usize::MAX));
    system
        .register_committing_handler(Arc::new(FastSyncCaptureHandler {
            observed_len: Arc::clone(&observed_len),
        }))
        .expect("register handler");

    let mut block = PayloadBlock::new();
    let mut header = Header::new();
    header.set_index(1);
    header.set_prev_hash(UInt256::zero());
    header.set_merkle_root(UInt256::zero());
    header.set_next_consensus(UInt160::zero());
    header.set_timestamp(1);
    header.witness = Witness::new();
    block.header = header;
    block.transactions = Vec::new();

    system.persist_block(block).expect("persist block");

    assert!(
        observed_len.load(Ordering::Relaxed) > 0,
        "fast-sync handlers that opt in should still receive application execution data"
    );
}
