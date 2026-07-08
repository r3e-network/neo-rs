//! Consensus driver state machine and validator-node task wiring.
//!
//! This module owns node-side orchestration around `neo-consensus`: round
//! snapshots, dBFT event routing, recovery-log setup, and committed-block
//! submission back into the blockchain service. Protocol validation and
//! proposal selection stay in sibling modules.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use neo_blockchain::{
    BlockProvider, BlockchainHandle, ChainTipProvider, LedgerProviderFactory, RuntimeEvent,
    StorageLedgerProviderFactory,
};
use neo_config::ProtocolSettings;
use neo_consensus::messages::ConsensusPayload;
use neo_consensus::{ConsensusEvent, ConsensusService, ValidatorInfo};
use neo_crypto::ECPoint;
use neo_io::Serializable;
use neo_mempool::MemoryPool;
use neo_network::NetworkHandle;
use neo_payloads::Transaction;
use neo_primitives::time::now_millis;
use neo_primitives::{UInt160, UInt256};
use neo_storage::persistence::{DataCache, Store, StoreCache};
use parking_lot::RwLock;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use super::DBFT_MAX_BLOCK_SYSTEM_FEE;
use super::native_provider::{
    ConsensusNativeProvider, ConsensusNativeProviderFactory, NativeConsensusProviderFactory,
};
use super::payload::consensus_to_extensible;
use super::proposal::{
    cache_available_proposal_transactions, prepare_request_passes_ledger_guards,
    resolve_transactions, select_primary_proposal_transactions,
};
use super::setup::{ConsensusSetup, resolve_public_key_index, validator_infos_from_keys};

/// Block version dBFT produces (C# Header default; consensus never sets a non-zero version).
const BLOCK_VERSION: u32 = 0;

/// Reads the current ledger tip from `snapshot` →
/// `(next_block_index, prev_hash, prev_timestamp)`.
fn ledger_tip(snapshot: &DataCache) -> (u32, UInt256, u64) {
    let ledger = StorageLedgerProviderFactory.provider(snapshot);
    let height = ledger.current_index().unwrap_or(0);
    let prev_hash = ledger.current_hash().unwrap_or_default();
    let prev_timestamp = ledger
        .header_by_hash(&prev_hash)
        .ok()
        .flatten()
        .map(|header| header.timestamp())
        .unwrap_or(0);
    (height + 1, prev_hash, prev_timestamp)
}

fn round_validator_context(
    snapshot: &DataCache,
    settings: &ProtocolSettings,
    block_index: u32,
) -> anyhow::Result<(Vec<ValidatorInfo>, UInt160)> {
    let native = NativeConsensusProviderFactory.provider();
    round_validator_context_with_provider(&native, snapshot, settings, block_index)
}

fn round_validator_context_with_provider(
    native: &impl ConsensusNativeProvider,
    snapshot: &DataCache,
    settings: &ProtocolSettings,
    block_index: u32,
) -> anyhow::Result<(Vec<ValidatorInfo>, UInt160)> {
    let validators_count = usize::try_from(settings.validators_count).unwrap_or(0);
    let validators =
        validator_infos_from_keys(native.next_block_validators(snapshot, validators_count)?);
    let next_consensus =
        native.next_consensus_address_for_block(snapshot, settings, block_index)?;
    Ok((validators, next_consensus))
}

/// The single-task consensus driver: owns the `ConsensusService` (so no lock is
/// needed) and routes its events to the network/mempool/ledger.
pub(super) struct ConsensusDriver {
    pub(super) service: ConsensusService,
    pub(super) event_rx: mpsc::Receiver<ConsensusEvent>,
    pub(super) inbound_rx: mpsc::Receiver<ConsensusPayload>,
    /// Hashes of transactions freshly accepted into the mempool (from peer
    /// relay or local submission). Feeds the C# `ConsensusService.OnTransaction`
    /// late-transaction path so a backup that was missing a proposal
    /// transaction can resume the round when it finally arrives.
    pub(super) tx_feed_rx: mpsc::Receiver<UInt256>,
    pub(super) blockchain: BlockchainHandle,
    pub(super) mempool: Arc<MemoryPool>,
    pub(super) network: NetworkHandle,
    pub(super) settings: Arc<ProtocolSettings>,
    pub(super) validators: Arc<RwLock<Vec<ValidatorInfo>>>,
    pub(super) public_key: ECPoint,
    /// Underlying store handle, used to mint a fresh `DataCache` at the start of
    /// each round so committee/validator/`NextConsensus` reads reflect the current
    /// persisted tip (C# `ConsensusContext.Reset` takes a fresh snapshot per round).
    pub(super) store: Arc<dyn Store>,
    /// The `prev_hash` of the round currently being driven (carried into
    /// `assemble_block`).
    pub(super) current_prev_hash: UInt256,
    /// Full transactions cached at proposal time, for commit-time assembly.
    pub(super) proposal_txs: HashMap<UInt256, Arc<Transaction>>,
}

