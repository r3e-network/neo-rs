//! Node-level dBFT consensus driver.
//!
//! Wires the verified `neo-consensus` `ConsensusService` state machine into a
//! running node: it instantiates the service from the validator configuration,
//! converts the service's outbound `ConsensusEvent`s into network/mempool/ledger
//! actions, decodes inbound dBFT `ExtensiblePayload`s from peers back into
//! `ConsensusPayload`s, and drives the per-block round lifecycle.
//!
//! The block-production primitives this builds on (the dBFT state machine,
//! `BlockData::assemble_block`) are verified in neo-consensus; the end-to-end
//! multi-node consensus behaviour is exercised only in a real deployment.
//!
//! Behind the `wip` feature.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use neo_blockchain::{BlockchainCommand, BlockchainHandle, RuntimeEvent};
use neo_config::ProtocolSettings;
use neo_consensus::messages::ConsensusPayload;
use neo_consensus::{ConsensusEvent, ConsensusService, ValidatorInfo};
use neo_crypto::{ECPoint, Secp256r1Crypto};
use neo_mempool::MemoryPool;
use neo_native_contracts::LedgerContract;
use neo_network::NetworkHandle;
use neo_payloads::{ExtensiblePayload, Transaction, Witness};
use neo_primitives::{UInt160, UInt256};
use neo_redeem_script::signature_redeem_script;
use neo_storage::persistence::DataCache;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

/// dBFT extensible category (C# `ConsensusContext.CreatePayload`: `Category = "dBFT"`).
const DBFT_CATEGORY: &str = "dBFT";
/// Block version dBFT produces (C# Header default; consensus never sets a non-zero version).
const BLOCK_VERSION: u32 = 0;

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
        signature_redeem_script(&validator.public_key.encoded()),
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
    let payload = ConsensusPayload::from_message_bytes(network, &ext.data, signature.to_vec()).ok()?;
    let validator = validators.get(payload.validator_index as usize)?;
    if validator.script_hash != ext.sender {
        return None;
    }
    Some(payload)
}

// ===================== validator-set + key derivation =====================

/// Builds the ordered dBFT validator set from the protocol settings.
///
/// C# dBFT uses `NEO.GetNextBlockValidators(...).OrderBy(p => p)`, which at
/// genesis reduces to `StandbyCommittee.Take(ValidatorsCount).OrderBy(p => p)`.
/// `standby_validators()` does the `Take(N)` but NOT the sort; the validator
/// **index** (and thus primary selection + `NextConsensus`) depends on the
/// sorted order, so the keys are sorted here.
pub fn build_consensus_validators(settings: &ProtocolSettings) -> Vec<ValidatorInfo> {
    let mut keys: Vec<ECPoint> = settings.standby_validators();
    keys.sort();
    keys.into_iter()
        .enumerate()
        .map(|(index, public_key)| {
            let script_hash = UInt160::from_script(&signature_redeem_script(public_key.as_bytes()));
            ValidatorInfo {
                index: index as u8,
                public_key,
                script_hash,
            }
        })
        .collect()
}

/// Finds this node's validator index by deriving its public key from the
/// private key and matching it against the (sorted) validator set. `None` when
/// this node is not a consensus validator (it then only relays).
pub fn resolve_my_index(private_key: &[u8; 32], validators: &[ValidatorInfo]) -> Option<u8> {
    let pub_bytes = Secp256r1Crypto::derive_public_key(private_key).ok()?;
    let my_pubkey = ECPoint::from_bytes(&pub_bytes).ok()?;
    validators
        .iter()
        .find(|v| v.public_key == my_pubkey)
        .map(|v| v.index)
}

/// Resolved consensus configuration: the validator set and this node's key/index.
pub struct ConsensusSetup {
    /// The ordered dBFT validator set.
    pub validators: Vec<ValidatorInfo>,
    /// This node's validator index, or `None` (observer; relay-only).
    pub my_index: Option<u8>,
    /// This node's 32-byte secp256r1 private key.
    pub private_key: [u8; 32],
    /// Network magic.
    pub network: u32,
    /// Target block time (ms) — the view-timeout base.
    pub ms_per_block: u64,
}

