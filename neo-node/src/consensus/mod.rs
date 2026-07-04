//! # neo-node::consensus
//!
//! Consensus-facing node adapters and startup helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-node`. This application crate may compose lower
//! layers but must not define protocol bytes, storage formats, consensus rules,
//! or VM semantics.
//!
//! ## Contents
//!
//! - `hsm`: node-side HSM signer wiring.
//! - `proposal`: consensus proposal construction helpers.
//! - `tests`: Module-local tests and regression coverage.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use neo_blockchain::{BlockchainCommand, BlockchainHandle, RuntimeEvent};
use neo_config::ProtocolSettings;
use neo_consensus::messages::ConsensusPayload;
use neo_consensus::{ConsensusEvent, ConsensusService, ConsensusSigner, ValidatorInfo};
use neo_crypto::{ECPoint, Secp256r1Crypto};
use neo_mempool::MemoryPool;
use neo_native_contracts::{LedgerContract, NeoToken};
use neo_network::NetworkHandle;
use neo_payloads::{ExtensiblePayload, Transaction, Witness};
use neo_primitives::{UInt160, UInt256, hex_util};
use neo_storage::persistence::{DataCache, Store, StoreCache};
use neo_vm::script_builder::RedeemScript;
use parking_lot::RwLock;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

mod hsm;
mod proposal;

pub use hsm::HsmKeyConfig;
use hsm::build_hsm_consensus_setup;
use proposal::{
    cache_available_proposal_transactions, prepare_request_passes_ledger_guards,
    resolve_transactions, select_primary_proposal_transactions,
};

#[cfg(test)]
use proposal::{expected_dbft_block_size_without_transactions, proposal_rejection_reason};

/// dBFT extensible category (C# `ConsensusContext.CreatePayload`: `Category = "dBFT"`).
const DBFT_CATEGORY: &str = "dBFT";
/// Block version dBFT produces (C# Header default; consensus never sets a non-zero version).
const BLOCK_VERSION: u32 = 0;
/// C# DBFTPlugin `DbftSettings.MaxBlockSystemFee` default for Neo v3.10.0.
const DBFT_MAX_BLOCK_SYSTEM_FEE: i64 = 150_000_000_000;

/// Milliseconds since the Unix epoch — the same clock the consensus crate uses
/// for view-timeout accounting.
fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// ===================== Extensible <-> ConsensusPayload codec =====================

/// `PUSHDATA1 0x40 <64-byte sig>` — a single-signature invocation script.
fn invocation_script_from_signature(signature: &[u8]) -> Vec<u8> {
    let mut script = Vec::with_capacity(2 + signature.len());
    script.push(neo_vm_rs::OpCode::PUSHDATA1.byte());
    script.push(signature.len() as u8);
    script.extend_from_slice(signature);
    script
}

/// Extracts the raw 64-byte signature from a single-sig invocation script.
fn signature_from_invocation_script(invocation: &[u8]) -> Option<&[u8]> {
    if invocation.len() != 66
        || invocation[0] != neo_vm_rs::OpCode::PUSHDATA1.byte()
        || invocation[1] != 0x40
    {
        return None;
    }
    Some(&invocation[2..66])
}

/// Builds the outbound dBFT [`ExtensiblePayload`] for a `ConsensusPayload` the
/// service produced (its `witness` is the raw 64-byte signature). Mirrors C#
/// `ConsensusContext.CreatePayload`.
fn consensus_to_extensible(
    payload: &ConsensusPayload,
    validators: &[ValidatorInfo],
) -> Option<ExtensiblePayload> {
    let validator = validators.get(payload.validator_index as usize)?;
    let mut ext = ExtensiblePayload::new();
    ext.category = DBFT_CATEGORY.to_string();
    ext.valid_block_start = 0;
    ext.valid_block_end = payload.block_index;
    ext.sender = validator.script_hash;
    ext.data = payload.to_message_bytes();
    ext.witness = Witness::new_with_scripts(
        invocation_script_from_signature(&payload.witness),
        RedeemScript::signature_redeem_script(&validator.public_key.encoded()),
    );
    Some(ext)
}

/// Decodes an inbound dBFT [`ExtensiblePayload`] into a [`ConsensusPayload`].
/// Returns `None` for non-dBFT, malformed, or spoofed payloads (the in-body
/// `validator_index` must map to the validator whose script hash is the
/// extensible's `sender`).
pub fn extensible_to_consensus(
    ext: &ExtensiblePayload,
    network: u32,
    validators: &[ValidatorInfo],
) -> Option<ConsensusPayload> {
    if ext.category != DBFT_CATEGORY {
        return None;
    }
    let signature = signature_from_invocation_script(&ext.witness.invocation_script)?;
    let payload =
        ConsensusPayload::from_message_bytes(network, &ext.data, signature.to_vec()).ok()?;
    let validator = validators.get(payload.validator_index as usize)?;
    if validator.script_hash != ext.sender {
        return None;
    }
    Some(payload)
}