/// Builds the dBFT recovery-log file path under the node data directory, mirroring
/// C# `DbftSettings.RecoveryLogs` (default sub-store name `"ConsensusState"`).
/// Returns `None` when no data directory is configured (in-memory / test runs),
/// which disables persistence (C# `IgnoreRecoveryLogs`).
fn recovery_log_path(data_dir: Option<&std::path::Path>) -> Option<std::path::PathBuf> {
    let dir = data_dir?.join("ConsensusState");
    if let Err(err) = std::fs::create_dir_all(&dir) {
        warn!(target: "neo", %err, dir = %dir.display(), "could not create consensus recovery-log dir; persistence disabled");
        return None;
    }
    Some(dir.join("consensus-state.bin"))
}

impl ConsensusDriver {
    /// Builds a fresh read snapshot of the current persisted store state. Called
    /// once per round (at start and on each `Imported`) so a driver process that
    /// spans a committee-refresh height reads the updated validator set rather
    /// than a frozen startup snapshot.
    fn fresh_round_snapshot(&self) -> Arc<DataCache> {
        Arc::new(
            StoreCache::new_from_store(Arc::clone(&self.store), false)
                .data_cache()
                .clone(),
        )
    }

    fn configure_round(
        &mut self,
        snapshot: &DataCache,
        block_index: u32,
    ) -> anyhow::Result<UInt160> {
        let (validators, next_consensus) =
            round_validator_context(snapshot, &self.settings, block_index)?;
        let my_index = resolve_public_key_index(&self.public_key, &validators);
        self.service.update_validators(validators.clone(), my_index);
        *self.validators.write() = validators;

        // Re-read the live per-block time from the round snapshot before the
        // timer arithmetic in `start_with_block_context`, matching C#
        // `ConsensusContext.Reset(0)` which sets `TimePerBlock =
        // neoSystem.GetTimePerBlock()` once per block round. Post-Echidna this is
        // the committee-settable `PolicyContract.MillisecondsPerBlock`; pre-Echidna
        // (and on the pre-genesis fallback) the reader returns the
        // `ProtocolSettings` default, so this replaces the frozen construction-time
        // value on every round. Without this, a committee `setMillisecondsPerBlock`
        // would desync Rust validators' block timers from the C# committee.
        let native = NativeConsensusProviderFactory.provider();
        let ms_per_block = native.milliseconds_per_block(snapshot, &self.settings)?;
        self.service
            .set_expected_block_time(u64::from(ms_per_block));

        // C# `DbftSettings.MaxBlockSize` / `MaxBlockSystemFee`: the block-policy
        // limits a backup re-checks in `CheckPrepareResponse` before sending its
        // `PrepareResponse`. Use the same source the primary applies in
        // `EnsureMaxBlockLimitation` (`select_primary_proposal_transactions` /
        // `proposed_block_policy_rejection`) so primary and backup agree.
        self.service
            .set_max_block_policy(self.settings.max_block_size, DBFT_MAX_BLOCK_SYSTEM_FEE);

        Ok(next_consensus)
    }