/// Builds the consensus setup from the protocol settings and the `[consensus]`
/// configuration. Returns `Ok(None)` when consensus is disabled. Returns an
/// error when consensus is enabled but the validator key is missing/malformed.
pub fn build_consensus_setup(
    settings: &ProtocolSettings,
    enabled: bool,
    private_key_hex: Option<&str>,
) -> anyhow::Result<Option<ConsensusSetup>> {
    if !enabled {
        return Ok(None);
    }
    let hex_key = private_key_hex.ok_or_else(|| {
        anyhow::anyhow!("[consensus].enabled = true requires [consensus].private_key_hex")
    })?;
    let raw = hex::decode(hex_key.trim())
        .map_err(|e| anyhow::anyhow!("invalid [consensus].private_key_hex: {e}"))?;
    let private_key: [u8; 32] = raw
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("[consensus].private_key_hex must be 32 bytes"))?;

    let validators = build_consensus_validators(settings);
    let my_index = resolve_my_index(&private_key, &validators);
    Ok(Some(ConsensusSetup {
        validators,
        my_index,
        private_key,
        network: settings.network,
        ms_per_block: u64::from(settings.milliseconds_per_block),
    }))
}

// ===================== the driver =====================

/// Resolves the full transactions for `hashes`, in block order, from the
/// proposal cache then the live mempool. Returns `None` if any is missing.
fn resolve_transactions(
    hashes: &[UInt256],
    cache: &HashMap<UInt256, Arc<Transaction>>,
    mempool: &MemoryPool,
) -> Option<Vec<Transaction>> {
    let mut out = Vec::with_capacity(hashes.len());
    for hash in hashes {
        if let Some(tx) = cache.get(hash) {
            out.push((**tx).clone());
        } else if let Some(item) = mempool.get(hash) {
            out.push((*item.transaction).clone());
        } else {
            return None;
        }
    }
    Some(out)
}

/// Reads the current ledger tip from `snapshot` → `(next_block_index, prev_hash)`.
fn ledger_tip(snapshot: &DataCache) -> (u32, UInt256) {
    let ledger = LedgerContract::new();
    let height = ledger.current_index(snapshot).unwrap_or(0);
    let prev_hash = ledger.current_hash(snapshot).unwrap_or_default();
    (height + 1, prev_hash)
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
    validators: Vec<ValidatorInfo>,
    /// The `prev_hash` of the round currently being driven (carried into
    /// `assemble_block` — the stored snapshot does not advance, so the tip is
    /// tracked from `start`/`Imported` rather than re-read).
    current_prev_hash: UInt256,
    /// Full transactions cached at proposal time, for commit-time assembly.
    proposal_txs: HashMap<UInt256, Arc<Transaction>>,
}