// ===================== validator-set + key derivation =====================

fn validator_infos_from_keys(keys: Vec<ECPoint>) -> Vec<ValidatorInfo> {
    keys.into_iter()
        .enumerate()
        .map(|(index, public_key)| {
            let script_hash = UInt160::from_script(&RedeemScript::signature_redeem_script(
                public_key.as_bytes(),
            ));
            ValidatorInfo {
                index: index as u8,
                public_key,
                script_hash,
            }
        })
        .collect()
}

/// Builds the ordered dBFT validator set from the protocol settings.
///
/// C# dBFT uses `NEO.GetNextBlockValidators(...).OrderBy(p => p)`, which at
/// genesis reduces to `StandbyCommittee.Take(ValidatorsCount).OrderBy(p => p)`.
/// `standby_validators()` does the `Take(N)` but NOT the sort; the validator
/// **index** (and thus primary selection) depends on the sorted order, so the
/// keys are sorted here.
pub fn build_consensus_validators(settings: &ProtocolSettings) -> Vec<ValidatorInfo> {
    let mut keys: Vec<ECPoint> = settings.standby_validators();
    keys.sort();
    validator_infos_from_keys(keys)
}

/// Finds this node's validator index by deriving its public key from the
/// private key and matching it against the (sorted) validator set. `None` when
/// this node is not a consensus validator (it then only relays).
pub fn resolve_my_index(private_key: &[u8; 32], validators: &[ValidatorInfo]) -> Option<u8> {
    let pub_bytes = Secp256r1Crypto::derive_public_key(private_key).ok()?;
    let my_pubkey = ECPoint::from_bytes(&pub_bytes).ok()?;
    resolve_public_key_index(&my_pubkey, validators)
}

fn resolve_public_key_index(public_key: &ECPoint, validators: &[ValidatorInfo]) -> Option<u8> {
    validators
        .iter()
        .find(|v| &v.public_key == public_key)
        .map(|v| v.index)
}

/// Resolved consensus configuration: the validator set and this node's key/index.
pub struct ConsensusSetup {
    /// The ordered dBFT validator set.
    pub validators: Vec<ValidatorInfo>,
    /// This node's public key, derived from `private_key`.
    pub public_key: ECPoint,
    /// This node's validator index, or `None` (observer; relay-only).
    pub my_index: Option<u8>,
    /// This node's 32-byte secp256r1 software private key. Zeroed and unused
    /// when `signer` is set (HSM-backed signing).
    pub private_key: [u8; 32],
    /// Network magic.
    pub network: u32,
    /// Target block time (ms) — the view-timeout base.
    pub ms_per_block: u64,
    /// Optional HSM-backed signer. When `Some`, the consensus service signs via
    /// this signer (keyed by the validator script hash) instead of `private_key`.
    pub signer: Option<Arc<dyn ConsensusSigner>>,
}

/// Builds the consensus setup from the protocol settings and the `[consensus]`
/// configuration. Returns `Ok(None)` when consensus is disabled. Returns an
/// error when consensus is enabled but the validator key is missing/malformed.
pub fn build_consensus_setup(
    settings: &ProtocolSettings,
    enabled: bool,
    private_key_hex: Option<&str>,
    hsm: Option<&HsmKeyConfig>,
) -> anyhow::Result<Option<ConsensusSetup>> {
    if !enabled {
        return Ok(None);
    }

    let validators = build_consensus_validators(settings);

    // HSM-backed signing takes precedence over a software key when configured.
    if let Some(hsm_cfg) = hsm {
        if private_key_hex.is_some() {
            warn!("[consensus] both private_key_hex and [consensus.hsm] are set; using the HSM");
        }
        return build_hsm_consensus_setup(settings, validators, hsm_cfg);
    }

    let hex_key = private_key_hex.ok_or_else(|| {
        anyhow::anyhow!(
            "[consensus].enabled = true requires [consensus].private_key_hex or [consensus.hsm]"
        )
    })?;
    let raw = hex_util::decode_hex(hex_key.trim())
        .map_err(|e| anyhow::anyhow!("invalid [consensus].private_key_hex: {e}"))?;
    let private_key: [u8; 32] = raw
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("[consensus].private_key_hex must be 32 bytes"))?;
    let public_key = ECPoint::from_bytes(
        &Secp256r1Crypto::derive_public_key(&private_key)
            .map_err(|e| anyhow::anyhow!("failed to derive consensus public key: {e}"))?,
    )
    .map_err(|e| anyhow::anyhow!("failed to decode consensus public key: {e}"))?;

    let my_index = resolve_my_index(&private_key, &validators);
    Ok(Some(ConsensusSetup {
        validators,
        public_key,
        my_index,
        private_key,
        network: settings.network,
        ms_per_block: u64::from(settings.milliseconds_per_block),
        signer: None,
    }))
}

