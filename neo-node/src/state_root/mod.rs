//! # neo-node::state_root
//!
//! Active signed-StateRoot (StateValidators) consensus driver.
//!
//! ## Boundary
//!
//! This module belongs to `neo-node`. This application crate may compose lower
//! layers but must not define protocol bytes, storage formats, consensus rules,
//! or VM semantics. The deterministic vote/aggregate/verify core lives in
//! `neo-blockchain` ([`neo_blockchain::StateRootVoteCollector`],
//! [`neo_blockchain::verify_state_root_with_native_provider`]); this module is
//! the node-side driver that feeds it network payloads and persists the
//! finalized signed root.
//!
//! ## C# reference
//!
//! Mirrors `Neo.Plugins.StateService`:
//! - `StatePlugin` (extensible category `"StateService"`, block-persist hook),
//! - `VerificationService` (inbound routing, vote/state-root relay, timers),
//! - `VerificationContext` (per-round verifier set, my-index, sender rotation,
//!   vote signing, `M`-of-`N` aggregation).
//!
//! ## Contents
//!
//! - the extensible `<-> {Vote, StateRoot}` codec,
//! - [`StateRootSetup`]/[`build_state_root_setup`]: resolve this node's optional
//!   StateValidator key,
//! - the single-task [`StateRootDriver`] and [`state_root_driver_task`] builder.

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
use neo_payloads::{ExtensiblePayload, Witness};
use neo_primitives::time::now_millis;
use neo_primitives::{UInt160, hex_util};
use neo_state_service::{
    MessageType, STATE_SERVICE_CATEGORY, StateRoot, StateStore, StateStoreLookup, Vote,
};
use neo_storage::DataCache;
use neo_storage::persistence::{Store, StoreCache};
use neo_vm::script_builder::{RedeemScript, ScriptBuilder};
use tokio::sync::mpsc;
use tracing::{info, warn};

/// Vote extensible `ValidBlockEnd` reach past the root index (C#
/// `VerificationService.MaxCachedVerificationProcessCount`).
const VOTE_VALID_BLOCK_END_THRESHOLD: u32 = 10;
/// StateRoot extensible `ValidBlockEnd` reach past the root index (C#
/// `VerificationContext.MaxValidUntilBlockIncrement`).
const STATE_ROOT_VALID_BLOCK_END_THRESHOLD: u32 = 100;
/// Initial delay before a validator first broadcasts its vote for a round
/// (C# `VerificationService.DelayMilliseconds`).
const INITIAL_VOTE_DELAY_MS: u64 = 3_000;
/// Cap the exponential retry backoff shift so `ms_per_block << retries` cannot
/// overflow or stall a round indefinitely.
const MAX_RETRY_SHIFT: u32 = 6;

// ===================== extensible <-> {Vote, StateRoot} codec =====================

/// Builds a `StateService` extensible carrying a `[MessageType][payload]` body,
/// signed by the sender's key. Mirrors C# `VerificationContext.CreatePayload`.
fn build_extensible(
    message_type: MessageType,
    payload_bytes: &[u8],
    root_index: u32,
    valid_block_end_threshold: u32,
    private_key: &[u8; 32],
    public_key: &ECPoint,
    network: u32,
) -> Option<ExtensiblePayload> {
    let mut data = Vec::with_capacity(1 + payload_bytes.len());
    data.push(message_type.to_byte());
    data.extend_from_slice(payload_bytes);

    let redeem = RedeemScript::signature_redeem_script(public_key.as_bytes());
    let mut ext = ExtensiblePayload::new();
    ext.category = STATE_SERVICE_CATEGORY.to_string();
    ext.valid_block_start = root_index;
    ext.valid_block_end = root_index.saturating_add(valid_block_end_threshold);
    ext.sender = UInt160::from_script(&redeem);
    ext.data = data;

    // Sign the extensible itself so peers accept and relay it (its witness must
    // match `sender`). Sign-data = network magic (LE) || payload hash.
    let hash = ext.hash();
    let mut sign_data = [0u8; 4 + 32];
    sign_data[..4].copy_from_slice(&network.to_le_bytes());
    sign_data[4..].copy_from_slice(&hash.to_bytes());
    let signature = Secp256r1Crypto::sign(&sign_data, private_key).ok()?;
    ext.witness = Witness::new_with_scripts(
        ScriptBuilder::new()
            .invocation_from_signature(&signature)
            .to_array(),
        redeem,
    );
    Some(ext)
}