impl ConsensusDriver {
    async fn run(mut self, startup_snapshot: Arc<DataCache>) {
        // C# `ConsensusContext.Reset`: first round is height+1 over the tip.
        let (block_index, prev_hash) = ledger_tip(&startup_snapshot);
        self.current_prev_hash = prev_hash;
        match self.service.start(block_index, now_millis(), prev_hash, BLOCK_VERSION) {
            Ok(()) => info!(target: "neo", block_index, "consensus started"),
            Err(err) => {
                info!(target: "neo", %err, "consensus not started (not a validator); driver idle");
                return;
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
                    self.on_consensus_event(event).await;
                }
                // Inbound consensus payloads from peers.
                maybe_msg = self.inbound_rx.recv() => {
                    let Some(payload) = maybe_msg else { break };
                    if let Err(err) = self.service.process_message(payload) {
                        warn!(target: "neo", %err, "consensus rejected inbound payload");
                    }
                }
                // A block persisted (locally committed or peer-synced) → next round.
                ev = persist_rx.recv() => {
                    match ev {
                        Ok(RuntimeEvent::Imported { hash, height }) => {
                            self.current_prev_hash = hash;
                            self.proposal_txs.clear();
                            match self.service.start(height + 1, now_millis(), hash, BLOCK_VERSION) {
                                Ok(()) => info!(target: "neo", block_index = height + 1, "consensus restarted"),
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

    async fn on_consensus_event(&mut self, event: ConsensusEvent) {
        match event {
            ConsensusEvent::BroadcastMessage(payload) => {
                if let Some(ext) = consensus_to_extensible(&payload, &self.validators) {
                    let _ = self.network.broadcast_extensible(ext).await;
                }
            }
            ConsensusEvent::RequestTransactions { max_count, .. } => {
                let mut hashes = Vec::new();
                for item in self.mempool.verified_snapshot().into_iter().take(max_count) {
                    let hash = item.hash();
                    self.proposal_txs.insert(hash, Arc::clone(&item.transaction));
                    hashes.push(hash);
                }
                if let Err(err) = self.service.on_transactions_received(hashes) {
                    warn!(target: "neo", %err, "consensus on_transactions_received failed");
                }
            }
            ConsensusEvent::BlockCommitted { block_index, block_data, .. } => {
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
                    Err(err) => error!(target: "neo", block_index, %err, "consensus block assembly failed"),
                }
            }
            ConsensusEvent::ViewChanged { block_index, old_view, new_view } => {
                info!(target: "neo", block_index, old_view, new_view, "consensus view changed");
            }
        }
    }
}

/// Spawns the consensus driver for a validator node, consuming the caller-owned
/// `inbound_rx` (its matching sender is wired into the network forwarder before
/// this is called — the network, and thus this driver, is built after the
/// forwarder). Returns the driver task handle, or `None` when this node is not
/// a consensus validator (relay-only).
pub fn spawn_consensus_driver(
    setup: ConsensusSetup,
    blockchain: BlockchainHandle,
    mempool: Arc<MemoryPool>,
    network: NetworkHandle,
    startup_snapshot: Arc<DataCache>,
    inbound_rx: mpsc::Receiver<ConsensusPayload>,
) -> Option<tokio::task::JoinHandle<()>> {
    setup.my_index?;

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
    service.set_expected_block_time(setup.ms_per_block);

    let driver = ConsensusDriver {
        service,
        event_rx,
        inbound_rx,
        blockchain,
        mempool,
        network,
        validators: setup.validators,
        current_prev_hash: UInt256::default(),
        proposal_txs: HashMap::new(),
    };

    Some(tokio::spawn(driver.run(startup_snapshot)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The dBFT extensible codec round-trips a consensus payload: encode a
    /// signed `ConsensusPayload` to an `ExtensiblePayload`, then decode it back
    /// to the same fields (the inbound path authenticates the sender).
    #[test]
    fn extensible_codec_round_trips() {
        let settings = ProtocolSettings::default();
        let validators = build_consensus_validators(&settings);
        assert!(!validators.is_empty(), "default settings carry a committee");

        let validator_index = 0u8;
        let signature = vec![0xABu8; 64];
        let mut original = ConsensusPayload::new(
            settings.network,
            7,             // block_index
            validator_index,
            0,             // view_number
            neo_consensus::ConsensusMessageType::Commit,
            vec![0x01, 0x02, 0x03], // body
        );
        original.witness = signature.clone();

        let ext = consensus_to_extensible(&original, &validators).expect("encode");
        assert_eq!(ext.category, DBFT_CATEGORY);
        assert_eq!(ext.valid_block_end, 7);
        assert_eq!(ext.sender, validators[validator_index as usize].script_hash);

        let decoded =
            extensible_to_consensus(&ext, settings.network, &validators).expect("decode");
        assert_eq!(decoded.block_index, 7);
        assert_eq!(decoded.validator_index, validator_index);
        assert_eq!(decoded.message_type, neo_consensus::ConsensusMessageType::Commit);
        assert_eq!(decoded.data, vec![0x01, 0x02, 0x03]);
        assert_eq!(decoded.witness, signature);
    }

    /// A non-dBFT extensible is ignored by the consensus decoder.
    #[test]
    fn extensible_codec_rejects_non_dbft() {
        let settings = ProtocolSettings::default();
        let validators = build_consensus_validators(&settings);
        let mut ext = ExtensiblePayload::new();
        ext.category = "StateService".to_string();
        ext.valid_block_end = 1;
        assert!(extensible_to_consensus(&ext, settings.network, &validators).is_none());
    }

    /// The validator set is sorted ascending by public key (consensus-critical:
    /// the index order drives primary selection + NextConsensus).
    #[test]
    fn validators_are_sorted_by_pubkey() {
        let settings = ProtocolSettings::default();
        let validators = build_consensus_validators(&settings);
        for pair in validators.windows(2) {
            assert!(pair[0].public_key <= pair[1].public_key, "validators must be sorted");
        }
        for (i, v) in validators.iter().enumerate() {
            assert_eq!(v.index as usize, i);
        }
    }

    /// `build_consensus_setup` is a no-op when disabled and errors when enabled
    /// without a key.
    #[test]
    fn setup_gating() {
        let settings = ProtocolSettings::default();
        assert!(build_consensus_setup(&settings, false, None).unwrap().is_none());
        assert!(build_consensus_setup(&settings, true, None).is_err());
        assert!(build_consensus_setup(&settings, true, Some("zz")).is_err());
    }
}
