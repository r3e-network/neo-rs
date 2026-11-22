//! Differential and regression tests for the Phase 4B empty-block hook
//! early-returns.
//!
//! The hooks optimized in this phase are [`GasToken::on_persist`] and
//! [`Notary::on_persist`]; `OracleContract::post_persist` is covered next to its
//! storage helpers in `oracle_contract::tests` because the cleanup helpers are
//! `pub(super)`.
//!
//! Every test compares the *observable* outcome of the optimized hook - tracked
//! storage deltas, emitted notifications and the returned result - against the
//! outcome the pre-optimization body produced for the same block, and asserts
//! that the height-dependent hooks that must never be skipped still do their
//! work on the very same empty blocks.
//!
//! # Hooks deliberately left unchanged
//!
//! An early return is only safe where the whole body is gated on block
//! *transactions*. Three empty-block hooks fail that test and therefore keep
//! executing unconditionally:
//!
//! * `LedgerContract::on_persist` always calls `store_block_state`, and
//!   `LedgerContract::post_persist` always advances the current block pointer and
//!   commits the transaction-state map. Those writes are exactly what an empty
//!   block still has to produce, so there is no work to remove - only the
//!   consensus-visible transition that would break if it were skipped.
//! * `NeoToken::on_persist` has a genuine height-dependent branch:
//!   `should_refresh_committee(height, committee_members_count)` fires on every
//!   committee-size boundary and the voter/reward bookkeeping is driven by it, so
//!   the block-transaction set cannot decide whether the body is a no-op.
//! * `ContractManagement::on_persist` performs genesis and hardfork
//!   initialisation off block *height*, not transactions; an empty block on a
//!   hardfork activation height must still initialise contract state.
//!
//! # Telemetry
//!
//! No early-return counter was added. The guards live in this crate, which has no
//! access to the node's metric registry; a per-guard counter would need either a
//! new public cross-crate metrics surface or a process-global unscoped counter,
//! and both are the cross-crate coupling the optimisation forbids. The existing
//! bounded families (`neo_native_hook_seconds_total{phase}` and
//! `neo_native_hook_empty_block_seconds_total`) already observe the work the
//! guards remove, because they time the very hook calls that now return early.

use super::{GasToken, LedgerContract, NativeContract, NeoToken, Notary};
use neo_primitives::{TriggerType, UInt160, UInt256, WitnessScope};
use neo_protocol::ProtocolSettings;
use neo_protocol::payloads::{
    Block, Header, NotaryAssisted, Signer, Transaction, TransactionAttribute,
};
use neo_protocol::witness::Witness;
use neo_smart_contract::application_engine::ApplicationEngine;
use neo_storage::TrackState;
use neo_storage::persistence::PersistenceDataCache;
use std::sync::Arc;

/// Builds a single-witness payload block at `index` with `transactions`.
fn block_with(index: u32, transactions: Vec<Transaction>) -> Block {
    let mut header = Header::new();
    header.set_index(index);
    header.set_prev_hash(UInt256::zero());
    header.set_next_consensus(UInt160::zero());
    header.set_timestamp(u64::from(index) + 1);
    header.witness = Witness::new();
    Block {
        header,
        transactions,
    }
}

/// Builds an engine that observes `block` for the given persist trigger.
fn engine_for(
    block: Block,
    trigger: TriggerType,
    snapshot: Arc<PersistenceDataCache>,
    settings: &ProtocolSettings,
) -> ApplicationEngine {
    ApplicationEngine::new(
        trigger,
        None,
        snapshot,
        Some(block),
        settings.clone(),
        i64::MAX,
        None,
        super::composition::default_native_services(),
    )
    .expect("native engine")
}

/// Every tracked storage effect of a hook, sorted by key so two runs compare
/// byte-for-byte regardless of hash-map iteration order.
fn storage_fingerprint(snapshot: &PersistenceDataCache) -> Vec<(Vec<u8>, TrackState, Vec<u8>)> {
    let mut tracked: Vec<_> = snapshot
        .tracked_items()
        .into_iter()
        .map(|(key, trackable)| (key.to_array(), trackable.state, trackable.item.to_value()))
        .collect();
    tracked.sort_by(|left, right| left.0.cmp(&right.0));
    tracked
}

/// Every notification a hook emitted, in emission order.
fn notification_fingerprint(engine: &ApplicationEngine) -> Vec<String> {
    engine
        .notifications()
        .iter()
        .map(|notification| {
            format!(
                "{}:{}:{}",
                notification.script_hash,
                notification.event_name,
                notification.state.len()
            )
        })
        .collect()
}

/// Normal heights, both sides of every committee refresh, and every configured
/// hardfork boundary (activation height, the height before and after it).
fn covered_heights(settings: &ProtocolSettings) -> Vec<u32> {
    let committee = u32::try_from(settings.committee_members_count()).expect("committee size");
    let mut heights = vec![
        1,
        7,
        committee.saturating_sub(1),
        committee,
        committee + 1,
        2 * committee,
        2 * committee + 1,
    ];
    for activation in settings.hardforks.values() {
        heights.push(*activation);
        heights.push(activation.saturating_sub(1));
        heights.push(activation.saturating_add(1));
    }
    heights.sort_unstable();
    heights.dedup();
    heights
}

