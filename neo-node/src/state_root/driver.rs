//! StateService voting, relay, and persistence driver.
//!
//! The deterministic vote aggregation and signed-root verification logic lives
//! below this crate. This module owns the node task that reacts to persisted
//! blocks, broadcasts local votes, ingests peer StateService payloads, and
//! persists verified signed roots.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use neo_blockchain::{
    BlockchainHandle, RuntimeEvent, StateRootVoteCollector, verify_state_root_with_native_provider,
};
use neo_config::ProtocolSettings;
use neo_crypto::{ECPoint, Secp256r1Crypto};
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_io::{MemoryReader, Serializable, SerializableExtensions};
use neo_native_contracts::{Role, RoleManagement};
use neo_network::NetworkHandle;
use neo_payloads::ExtensiblePayload;
use neo_primitives::time::now_millis;
use neo_state_service::{MessageType, StateRoot, StateStore, StateStoreLookup, Vote};
use neo_storage::DataCache;
use neo_storage::persistence::{Store, StoreCache};
use tokio::sync::mpsc;
use tracing::{info, warn};

use super::StateRootSetup;
use super::codec::{
    STATE_ROOT_VALID_BLOCK_END_THRESHOLD, VOTE_VALID_BLOCK_END_THRESHOLD, build_extensible,
    decode_message,
};

/// Initial delay before a validator first broadcasts its vote for a round
/// (C# `VerificationService.DelayMilliseconds`).
const INITIAL_VOTE_DELAY_MS: u64 = 3_000;
/// Cap the exponential retry backoff shift so `ms_per_block << retries` cannot
/// overflow or stall a round indefinitely.
const MAX_RETRY_SHIFT: u32 = 6;

/// A voting round this node is a designated StateValidator for.
struct ActiveRound {
    verifiers: Vec<ECPoint>,
    my_index: usize,
    retries: u32,
    /// Epoch-ms at which to next (re)send the vote and rotate the sender.
    next_action_ms: u64,
    finalized: bool,
}

/// Single-task driver: owns the vote collector and per-round state, routes
/// inbound `StateService` payloads, and persists finalized signed roots.
pub(super) struct StateRootDriver {
    setup: StateRootSetup,
    blockchain: BlockchainHandle,
    network: NetworkHandle,
    settings: Arc<ProtocolSettings>,
    /// Chain store: `RoleManagement` designations are read from a fresh snapshot.
    store: Arc<dyn Store>,
    /// Native contract provider captured at node startup for witness verification.
    native_contract_provider: Arc<dyn NativeContractProvider>,
    /// Local computed roots + persisted signed roots.
    state_store: Arc<StateStore>,
    inbound_rx: mpsc::Receiver<ExtensiblePayload>,
    collector: StateRootVoteCollector,
    active: HashMap<u32, ActiveRound>,
}

impl StateRootDriver {
    /// A fresh read snapshot of the current persisted chain state, for
    /// `RoleManagement` designation lookups (C# reads `NeoSystem.StoreView`).
    fn fresh_chain_snapshot(&self) -> Arc<DataCache> {
        Arc::new(
            StoreCache::new_from_store(Arc::clone(&self.store), false)
                .data_cache()
                .clone(),
        )
    }

    /// The StateValidators designated at `index`, in designation order (the
    /// order vote `validator_index` and multisig aggregation both index into).
    fn verifiers_at(&self, snapshot: &DataCache, index: u32) -> Vec<ECPoint> {
        RoleManagement::new()
            .get_designated_by_role_at(snapshot, Role::StateValidator, index)
            .unwrap_or_default()
    }

    /// The locally-computed (unsigned) state root for `index`, if available.
    fn local_root(&self, index: u32) -> Option<StateRoot> {
        self.state_store
            .get_state_root(StateStoreLookup::ByBlockIndex(index))
    }

    /// This node's index in `verifiers`, if it is a designated StateValidator.
    fn my_index_in(&self, verifiers: &[ECPoint]) -> Option<usize> {
        let (_, public_key) = self.setup.keypair.as_ref()?;
        verifiers.iter().position(|v| v == public_key)
    }

    /// The current sender for `root_index` at `retries` retries, matching C#
    /// `VerificationContext.Sender = ((rootIndex - retries) mod N)`.
    pub(super) fn sender_index(root_index: u32, retries: u32, n: usize) -> usize {
        let n = n as i64;
        let p = (i64::from(root_index) - i64::from(retries)).rem_euclid(n);
        p as usize
    }

