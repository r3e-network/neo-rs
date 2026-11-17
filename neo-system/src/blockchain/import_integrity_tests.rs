//
// import_integrity_tests.rs - Body-integrity enforcement on the Import path
//
// A trusted import (`verify == false`) skips header verification, matching C#
// `Blockchain.OnImport(blocks, verify: false)`. It must still reject a block
// whose body is not the one its header commits to. Every test here goes
// through the blockchain actor's `Import` message and asserts on the typed
// `ImportResult` reply, so a rejection is attributable to verification rather
// than to a later persistence failure.
//

use super::{BlockchainCommand, Import, ImportError, ImportResult};
use crate::NeoSystem;
use crate::network::p2p::payloads::{
    block::Block as PayloadBlock, header::Header, transaction::Transaction,
};
use async_trait::async_trait;
use neo_actors::{Actor, ActorContext, ActorResult, Props};
use neo_native_contracts::LedgerContract;
use neo_primitives::{UInt160, UInt256};
use neo_protocol::execution::Block;
use neo_protocol::protocol_settings::ProtocolSettings;
use neo_protocol::witness::Witness;
use std::any::Any;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{Duration, timeout};

struct ImportResultProbe {
    tx: mpsc::UnboundedSender<ImportResult>,
}

#[async_trait]
impl Actor for ImportResultProbe {
    async fn handle(
        &mut self,
        envelope: Box<dyn Any + Send>,
        _ctx: &mut ActorContext,
    ) -> ActorResult {
        if let Ok(result) = envelope.downcast::<ImportResult>() {
            let _ = self.tx.send(*result);
        }
        Ok(())
    }
}

fn new_system() -> Arc<NeoSystem> {
    NeoSystem::new(ProtocolSettings::default(), None, None).expect("neo system")
}

/// Sends `blocks` through the blockchain actor's Import message and returns the
/// typed reply.
async fn import(system: &NeoSystem, blocks: Vec<Block>, verify: bool) -> ImportResult {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let probe = system
        .actor_system()
        .actor_of(
            Props::new(move || ImportResultProbe { tx: tx.clone() }),
            "import_result_probe",
        )
        .expect("probe actor");

    let blocks: Vec<Arc<Block>> = blocks.into_iter().map(Arc::new).collect();
    let import = Import {
        block: Arc::clone(blocks.first().expect("at least one block")),
        blocks,
        verify,
    };
    system
        .blockchain_actor()
        .tell_from(BlockchainCommand::Import(import), Some(probe))
        .expect("send import");

    timeout(Duration::from_secs(30), rx.recv())
        .await
        .expect("import reply timeout")
        .expect("import reply")
}

/// Asserts that neither the in-memory ledger nor the durable store advanced.
fn assert_nothing_persisted(system: &NeoSystem) {
    assert_eq!(system.ledger_context().current_height(), 0);
    let stored_height = LedgerContract::new()
        .current_index(&system.store_cache())
        .expect("stored height");
    assert_eq!(stored_height, 0);
}

fn assert_rejected_before_execution(system: &NeoSystem, result: &ImportResult, index: u32) {
    // `VerificationFailure`, not `PersistenceFailure`: the block never reached
    // the persistence pipeline, so nothing in it was executed.
    assert_eq!(
        result.error,
        Some(ImportError::VerificationFailure { index })
    );
    assert!(result.block.is_none(), "no block may be reported committed");
    assert_nothing_persisted(system);
}

fn distinct_transaction(nonce: u32) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_nonce(nonce);
    tx
}

/// Builds an execution block at `index` whose body matches its header, but
/// whose header would fail full verification: the previous hash is zero (not
/// the parent's hash), the timestamp predates genesis, and the witness is
/// empty.
fn body_consistent_block(index: u32, transactions: Vec<Transaction>) -> Block {
    let mut header = Header::new();
    header.set_index(index);
    header.set_prev_hash(UInt256::zero());
    header.set_next_consensus(UInt160::zero());
    header.set_timestamp(u64::from(index));
    header.witness = Witness::new();

    let mut payload = PayloadBlock::new();
    payload.header = header;
    payload.transactions = transactions;
    payload.rebuild_merkle_root();
    Block::from(&payload)
}

#[tokio::test(flavor = "multi_thread")]
async fn trusted_import_rejects_body_not_matching_merkle_root() {
    let system = new_system();
    // Header commits to an empty body; a transaction is substituted in.
    let mut block = body_consistent_block(1, Vec::new());
    assert_eq!(block.header.merkle_root, UInt256::zero());
    block.transactions = vec![distinct_transaction(1)];

    let result = import(&system, vec![block], false).await;
    assert_rejected_before_execution(&system, &result, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn trusted_import_rejects_non_zero_merkle_root_for_empty_body() {
    let system = new_system();
    let mut block = body_consistent_block(1, vec![distinct_transaction(1)]);
    assert_ne!(block.header.merkle_root, UInt256::zero());
    block.transactions.clear();

    let result = import(&system, vec![block], false).await;
    assert_rejected_before_execution(&system, &result, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn trusted_import_rejects_duplicate_transactions() {
    let system = new_system();
    // The merkle root is computed over the duplicated list, so only the
    // duplicate rule can reject this block.
    let tx = distinct_transaction(1);
    let block = body_consistent_block(1, vec![tx.clone(), tx]);
    assert!(block.to_payload_block().verify_merkle_root());

    let result = import(&system, vec![block], false).await;
    assert_rejected_before_execution(&system, &result, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn trusted_import_rejects_unsupported_block_version() {
    let system = new_system();
    let mut block = body_consistent_block(1, Vec::new());
    block.header.version = 1;

    let result = import(&system, vec![block], false).await;
    assert_rejected_before_execution(&system, &result, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn trusted_import_rejects_header_without_witness() {
    let system = new_system();
    let mut block = body_consistent_block(1, Vec::new());
    block.header.witnesses.clear();

    let result = import(&system, vec![block], false).await;
    assert_rejected_before_execution(&system, &result, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn trusted_import_rejects_header_with_two_witnesses() {
    let system = new_system();
    let mut block = body_consistent_block(1, Vec::new());
    block.header.witnesses = vec![Witness::new(), Witness::new()];

    let result = import(&system, vec![block], false).await;
    assert_rejected_before_execution(&system, &result, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn trusted_import_still_skips_header_verification() {
    let block = body_consistent_block(1, Vec::new());

    // Control: with verification on, this header is rejected (bad linkage,
    // timestamp and witness), so the acceptance below proves the trusted path
    // skipped header verification rather than the header being valid.
    let verifying = new_system();
    let result = import(&verifying, vec![block.clone()], true).await;
    assert_rejected_before_execution(&verifying, &result, 1);

    let trusting = new_system();
    let result = import(&trusting, vec![block.clone()], false).await;
    assert!(result.is_success(), "unexpected error: {:?}", result.error);
    assert_eq!(
        result.committed_block().map(|committed| committed.hash()),
        Some(block.hash())
    );
    assert_eq!(trusting.ledger_context().current_height(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn trusted_import_reports_blocks_committed_before_integrity_failure() {
    let system = new_system();
    let first = body_consistent_block(1, Vec::new());
    let mut second = body_consistent_block(2, Vec::new());
    second.transactions = vec![distinct_transaction(1)];

    let result = import(&system, vec![first.clone(), second], false).await;

    assert_eq!(
        result.error,
        Some(ImportError::VerificationFailure { index: 2 })
    );
    assert_eq!(
        result.committed_block().map(|committed| committed.hash()),
        Some(first.hash())
    );
    assert_eq!(system.ledger_context().current_height(), 1);
}