// ===================== the driver =====================

/// Reads the current ledger tip from `snapshot` →
/// `(next_block_index, prev_hash, prev_timestamp)`.
fn ledger_tip(snapshot: &DataCache) -> (u32, UInt256, u64) {
    let ledger = LedgerContract::new();
    let height = ledger.current_index(snapshot).unwrap_or(0);
    let prev_hash = ledger.current_hash(snapshot).unwrap_or_default();
    let prev_timestamp = ledger
        .get_trimmed_block(snapshot, &prev_hash)
        .ok()
        .flatten()
        .map(|block| block.header.timestamp())
        .unwrap_or(0);
    (height + 1, prev_hash, prev_timestamp)
}

fn round_validator_context(
    snapshot: &DataCache,
    settings: &ProtocolSettings,
    block_index: u32,
) -> anyhow::Result<(Vec<ValidatorInfo>, UInt160)> {
    let validators_count = usize::try_from(settings.validators_count).unwrap_or(0);
    let validators = validator_infos_from_keys(
        NeoToken::new().next_block_validators(snapshot, validators_count)?,
    );
    let next_consensus =
        NeoToken::new().next_consensus_address_for_block(snapshot, settings, block_index)?;
    Ok((validators, next_consensus))
}

/// The single-task consensus driver: owns the `ConsensusService` (so no lock is
/// needed) and routes its events to the network/mempool/ledger.
struct ConsensusDriver {
    service: ConsensusService,
    event_rx: mpsc::Receiver<ConsensusEvent>,
    inbound_rx: mpsc::Receiver<ConsensusPayload>,
    blockchain: BlockchainHandle,
    mempool: Arc<MemoryPool>,
    network: NetworkHandle,
    settings: Arc<ProtocolSettings>,
    validators: Arc<RwLock<Vec<ValidatorInfo>>>,
    public_key: ECPoint,
    /// Underlying store handle, used to mint a fresh `DataCache` at the start of
    /// each round so committee/validator/`NextConsensus` reads reflect the current
    /// persisted tip (C# `ConsensusContext.Reset` takes a fresh snapshot per round).
    store: Arc<dyn Store>,
    /// The `prev_hash` of the round currently being driven (carried into
    /// `assemble_block`).
    current_prev_hash: UInt256,
    /// Full transactions cached at proposal time, for commit-time assembly.
    proposal_txs: HashMap<UInt256, Arc<Transaction>>,
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
                    if let Err(err) = self.service.process_message(payload) {
                        warn!(target: "neo", %err, "consensus rejected inbound payload");
                    }
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
                        Ok(_) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
                // View-timeout tick (the real deadline lives inside the context).
                _ = ticker.tick() => {
                    if let Err(err) = self.service.on_timer_tick(now_millis()) {
                        warn!(target: "neo", %err, "consensus timer tick failed");
                    }
                }
            }
        }
        info!(target: "neo", "consensus driver loop exited");
    }

    async fn on_consensus_event(&mut self, event: ConsensusEvent, snapshot: &DataCache) {
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
                if let Err(err) = self.service.on_transactions_received(hashes) {
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
                if let Err(err) = self
                    .service
                    .on_transactions_received(availability.available)
                {
                    warn!(target: "neo", %err, "consensus on_transactions_received failed");
                }
                if let Some(reason) = availability.rejection_reason {
                    if let Err(err) = self.service.request_change_view(reason, now_millis()) {
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
                            .tell(BlockchainCommand::InventoryBlock {
                                block: Arc::clone(&block),
                                relay: true,
                                pre_verified: true,
                            })
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
    inbound_rx: mpsc::Receiver<ConsensusPayload>,
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

    let driver = ConsensusDriver {
        service,
        event_rx,
        inbound_rx,
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

#[cfg(test)]
#[path = "../tests/consensus/mod.rs"]
mod tests;