    /// A block persisted: open a voting round for its state root if this node is
    /// a designated StateValidator with the local root available.
    async fn on_block_persisted(&mut self, index: u32) {
        // Bound memory: drop finalized/stale rounds well below the new tip.
        self.active
            .retain(|round_index, _| *round_index + VOTE_VALID_BLOCK_END_THRESHOLD >= index);
        self.collector
            .prune_below(index.saturating_sub(VOTE_VALID_BLOCK_END_THRESHOLD));

        if self.setup.keypair.is_none() {
            return; // observer: only verifies + persists inbound signed roots.
        }
        let snapshot = self.fresh_chain_snapshot();
        let verifiers = self.verifiers_at(&snapshot, index);
        let Some(my_index) = self.my_index_in(&verifiers) else {
            return; // not designated for this round.
        };
        self.active.insert(
            index,
            ActiveRound {
                verifiers,
                my_index,
                retries: 0,
                next_action_ms: now_millis() + INITIAL_VOTE_DELAY_MS,
                finalized: false,
            },
        );
    }

    /// (Re)broadcasts this node's vote for `round_index` and feeds it to the
    /// collector, then schedules the next retry (rotating the sender).
    async fn fire_round(&mut self, round_index: u32) {
        let Some(round) = self.active.get(&round_index) else {
            return;
        };
        if round.finalized {
            return;
        }
        let Some((private_key, public_key)) = self.setup.keypair.clone() else {
            return;
        };
        let my_index = round.my_index;
        let verifiers = round.verifiers.clone();

        let Some(mut root) = self.local_root(round_index) else {
            // Local root not computed yet; retry on the next tick without
            // advancing the retry counter (keeps the current sender).
            if let Some(round) = self.active.get_mut(&round_index) {
                round.next_action_ms = now_millis() + INITIAL_VOTE_DELAY_MS;
            }
            return;
        };

        let signature =
            Secp256r1Crypto::sign(&root.get_sign_data(self.setup.network), &private_key);
        let Ok(signature) = signature else {
            warn!(target: "neo::state_root", round_index, "state root vote signing failed");
            return;
        };

        let vote = Vote {
            validator_index: my_index as i32,
            root_index: round_index,
            signature: signature.to_vec(),
        };
        if let Ok(bytes) = vote.to_array() {
            if let Some(ext) = build_extensible(
                MessageType::Vote,
                &bytes,
                round_index,
                VOTE_VALID_BLOCK_END_THRESHOLD,
                &private_key,
                &public_key,
                self.setup.network,
            ) {
                let _ = self.network.broadcast_extensible(ext).await;
            }
        }

        // Feed our own vote into the collector (may finalize a small validator set).
        self.ingest_vote(round_index, my_index, signature.to_vec(), &verifiers)
            .await;

        // Schedule the next retry with exponential backoff, rotating the sender.
        if let Some(round) = self.active.get_mut(&round_index)
            && !round.finalized
        {
            round.retries = round.retries.saturating_add(1);
            let shift = round.retries.min(MAX_RETRY_SHIFT);
            round.next_action_ms = now_millis() + (self.setup.ms_per_block << shift);
        }
    }

    /// Validates a vote and, on reaching `M`, persists the signed root and (if
    /// this node is the round's sender) broadcasts it.
    async fn ingest_vote(
        &mut self,
        root_index: u32,
        validator_index: usize,
        signature: Vec<u8>,
        verifiers: &[ECPoint],
    ) {
        let Some(mut root) = self.local_root(root_index) else {
            return;
        };
        let Some(signed) = self.collector.add_vote(
            &mut root,
            validator_index,
            signature,
            verifiers,
            self.setup.network,
        ) else {
            return;
        };
        self.finalize_signed_root(root_index, signed, verifiers.len())
            .await;
    }

    /// Persists an aggregated signed root and broadcasts it when this node is
    /// the round's current sender (C# `VerificationService.CheckVotes`).
    async fn finalize_signed_root(&mut self, root_index: u32, signed: StateRoot, n: usize) {
        // Idempotent persist: `try_add_state_root` returns false if already stored.
        let signed_clone = signed.clone();
        if self.state_store.try_add_state_root(signed_clone.clone()) {
            self.state_store
                .commit_validated_state_roots(&[signed_clone]);
            info!(
                target: "neo::state_root",
                root_index,
                "persisted network-signed state root",
            );
        }

        let should_broadcast = self
            .active
            .get(&root_index)
            .map(|round| round.my_index == Self::sender_index(root_index, round.retries, n))
            .unwrap_or(false);

        // Only the round's current sender relays the aggregated root, and only
        // it stops here. A non-sender validator has the signed root persisted but
        // keeps its round active so the retry timer rotates the sender — this is
        // what preserves liveness when the designated sender is offline (a peer
        // relay, or our own turn on a later retry, then finalizes the round).
        if !should_broadcast {
            return;
        }
        if let Some((private_key, public_key)) = self.setup.keypair.clone()
            && let Some(ext) = build_extensible(
                MessageType::StateRoot,
                &signed.to_array(),
                root_index,
                STATE_ROOT_VALID_BLOCK_END_THRESHOLD,
                &private_key,
                &public_key,
                self.setup.network,
            )
        {
            let _ = self.network.broadcast_extensible(ext).await;
            info!(target: "neo::state_root", root_index, "relayed signed state root");
        }
        if let Some(round) = self.active.get_mut(&root_index) {
            round.finalized = true;
        }
    }