/// Splits an inbound `StateService` extensible into its `(MessageType, body)`.
fn decode_message(ext: &ExtensiblePayload) -> Option<(MessageType, &[u8])> {
    if ext.category != STATE_SERVICE_CATEGORY {
        return None;
    }
    let (&type_byte, body) = ext.data.split_first()?;
    let message_type = MessageType::from_byte(type_byte)?;
    Some((message_type, body))
}

// ===================== setup =====================

/// This node's optional StateValidator identity and the timing it needs.
pub struct StateRootSetup {
    /// The StateValidator signing key + its public point, or `None` when this
    /// node only verifies and persists inbound signed roots (observer).
    pub keypair: Option<([u8; 32], ECPoint)>,
    /// Network magic (signed into vote and extensible sign-data).
    pub network: u32,
    /// Target block time (ms) — the retry-backoff base.
    pub ms_per_block: u64,
}

/// Resolves the StateService driver setup. Returns `Ok(None)` when the state
/// service is disabled (no local roots to attest). When a validator key is
/// configured it is parsed into a keypair; otherwise the node runs as an
/// observer that still verifies and persists inbound signed roots.
pub fn build_state_root_setup(
    settings: &ProtocolSettings,
    state_service_enabled: bool,
    validator_key_hex: Option<&str>,
) -> anyhow::Result<Option<StateRootSetup>> {
    if !state_service_enabled {
        return Ok(None);
    }
    let keypair = match validator_key_hex {
        Some(hex_key) => {
            let raw = hex_util::decode_hex(hex_key.trim())
                .map_err(|e| anyhow::anyhow!("invalid state validator private key hex: {e}"))?;
            let private_key: [u8; 32] = raw
                .as_slice()
                .try_into()
                .map_err(|_| anyhow::anyhow!("state validator private key must be 32 bytes"))?;
            let public_key = ECPoint::from_bytes(
                &Secp256r1Crypto::derive_public_key(&private_key)
                    .map_err(|e| anyhow::anyhow!("failed to derive state validator key: {e}"))?,
            )
            .map_err(|e| anyhow::anyhow!("failed to decode state validator key: {e}"))?;
            Some((private_key, public_key))
        }
        None => None,
    };
    Ok(Some(StateRootSetup {
        keypair,
        network: settings.network,
        ms_per_block: u64::from(settings.milliseconds_per_block),
    }))
}

// ===================== the driver =====================

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
struct StateRootDriver {
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
    fn sender_index(root_index: u32, retries: u32, n: usize) -> usize {
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
        if let Some(round) = self.active.get_mut(&round_index) {
            if !round.finalized {
                round.retries = round.retries.saturating_add(1);
                let shift = round.retries.min(MAX_RETRY_SHIFT);
                round.next_action_ms = now_millis() + (self.setup.ms_per_block << shift);
            }
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
        if let Some((private_key, public_key)) = self.setup.keypair.clone() {
            if let Some(ext) = build_extensible(
                MessageType::StateRoot,
                &signed.to_array(),
                root_index,
                STATE_ROOT_VALID_BLOCK_END_THRESHOLD,
                &private_key,
                &public_key,
                self.setup.network,
            ) {
                let _ = self.network.broadcast_extensible(ext).await;
                info!(target: "neo::state_root", root_index, "relayed signed state root");
            }
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
                        Ok(_) => {}
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

#[cfg(test)]
#[path = "../tests/state_root/mod.rs"]
mod tests;