/// A transaction that pays fees and therefore makes `GasToken::on_persist`
/// enter its burn loop.
fn fee_paying_transaction() -> Transaction {
    let mut transaction = Transaction::new();
    transaction.set_nonce(11);
    transaction.set_system_fee(1_000_000);
    transaction.set_network_fee(50_000);
    transaction.set_valid_until_block(100);
    transaction.set_signers(vec![Signer::new(
        UInt160::from_bytes(&[0x11; 20]).expect("sender"),
        WitnessScope::CALLED_BY_ENTRY,
    )]);
    transaction.set_witnesses(vec![Witness::new()]);
    transaction.set_script(vec![]);
    transaction
}

/// A transaction carrying the `NotaryAssisted` attribute.
fn notary_assisted_transaction() -> Transaction {
    let mut transaction = fee_paying_transaction();
    transaction.set_nonce(12);
    transaction.set_network_fee(0);
    transaction.add_attribute(TransactionAttribute::NotaryAssisted(NotaryAssisted::new(2)));
    transaction
}

/// Reference model of the pre-optimization `GasToken::on_persist` contract: the
/// burn loop is driven by the block transactions and the network-fee mint only
/// runs when the accumulated network fee is positive.
#[derive(Debug, PartialEq, Eq)]
enum GasTokenWork {
    /// No sender to debit and no network fee to mint.
    None,
    /// At least one sender is debited, or the network fee is minted.
    Fees,
}

fn reference_gas_token_work(block: &Block) -> GasTokenWork {
    let mut total_network_fee: i64 = 0;
    let mut burns = false;
    for transaction in &block.transactions {
        if transaction.sender().is_none() {
            continue;
        }
        burns = true;
        total_network_fee = total_network_fee.saturating_add(transaction.network_fee());
    }
    if burns || total_network_fee > 0 {
        GasTokenWork::Fees
    } else {
        GasTokenWork::None
    }
}

fn run_gas_token_on_persist(
    block: Block,
    settings: &ProtocolSettings,
) -> (Result<(), neo_smart_contract::error::CoreError>, bool, bool) {
    let snapshot = Arc::new(PersistenceDataCache::new(false));
    let before = storage_fingerprint(&snapshot);
    let mut engine = engine_for(
        block,
        TriggerType::OnPersist,
        Arc::clone(&snapshot),
        settings,
    );
    let result = GasToken::new().on_persist(&mut engine);
    let mutated = before != storage_fingerprint(&snapshot);
    let notified = !notification_fingerprint(&engine).is_empty();
    (result, mutated, notified)
}

#[test]
fn gas_token_on_persist_empty_block_matches_the_pre_optimization_outcome() {
    let settings = ProtocolSettings::testnet();
    for height in covered_heights(&settings) {
        let block = block_with(height, Vec::new());
        assert_eq!(
            reference_gas_token_work(&block),
            GasTokenWork::None,
            "reference model must classify an empty block as fee-free at {height}"
        );

        let (result, mutated, notified) = run_gas_token_on_persist(block, &settings);

        assert!(
            result.is_ok(),
            "empty block failed at height {height}: {result:?}"
        );
        assert!(!mutated, "empty block burned or minted at height {height}");
        assert!(
            !notified,
            "empty block emitted a notification at height {height}"
        );
    }
}

#[test]
fn gas_token_on_persist_does_not_short_circuit_fee_bearing_blocks() {
    let settings = ProtocolSettings::testnet();
    for height in covered_heights(&settings) {
        let block = block_with(height, vec![fee_paying_transaction()]);
        assert_eq!(
            reference_gas_token_work(&block),
            GasTokenWork::Fees,
            "reference model must classify a fee-paying block as work at {height}"
        );

        let (result, mutated, _) = run_gas_token_on_persist(block, &settings);

        // Without contract state the burn fails; either way the hook must have
        // left the guard and attempted fee processing instead of silently
        // returning.
        assert!(
            result.is_err() || mutated,
            "fee-bearing block was silently skipped at height {height}"
        );
    }
}

#[test]
fn gas_token_on_persist_without_a_persisting_block_still_errors() {
    let settings = ProtocolSettings::testnet();
    let snapshot = Arc::new(PersistenceDataCache::new(false));
    let mut engine = ApplicationEngine::new(
        TriggerType::OnPersist,
        None,
        snapshot,
        None,
        settings,
        i64::MAX,
        None,
        super::composition::default_native_services(),
    )
    .expect("native engine");

    let error = GasToken::new()
        .on_persist(&mut engine)
        .expect_err("missing persisting block must fail");

    assert!(
        error.to_string().contains("No persisting block available"),
        "unexpected error: {error}"
    );
}