    /// Routes an inbound `StateService` extensible: a `Vote` into the collector,
    /// or a signed `StateRoot` to verification + persistence.
    async fn on_inbound(&mut self, ext: ExtensiblePayload) {
        let Some((message_type, body)) = decode_message(&ext) else {
            return;
        };
        match message_type {
            MessageType::Vote => {
                let Ok(vote) = Vote::deserialize(&mut MemoryReader::new(body)) else {
                    return;
                };
                if vote.validator_index < 0 {
                    return;
                }
                let snapshot = self.fresh_chain_snapshot();
                let verifiers = self.verifiers_at(&snapshot, vote.root_index);
                if verifiers.is_empty() {
                    return;
                }
                self.ingest_vote(
                    vote.root_index,
                    vote.validator_index as usize,
                    vote.signature,
                    &verifiers,
                )
                .await;
            }
            MessageType::StateRoot => {
                let Ok(root) = StateRoot::deserialize(&mut MemoryReader::new(body)) else {
                    return;
                };
                let root_index = root.index();
                let snapshot = self.fresh_chain_snapshot();
                if !verify_state_root_with_native_provider(
                    &root,
                    &self.settings,
                    &snapshot,
                    Some(Arc::clone(&self.native_contract_provider)),
                ) {
                    warn!(target: "neo::state_root", root_index, "rejected unverifiable signed state root");
                    return;
                }
                if self.state_store.try_add_state_root(root.clone()) {
                    self.state_store.commit_validated_state_roots(&[root]);
                    info!(
                        target: "neo::state_root",
                        root_index,
                        "persisted peer-relayed signed state root",
                    );
                }
                if let Some(round) = self.active.get_mut(&root_index) {
                    round.finalized = true;
                }
            }
        }
    }

    async fn run(mut self) {
        let mut persist_rx = self.blockchain.subscribe();
        let mut ticker = tokio::time::interval(Duration::from_millis(1_000));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                // Inbound StateService payloads from peers.
                maybe_ext = self.inbound_rx.recv() => {
                    let Some(ext) = maybe_ext else { break };
                    self.on_inbound(ext).await;
                }
                // A block persisted -> open a voting round for its state root.
                ev = persist_rx.recv() => {
                    match ev {
                        Ok(RuntimeEvent::Imported { height, .. }) => {
                            self.on_block_persisted(height).await;
                        }
                        Ok(RuntimeEvent::Reverted { .. })
                        | Ok(RuntimeEvent::TipChanged { .. }) => {
                            // StateService voting is opened only by a newly
                            // persisted block. Tip/revert notifications are
                            // canonical-chain bookkeeping for consumers that
                            // maintain read indexes.
                        }
                        Ok(RuntimeEvent::RelayResult { .. }) => {
                            // Relay outcomes do not create state roots.
                        }
                        Ok(RuntimeEvent::Shutdown) => break,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
                // Retry tick: (re)send votes for due, unfinalized rounds.
                _ = ticker.tick() => {
                    let now = now_millis();
                    let due: Vec<u32> = self
                        .active
                        .iter()
                        .filter(|(_, r)| !r.finalized && now >= r.next_action_ms)
                        .map(|(index, _)| *index)
                        .collect();
                    for round_index in due {
                        self.fire_round(round_index).await;
                    }
                }
            }
        }
        info!(target: "neo::state_root", "state root driver loop exited");
    }
}

/// Builds the StateService driver future. Consumes the caller-owned
/// `inbound_rx` (its sender is wired into the inventory forwarder before the
/// network is built). Runs for validators (vote + aggregate + relay) and
/// observers (verify + persist inbound signed roots) alike.
// Rationale: the state-root driver is the node composition seam and must
// receive every provider/handle explicitly instead of capturing globals.
#[allow(clippy::too_many_arguments)]
pub fn state_root_driver_task(
    setup: StateRootSetup,
    blockchain: BlockchainHandle,
    network: NetworkHandle,
    settings: Arc<ProtocolSettings>,
    store: Arc<dyn Store>,
    native_contract_provider: Arc<dyn NativeContractProvider>,
    state_store: Arc<StateStore>,
    inbound_rx: mpsc::Receiver<ExtensiblePayload>,
) -> impl std::future::Future<Output = ()> + Send + 'static {
    let driver = StateRootDriver {
        setup,
        blockchain,
        network,
        settings,
        store,
        native_contract_provider,
        state_store,
        inbound_rx,
        collector: StateRootVoteCollector::new(),
        active: HashMap::new(),
    };
    driver.run()
}