    async fn run(mut self) {
        // Fresh snapshot for the first round (refreshed on each Imported below).
        let mut round_snapshot = self.fresh_round_snapshot();
        // C# `ConsensusContext.Reset`: first round is height+1 over the tip.
        let (block_index, prev_hash, prev_timestamp) = ledger_tip(&round_snapshot);
        let next_consensus = match self.configure_round(&round_snapshot, block_index) {
            Ok(next_consensus) => next_consensus,
            Err(err) => {
                warn!(target: "neo", %err, "consensus round context unavailable");
                return;
            }
        };
        self.current_prev_hash = prev_hash;
        // C# `ConsensusService.OnStart`: before `InitializeConsensus`, attempt to
        // resume from the recovery log so a crash/restart cannot double-sign a
        // different block at the same (height, view). Only a log written for this
        // exact `block_index` resumes; otherwise fall through to a fresh start.
        let resumed = match self
            .service
            .try_load_and_resume(
                block_index,
                now_millis(),
                prev_hash,
                next_consensus,
                BLOCK_VERSION,
            )
            .await
        {
            Ok(resumed) => resumed,
            Err(err) => {
                warn!(target: "neo", %err, block_index, "consensus recovery-log resume failed; starting fresh");
                false
            }
        };
        if resumed {
            info!(target: "neo", block_index, "consensus resumed from recovery log");
        } else {
            match self.service.start_with_block_context(
                block_index,
                now_millis(),
                prev_hash,
                prev_timestamp,
                next_consensus,
                BLOCK_VERSION,
            ) {
                Ok(()) => info!(target: "neo", block_index, "consensus started"),
                Err(err) => {
                    info!(target: "neo", %err, block_index, "consensus not started; driver idle");
                }
            }
        }

        let mut persist_rx = self.blockchain.subscribe();
        let mut ticker = tokio::time::interval(Duration::from_millis(1_000));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                // Outbound work from the state machine.
                maybe_event = self.event_rx.recv() => {
                    let Some(event) = maybe_event else { break };
                    self.on_consensus_event(event, &round_snapshot).await;
                }
                // Inbound consensus payloads from peers.
                maybe_msg = self.inbound_rx.recv() => {
                    let Some(payload) = maybe_msg else { break };
                    if !prepare_request_passes_ledger_guards(
                        &payload,
                        &round_snapshot,
                        &self.mempool,
                        &self.settings,
                    ) {
                        continue;
                    }
                    if let Err(err) = self.service.process_message(payload).await {
                        warn!(target: "neo", %err, "consensus rejected inbound payload");
                    }
                }
                // Late-transaction feed (C# `ConsensusService.OnTransaction`):
                // a transaction just landed in the mempool. If this backup is
                // waiting on it for the current proposal, feed it in so the
                // round can resume instead of degrading to a view change.
                maybe_tx = self.tx_feed_rx.recv() => {
                    let Some(tx_hash) = maybe_tx else { break };
                    self.on_transaction_feed(tx_hash).await;
                }
                // A block persisted (locally committed or peer-synced) → next round.
                ev = persist_rx.recv() => {
                    match ev {
                        Ok(RuntimeEvent::Imported { hash, height, timestamp }) => {
                            let block_index = height + 1;
                            // Re-read committee/validators from the current tip.
                            round_snapshot = self.fresh_round_snapshot();
                            let next_consensus = match self.configure_round(&round_snapshot, block_index) {
                                Ok(next_consensus) => next_consensus,
                                Err(err) => {
                                    warn!(target: "neo", %err, block_index, "consensus round context unavailable");
                                    continue;
                                }
                            };
                            self.current_prev_hash = hash;
                            self.proposal_txs.clear();
                            match self.service.start_with_block_context(
                                block_index,
                                now_millis(),
                                hash,
                                timestamp,
                                next_consensus,
                                BLOCK_VERSION,
                            ) {
                                Ok(()) => info!(target: "neo", block_index, "consensus restarted"),
                                Err(err) => info!(target: "neo", %err, "consensus next round not started"),
                            }
                        }
                        Ok(RuntimeEvent::Reverted { .. })
                        | Ok(RuntimeEvent::TipChanged { .. }) => {
                            // dBFT round startup is tied to a persisted block
                            // import. Tip bookkeeping does not provide the
                            // timestamp/hash context needed for a new round.
                        }
                        Ok(RuntimeEvent::RelayResult { .. }) => {
                            // Relay outcomes are RPC/subscription feedback and
                            // must not perturb consensus round state.
                        }
                        Ok(RuntimeEvent::Shutdown) => break,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
                // View-timeout tick (the real deadline lives inside the context).
                _ = ticker.tick() => {
                    if let Err(err) = self.service.on_timer_tick(now_millis()).await {
                        warn!(target: "neo", %err, "consensus timer tick failed");
                    }
                }
            }
        }
        info!(target: "neo", "consensus driver loop exited");
    }