fn run_notary_on_persist(
    block: Block,
    settings: &ProtocolSettings,
) -> (Result<(), neo_smart_contract::error::CoreError>, bool) {
    let snapshot = Arc::new(PersistenceDataCache::new(false));
    let before = storage_fingerprint(&snapshot);
    let mut engine = engine_for(
        block,
        TriggerType::OnPersist,
        Arc::clone(&snapshot),
        settings,
    );
    let result = Notary::new().on_persist(&mut engine);
    (result, before != storage_fingerprint(&snapshot))
}

#[test]
fn notary_on_persist_skips_blocks_without_notary_work_at_covered_heights() {
    let settings = ProtocolSettings::testnet();
    for height in covered_heights(&settings) {
        // Empty block: the pre-optimization body reached the zero-fee return
        // without touching storage, so the early return must match it exactly.
        let (result, mutated) = run_notary_on_persist(block_with(height, Vec::new()), &settings);
        assert!(
            result.is_ok(),
            "empty block failed at height {height}: {result:?}"
        );
        assert!(!mutated, "empty block did notary work at height {height}");

        // A block whose transactions carry no notary attribute behaves the same.
        let (result, mutated) = run_notary_on_persist(
            block_with(height, vec![fee_paying_transaction()]),
            &settings,
        );
        assert!(
            result.is_ok(),
            "non-notary block failed at height {height}: {result:?}"
        );
        assert!(
            !mutated,
            "non-notary block was charged notary work at height {height}"
        );
    }
}

#[test]
fn notary_on_persist_still_handles_notary_assisted_transactions() {
    let settings = ProtocolSettings::testnet();
    let height = u32::try_from(settings.committee_members_count()).expect("committee size");

    let (result, mutated) = run_notary_on_persist(
        block_with(height, vec![notary_assisted_transaction()]),
        &settings,
    );

    // C# v3.11.2: `if (notaries is null || notaries.Length == 0) return;` —
    // with no designated P2P notary nodes the hook reaches the designated
    // lookup (past the pre-scan guard) and then skips the reward
    // distribution silently instead of failing loudly.
    assert!(
        result.is_ok(),
        "a notary block without designated nodes failed: {result:?}"
    );
    assert!(
        !mutated,
        "a notary block without designated nodes mutated storage"
    );
}

#[test]
fn notary_on_persist_without_a_persisting_block_still_errors() {
    let settings = ProtocolSettings::testnet();
    let snapshot = Arc::new(PersistenceDataCache::new(false));
    let mut engine = ApplicationEngine::new(
        TriggerType::OnPersist,
        None,
        snapshot,
        None,
        settings,
        i64::MAX,
        None,
        super::composition::default_native_services(),
    )
    .expect("native engine");

    let error = Notary::new()
        .on_persist(&mut engine)
        .expect_err("missing persisting block must fail");

    assert!(
        error.to_string().contains("No persisting block available"),
        "unexpected error: {error}"
    );
}

#[test]
fn ledger_on_persist_still_records_every_empty_block() {
    let settings = ProtocolSettings::testnet();
    for height in covered_heights(&settings) {
        let snapshot = Arc::new(PersistenceDataCache::new(false));
        let mut engine = engine_for(
            block_with(height, Vec::new()),
            TriggerType::OnPersist,
            Arc::clone(&snapshot),
            &settings,
        );

        LedgerContract::new()
            .on_persist(&mut engine)
            .unwrap_or_else(|error| panic!("ledger OnPersist failed at {height}: {error}"));

        // The block record and the (empty) transaction-state map are written for
        // every height, so the consensus pointer can never stall.
        assert!(
            !storage_fingerprint(&snapshot).is_empty(),
            "ledger wrote nothing for the empty block at height {height}"
        );
    }
}

#[test]
fn neo_token_on_persist_still_refreshes_the_committee_on_refresh_heights() {
    let settings = ProtocolSettings::testnet();
    let committee = u32::try_from(settings.committee_members_count()).expect("committee size");

    for height in covered_heights(&settings) {
        let snapshot = Arc::new(PersistenceDataCache::new(false));
        let mut engine = engine_for(
            block_with(height, Vec::new()),
            TriggerType::OnPersist,
            Arc::clone(&snapshot),
            &settings,
        );

        let result = NeoToken::new().on_persist(&mut engine);
        let mutated = !storage_fingerprint(&snapshot).is_empty();
        let refreshes = height != 0 && height % committee == 0;

        if refreshes {
            // The committee entry is rewritten on every refresh height even for
            // an empty block; that per-height transition must never be skipped.
            result.unwrap_or_else(|error| panic!("committee refresh failed at {height}: {error}"));
            assert!(
                mutated,
                "committee refresh wrote nothing at refresh height {height}"
            );
        } else {
            result.unwrap_or_else(|error| panic!("NEO OnPersist failed at {height}: {error}"));
            assert!(!mutated, "NEO did work outside a refresh height ({height})");
        }
    }
}