    /// Feeds a freshly-accepted mempool transaction into the consensus state
    /// machine (C# `ConsensusService.OnTransaction`). If the transaction is one
    /// the current proposal is waiting for, caches its full body for commit-time
    /// block assembly and hands the hash to the service, which resumes the round
    /// (sends the backup's PrepareResponse, re-checks the commit threshold) once
    /// the last missing transaction arrives.
    pub(super) async fn on_transaction_feed(&mut self, tx_hash: UInt256) {
        // Cache the full transaction for commit-time block assembly: the backup
        // pulled it via GetData into the mempool, and the committed-block path
        // resolves hashes from `proposal_txs` first, then the mempool. Populate
        // the cache now so a later mempool eviction cannot lose a transaction we
        // committed to.
        if self.service.context().is_proposed_transaction(&tx_hash) {
            if let Some(item) = self.mempool.get(&tx_hash) {
                // Feed the two block-policy metrics (C# `Transaction.Size` /
                // `Transaction.SystemFee`) so the backup can evaluate
                // `CheckPrepareResponse` once this late-arriving transaction
                // completes the proposal.
                self.service.record_transaction_metrics(
                    tx_hash,
                    <Transaction as Serializable>::size(item.transaction.as_ref()),
                    item.transaction.system_fee(),
                );
                self.proposal_txs.insert(tx_hash, item.transaction);
            }
        }
        if let Err(err) = self.service.on_transaction(tx_hash).await {
            warn!(target: "neo", %err, "consensus on_transaction failed");
        }
    }

    pub(super) async fn on_consensus_event(&mut self, event: ConsensusEvent, snapshot: &DataCache) {
        match event {
            ConsensusEvent::BroadcastMessage(payload) => {
                let ext = {
                    let validators = self.validators.read();
                    consensus_to_extensible(&payload, &validators)
                };
                if let Some(ext) = ext {
                    let _ = self.network.broadcast_extensible(ext).await;
                }
            }
            ConsensusEvent::RequestTransactions {
                max_count,
                invalid_tx_hashes,
                ..
            } => {
                let hashes = {
                    let validators = self.validators.read();
                    select_primary_proposal_transactions(
                        self.mempool.verified_snapshot(),
                        max_count,
                        &mut self.proposal_txs,
                        &validators,
                        &self.settings,
                        &invalid_tx_hashes,
                    )
                };
                // C# `ConsensusService.SendPrepareRequest`: right after the
                // primary broadcasts its `PrepareRequest` it announces the
                // proposal's transaction hashes via `Inv(TX)`, so any backup
                // that lacks a referenced transaction pulls it (each peer's
                // `Inv` handler auto-issues `GetData` for hashes it does not
                // hold). Without this a slow-to-propagate transaction strands
                // backups and forces a view change. `on_transactions_received`
                // below builds + broadcasts the `PrepareRequest`; announce the
                // same hashes here (C# order is PrepareRequest then Inv, but
                // both are async fire-and-forget on the wire).
                if !hashes.is_empty() {
                    if let Err(err) = self
                        .network
                        .broadcast_inv(neo_network::InventoryType::Transaction, hashes.clone())
                        .await
                    {
                        warn!(target: "neo", %err, "consensus proposal Inv(TX) broadcast failed");
                    }
                }
                if let Err(err) = self.service.on_transactions_received(hashes).await {
                    warn!(target: "neo", %err, "consensus on_transactions_received failed");
                }
            }
            ConsensusEvent::RequestProposalTransactions {
                transaction_hashes, ..
            } => {
                let availability = {
                    let validators = self.validators.read();
                    cache_available_proposal_transactions(
                        &transaction_hashes,
                        &mut self.proposal_txs,
                        &self.mempool,
                        snapshot,
                        &self.settings,
                        &validators,
                    )
                };
                // Feed the block-policy metrics (C# `Transaction.Size` /
                // `Transaction.SystemFee`) for every proposal transaction whose
                // body was just cached, so the backup's `CheckPrepareResponse`
                // policy checks (inside `send_prepare_response`) see the same
                // per-tx size/fee the primary used in `EnsureMaxBlockLimitation`.
                for hash in &availability.available {
                    if let Some(tx) = self.proposal_txs.get(hash) {
                        self.service.record_transaction_metrics(
                            *hash,
                            <Transaction as Serializable>::size(tx.as_ref()),
                            tx.system_fee(),
                        );
                    }
                }
                if let Err(err) = self
                    .service
                    .on_transactions_received(availability.available)
                    .await
                {
                    warn!(target: "neo", %err, "consensus on_transactions_received failed");
                }
                if let Some(reason) = availability.rejection_reason {
                    if let Err(err) = self.service.request_change_view(reason, now_millis()).await {
                        warn!(target: "neo", %err, ?reason, "consensus request_change_view failed");
                    }
                }
            }
            ConsensusEvent::BlockCommitted {
                block_index,
                block_data,
                ..
            } => {
                let txs = match resolve_transactions(
                    &block_data.transaction_hashes,
                    &self.proposal_txs,
                    &self.mempool,
                ) {
                    Some(txs) => txs,
                    None => {
                        error!(target: "neo", block_index, "missing transaction for committed block; cannot assemble");
                        return;
                    }
                };
                match block_data.assemble_block(BLOCK_VERSION, self.current_prev_hash, txs) {
                    Ok(block) => {
                        self.current_prev_hash = block.header.hash();
                        let block = Arc::new(block);
                        // Persist through the C# Blockchain.Persist pipeline; the
                        // validators already signed, so it is pre-verified.
                        let _ = self
                            .blockchain
                            .submit_inventory_block(Arc::clone(&block), true, true)
                            .await;
                        // The InventoryBlock handler does not relay, so broadcast
                        // the new block to peers explicitly.
                        let _ = self.network.broadcast_block((*block).clone()).await;
                        info!(target: "neo", block_index, "consensus produced + submitted block");
                        // The next round restarts off the RuntimeEvent::Imported.
                    }
                    Err(err) => {
                        error!(target: "neo", block_index, %err, "consensus block assembly failed")
                    }
                }
            }
            ConsensusEvent::ViewChanged {
                block_index,
                old_view,
                new_view,
            } => {
                info!(target: "neo", block_index, old_view, new_view, "consensus view changed");
            }
        }
    }
}

/// Builds the consensus driver future for a validator node, consuming the
/// caller-owned `inbound_rx` (its matching sender is wired into the network
/// forwarder before this is called — the network, and thus this driver, is
/// built after the forwarder). Returns `None` when this node is relay-only.
pub fn consensus_driver_task(
    setup: ConsensusSetup,
    blockchain: BlockchainHandle,
    mempool: Arc<MemoryPool>,
    network: NetworkHandle,
    settings: Arc<ProtocolSettings>,
    validators: Arc<RwLock<Vec<ValidatorInfo>>>,
    store: Arc<dyn Store>,
    data_dir: Option<&std::path::Path>,
    inbound_rx: mpsc::Receiver<ConsensusPayload>,
    tx_feed_rx: mpsc::Receiver<UInt256>,
) -> Option<impl std::future::Future<Output = ()> + Send + 'static> {
    // Generously sized: a commit emits BroadcastMessage(Commit) + BlockCommitted
    // back-to-back via the consensus crate's non-blocking try_send.
    let (event_tx, event_rx) = mpsc::channel::<ConsensusEvent>(1024);

    let mut service = ConsensusService::new(
        setup.network,
        setup.validators.clone(),
        setup.my_index,
        setup.private_key.to_vec(),
        event_tx,
    );
    // When an HSM-backed signer is configured, route consensus signing through
    // it (the software private_key above is zeroed and unused in that case).
    service.set_signer(setup.signer.clone());
    service.set_expected_block_time(setup.ms_per_block);
    service.set_max_transactions_per_block(settings.max_transactions_per_block);
    // Crash-recovery persistence (C# `DbftSettings.RecoveryLogs`): the context is
    // saved before this node broadcasts its own Commit and reloaded on startup.
    // A missing data directory (in-memory node) disables it (C# `IgnoreRecoveryLogs`).
    let state_path = recovery_log_path(data_dir);
    if let Some(ref path) = state_path {
        info!(target: "neo", path = %path.display(), "dBFT recovery log enabled");
    }
    service.set_state_path(state_path);

    let driver = ConsensusDriver {
        service,
        event_rx,
        inbound_rx,
        tx_feed_rx,
        blockchain,
        mempool,
        network,
        settings,
        validators,
        public_key: setup.public_key,
        store,
        current_prev_hash: UInt256::default(),
        proposal_txs: HashMap::new(),
    };

    Some(driver.run())
}
