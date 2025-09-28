//! Rust translation of Neo's `ConsensusContext`.

use crate::dbft_plugin::dbft_settings::DbftSettings;
use crate::dbft_plugin::messages::ConsensusMessagePayload;
use neo_core::cryptography::ECPoint;
use neo_core::ledger::TransactionVerificationContext;
use neo_core::network::p2p::payloads::{Block, ExtensiblePayload, Witness};
use neo_core::persistence::{DataCache, IStore, StoreCache};
use neo_core::sign::ISigner;
use neo_core::time_provider::TimeProvider;
use neo_core::{MerkleTree, NeoSystem, Transaction, UInt256};
use neo_core::UInt160;
use neo_core::neo_io::{BinaryWriter, MemoryReader};
use neo_core::extensions::{BinaryWriterExtensions, MemoryReaderExtensions};
use neo_core::smart_contract::Contract;
use neo_core::smart_contract::native::NativeHelpers;
use neo_vm::ScriptBuilder;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tracing::debug;

pub use neo_core::network::p2p::payloads::ExtensiblePayload;

/// In-memory representation of the consensus state for the dBFT plugin.
#[allow(clippy::struct_excessive_bools)]
pub struct ConsensusContext {
    pub block: Block,
    pub view_number: u8,
    pub time_per_block: Duration,
    pub validators: Vec<ECPoint>,
    pub my_index: i32,
    pub transaction_hashes: Option<Vec<UInt256>>,
    pub transactions: Option<HashMap<UInt256, Transaction>>,
    pub preparation_payloads: Vec<Option<ExtensiblePayload>>,
    pub commit_payloads: Vec<Option<ExtensiblePayload>>,
    pub change_view_payloads: Vec<Option<ExtensiblePayload>>,
    pub last_change_view_payloads: Vec<Option<ExtensiblePayload>>,
    pub last_seen_message: HashMap<ECPoint, u32>,
    pub verification_context: TransactionVerificationContext,
    cached_messages: HashMap<UInt256, ConsensusMessagePayload>,
    pub neo_system: Arc<NeoSystem>,
    pub dbft_settings: DbftSettings,
    pub signer: Arc<dyn ISigner>,
    pub store: Option<Arc<dyn IStore>>,
    pub snapshot: Option<StoreCache>,
    pub data_cache: DataCache,
    pub witness_size: usize,
    pub my_public_key: Option<ECPoint>,
}

impl ConsensusContext {
    /// Creates a new consensus context instance.
    pub fn new(
        neo_system: Arc<NeoSystem>,
        settings: DbftSettings,
        signer: Arc<dyn ISigner>,
    ) -> Self {
        let milliseconds_per_block = neo_system
            .settings()
            .milliseconds_per_block as u64;
        let time_per_block = Duration::from_millis(milliseconds_per_block);

        Self {
            block: Block::new(),
            view_number: 0,
            time_per_block,
            validators: Vec::new(),
            my_index: -1,
            transaction_hashes: None,
            transactions: None,
            preparation_payloads: Vec::new(),
            commit_payloads: Vec::new(),
            change_view_payloads: Vec::new(),
            last_change_view_payloads: Vec::new(),
            last_seen_message: HashMap::new(),
            verification_context: TransactionVerificationContext::new(),
            cached_messages: HashMap::new(),
            neo_system,
            dbft_settings: settings,
            signer,
            store: None,
            snapshot: None,
            data_cache: DataCache::new(true),
            witness_size: 0,
            my_public_key: None,
        }
    }

    /// Returns the current block reference.
    pub fn block(&self) -> &Block {
        &self.block
    }

    /// Returns a mutable reference to the current block.
    pub fn block_mut(&mut self) -> &mut Block {
        &mut self.block
    }

    /// Current view number.
    pub fn view_number(&self) -> u8 {
        self.view_number
    }

    /// Reference to the owning Neo system.
    pub fn neo_system(&self) -> &Arc<NeoSystem> {
        &self.neo_system
    }

    /// dBFT configuration for the running node.
    pub fn settings(&self) -> &DbftSettings {
        &self.dbft_settings
    }

    /// Returns the fault tolerance threshold `F`.
    pub fn f(&self) -> usize {
        self.validators
            .len()
            .saturating_sub(1)
            .checked_div(3)
            .unwrap_or(0)
    }

    /// Returns the minimum number of validators required for consensus `M`.
    pub fn m(&self) -> usize {
        self.validators.len().saturating_sub(self.f())
    }

    /// True if this node is the primary validator for the current view.
    pub fn is_primary(&self) -> bool {
        self.my_index >= 0 && (self.my_index as u8) == self.block.primary_index()
    }

    /// True if this node participates as a backup validator.
    pub fn is_backup(&self) -> bool {
        self.my_index >= 0 && (self.my_index as u8) != self.block.primary_index()
    }

    /// True if this node only observes the consensus process.
    pub fn watch_only(&self) -> bool {
        self.my_index < 0
    }

    /// Counts validators that have produced a commit payload.
    pub fn count_committed(&self) -> usize {
        self.commit_payloads
            .iter()
            .filter(|payload| payload.is_some())
            .count()
    }

    /// Counts validators that have not produced recent messages.
    pub fn count_failed(&self) -> usize {
        if self.last_seen_message.is_empty() {
            return 0;
        }

        let expected_height = self.block.index().saturating_sub(1);
        self.validators
            .iter()
            .filter(|validator| {
                self.last_seen_message
                    .get(*validator)
                    .map(|seen| *seen < expected_height)
                    .unwrap_or(true)
            })
            .count()
    }

    /// Returns true if the current proposal has been sent or received.
    pub fn request_sent_or_received(&self) -> bool {
        let primary = self.block.primary_index() as usize;
        self
            .preparation_payloads
            .get(primary)
            .map(|payload| payload.is_some())
            .unwrap_or(false)
    }

    /// Returns true if this node has already broadcast a prepare response.
    pub fn response_sent(&self) -> bool {
        if self.watch_only() {
            return false;
        }

        let index = self.my_index as usize;
        self
            .preparation_payloads
            .get(index)
            .map(|payload| payload.is_some())
            .unwrap_or(false)
    }

    /// Returns true if this node has already broadcast a commit.
    pub fn commit_sent(&self) -> bool {
        if self.watch_only() {
            return false;
        }

        let index = self.my_index as usize;
        self
            .commit_payloads
            .get(index)
            .map(|payload| payload.is_some())
            .unwrap_or(false)
    }

    /// Returns true if the block has already been assembled and broadcast.
    pub fn block_sent(&self) -> bool {
        match (&self.transaction_hashes, &self.transactions) {
            (Some(hashes), Some(transactions)) => hashes
                .iter()
                .all(|hash| transactions.contains_key(hash)),
            _ => false,
        }
    }

    /// Returns true if the node is currently in a view-change state.
    pub fn view_changing(&self) -> bool {
        if self.watch_only() {
            return false;
        }

        let index = self.my_index as usize;
        match self.change_view_payloads.get(index).and_then(|payload| payload.as_ref()) {
            Some(payload) => self
                .get_message(payload)
                .and_then(|message| message.as_change_view().cloned())
                .map(|message| message.new_view_number() > self.view_number)
                .unwrap_or(false),
            None => false,
        }
    }

    /// Returns true if the node should refuse additional payloads due to view change.
    pub fn not_accepting_payloads_due_to_view_changing(&self) -> bool {
        self.view_changing() && !self.more_than_f_nodes_committed_or_lost()
    }

    /// Returns true if more than `F` nodes have committed or are marked missing.
    pub fn more_than_f_nodes_committed_or_lost(&self) -> bool {
        self.count_committed() + self.count_failed() > self.f()
    }

    /// Attempts to load consensus state from recovery logs.
    pub fn load(&mut self) -> bool {
        let Some(store) = &self.store else { return false };
        let snapshot = store.get_snapshot();
        match snapshot.try_get(&vec![0xF4]) {
            Some(data) => self.deserialize_state(&data).is_ok(),
            None => false,
        }
    }

    /// Persists the consensus state when recovery logs are enabled.
    pub fn save(&mut self) {
        if let Some(store) = &self.store {
            let mut snapshot = store.get_snapshot();
            snapshot.put(vec![0xF4], self.serialize_state());
            snapshot.commit();
        }
    }

    /// Serializes the consensus state using the same layout as the C# plugin.
    /// This produces a byte vector that can be stored under the consensus state key.
    fn serialize_state(&self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();

        // Header basics
        let header = &self.block.header;
        let version = header.version();
        let index = header.index();
        let timestamp = header.timestamp();
        let nonce = header.nonce();
        let primary_index = header.primary_index();
        let next_consensus = header.next_consensus().clone();

        let _ = writer.write_u32(version);
        let _ = writer.write_u32(index);
        let _ = writer.write_u64(timestamp);
        let _ = writer.write_u64(nonce);
        let _ = writer.write_u8(primary_index);
        let _ = writer.write_serializable(&next_consensus);

        // View number
        let _ = writer.write_u8(self.view_number);

        // Transaction hashes and transactions
        if let Some(hashes) = &self.transaction_hashes {
            let _ = writer.write_var_uint(hashes.len() as u64);
            for h in hashes {
                let _ = writer.write_serializable(h);
            }
        } else {
            let _ = writer.write_var_uint(0);
        }

        if let Some(txs) = &self.transactions {
            let values: Vec<_> = txs.values().cloned().collect();
            let _ = writer.write_serializable_collection(&values);
        } else {
            let _ = writer.write_var_uint(0);
        }

        // Nullable arrays of payloads
        let _ = writer.write_nullable_array(&self.preparation_payloads);
        let _ = writer.write_nullable_array(&self.commit_payloads);
        let _ = writer.write_nullable_array(&self.change_view_payloads);
        let _ = writer.write_nullable_array(&self.last_change_view_payloads);

        writer.into_bytes()
    }

    /// Deserializes a consensus state produced by `serialize_state`.
    fn deserialize_state(&mut self, data: &[u8]) -> Result<(), String> {
        let mut reader = MemoryReader::new(data);

        // Reset non-persistent volatile state
        self.reset(0);

        // Header basics
        let version = reader.read_u32().map_err(|e| e.to_string())?;
        let index = reader.read_u32().map_err(|e| e.to_string())?;
        let timestamp = reader.read_u64().map_err(|e| e.to_string())?;
        let nonce = reader.read_u64().map_err(|e| e.to_string())?;
        let primary_index = reader.read_u8().map_err(|e| e.to_string())?;
        let next_consensus = UInt160::deserialize(&mut reader).map_err(|e| e.to_string())?;

        {
            let block_mut = self.block_mut();
            block_mut.header.set_version(version);
            block_mut.header.set_index(index);
            block_mut.header.set_timestamp(timestamp);
            block_mut.header.set_nonce(nonce);
            block_mut.header.set_primary_index(primary_index);
            block_mut.header.set_next_consensus(next_consensus);
        }

        // View number
        self.view_number = reader.read_u8().map_err(|e| e.to_string())?;

        // Transaction hashes
        let hash_count = reader.read_var_int(u16::MAX as u64).map_err(|e| e.to_string())? as usize;
        let mut hashes = Vec::with_capacity(hash_count);
        for _ in 0..hash_count {
            hashes.push(UInt256::deserialize(&mut reader).map_err(|e| e.to_string())?);
        }

        // Transactions
        let txs: Vec<Transaction> = reader
            .read_serializable_array::<Transaction>(u16::MAX as usize)
            .map_err(|e| e.to_string())?;

        // Payload arrays
        let max = self.neo_system.settings().validators_count as usize;
        self.preparation_payloads = reader
            .read_nullable_array::<ExtensiblePayload>(max)
            .map_err(|e| e.to_string())?;
        self.commit_payloads = reader
            .read_nullable_array::<ExtensiblePayload>(max)
            .map_err(|e| e.to_string())?;
        self.change_view_payloads = reader
            .read_nullable_array::<ExtensiblePayload>(max)
            .map_err(|e| e.to_string())?;
        self.last_change_view_payloads = reader
            .read_nullable_array::<ExtensiblePayload>(max)
            .map_err(|e| e.to_string())?;

        // Apply hashes/transactions following C# semantics
        if hashes.is_empty() && !self.request_sent_or_received() {
            self.transaction_hashes = None;
        } else {
            self.transaction_hashes = Some(hashes);
        }

        if txs.is_empty() && !self.request_sent_or_received() {
            self.transactions = None;
        } else {
            let mut map = HashMap::with_capacity(txs.len());
            for tx in txs.into_iter() {
                map.insert(tx.hash(), tx.clone());
                self.verification_context.add_transaction(&tx);
            }
            self.transactions = Some(map);
        }

        Ok(())
    }

    /// Resets the consensus context for the provided view number.
    pub fn reset(&mut self, view_number: u8) {
        if view_number == 0 {
            self.block = Block::new();
            self.transaction_hashes = None;
            self.transactions = Some(HashMap::new());
            self.my_public_key = None;
            self.data_cache = DataCache::new(true);

            // Initialize validators via native helpers (C#-consistent API); fallback to settings.
            let next_validators = NativeHelpers::get_next_block_validators(self.neo_system.settings());
            let previous_validators = std::mem::take(&mut self.validators);
            self.validators = next_validators;

            // Recompute witness size and next consensus when first run or validator count changes.
            if self.witness_size == 0 || previous_validators.len() != self.validators.len() {
                // Build a dummy multi-sig witness with M signatures to estimate size.
                let m = self.m();
                let mut sb = ScriptBuilder::new();
                let dummy_sig = vec![0u8; 64];
                for _ in 0..m {
                    sb.emit_push(&dummy_sig);
                }
                let invocation = sb.to_array();
                let verification = Contract::create_multi_sig_redeem_script(m, &self.validators);
                let witness = Witness::new_with_scripts(invocation, verification);
                self.witness_size = witness.size();

                // Compute next consensus address using the same branching as C# (refresh vs get)
                let next_height = NativeHelpers::current_index().saturating_add(1);
                let committee_members = self.neo_system.settings().committee_members_count();
                let next_validators = if NativeHelpers::should_refresh_committee(next_height, committee_members) {
                    NativeHelpers::compute_next_block_validators(self.neo_system.settings())
                } else {
                    NativeHelpers::get_next_block_validators(self.neo_system.settings())
                };
                // Update validators and NextConsensus consistently
                self.validators = next_validators;
                let next_consensus = NativeHelpers::get_bft_address(&self.validators);
                self.block.header.set_next_consensus(next_consensus);
            }

            // Set PrevHash and Index like C# Reset(0)
            let prev_hash = NativeHelpers::current_hash();
            let current_index = NativeHelpers::current_index();
            self.block.header.set_prev_hash(prev_hash);
            self.block.header.set_index(current_index.saturating_add(1));

            // Determine own validator index if wallet contains corresponding key
            self.my_index = -1;
            for (i, v) in self.validators.iter().enumerate() {
                if self.signer.contains_signable(v) {
                    self.my_index = i as i32;
                    self.my_public_key = Some(v.clone());
                    break;
                }
            }

            // Initialize LastSeenMessage with previous values if possible, otherwise current block index
            let mut new_last_seen = HashMap::new();
            for v in &self.validators {
                if let Some(val) = self.last_seen_message.get(v) {
                    new_last_seen.insert(v.clone(), *val);
                } else {
                    new_last_seen.insert(v.clone(), self.block.index());
                }
            }
            self.last_seen_message = new_last_seen;
        } else {
            let count = self.validators.len();
            self.last_change_view_payloads = (0..count)
                .map(|index| self.change_view_payloads.get(index).cloned().unwrap_or(None))
                .collect();
        }

        self.view_number = view_number;
        self.verification_context = TransactionVerificationContext::new();
        self.cached_messages.clear();
        self.resize_payload_buffers();

        if !self.validators.is_empty() {
            let primary = self.get_primary_index(view_number);
            self.block.header.set_primary_index(primary);
        }

        self.block.header.set_merkle_root(UInt256::default());
        self.block.header.set_timestamp(0);
        self.block.header.set_nonce(0);
        self.block.transactions.clear();

        if self.my_index >= 0 {
            if let Some(validator) = self.validators.get(self.my_index as usize) {
                self
                    .last_seen_message
                    .insert(validator.clone(), self.block.index());
            }
        }
    }

    /// Provides mutable access to the cached message map for helper modules.
    pub(crate) fn cached_messages_mut(&mut self) -> &mut HashMap<UInt256, ConsensusMessagePayload> {
        &mut self.cached_messages
    }

    /// Provides immutable access to the cached message map.
    pub(crate) fn cached_messages(&self) -> &HashMap<UInt256, ConsensusMessagePayload> {
        &self.cached_messages
    }

    /// Simple logger helper mirroring the C# logging behaviour.
    pub(crate) fn log(&self, message: &str) {
        debug!(target: "dbft::consensus_context", "{}", message);
    }

    /// Ensures internal payload buffers match the validator set size.
    fn resize_payload_buffers(&mut self) {
        let count = self.validators.len();
        self.preparation_payloads.resize_with(count, || None);
        self.commit_payloads.resize_with(count, || None);
        self.change_view_payloads.resize_with(count, || None);
        self.last_change_view_payloads.resize_with(count, || None);
    }

    /// Convenience accessor to the current UTC timestamp in milliseconds.
    pub(crate) fn current_timestamp(&self) -> u64 {
        TimeProvider::current().utc_now().timestamp_millis() as u64
    }

    /// Returns a reference to the validator set.
    pub fn validators(&self) -> &[ECPoint] {
        &self.validators
    }

    /// Updates the validator set and resizes payload buffers accordingly.
    pub fn set_validators(&mut self, validators: Vec<ECPoint>) {
        self.validators = validators;
        self.resize_payload_buffers();
    }

    /// Updates the persistence store reference to enable state load/save.
    pub fn set_store(&mut self, store: Arc<dyn IStore>) {
        // Keep a handle to the store and create a snapshot cache mirroring C# behaviour.
        self.store = Some(store.clone());
        self.snapshot = Some(StoreCache::new_from_store(store, false));
    }

    /// Returns the cached witness size hint.
    pub fn witness_size(&self) -> usize {
        self.witness_size
    }

    /// Updates the witness size hint used for block estimation.
    pub fn set_witness_size(&mut self, value: usize) {
        self.witness_size = value;
    }

    /// Returns the proposed transaction hashes, if any.
    pub fn transaction_hashes(&self) -> Option<&[UInt256]> {
        self.transaction_hashes.as_deref()
    }

    /// Returns the collected transactions for the current proposal.
    pub fn transactions(&self) -> Option<&HashMap<UInt256, Transaction>> {
        self.transactions.as_ref()
    }

    /// Mutable access to collected transactions.
    pub fn transactions_mut(&mut self) -> Option<&mut HashMap<UInt256, Transaction>> {
        self.transactions.as_mut()
    }

    /// Returns the preparation payload buffers.
    pub fn preparation_payloads(&self) -> &[Option<ExtensiblePayload>] {
        &self.preparation_payloads
    }

    /// Returns the commit payload buffers.
    pub fn commit_payloads(&self) -> &[Option<ExtensiblePayload>] {
        &self.commit_payloads
    }

    /// Returns a mutable reference to the last-seen message registry.
    pub fn last_seen_message_mut(&mut self) -> &mut HashMap<ECPoint, u32> {
        &mut self.last_seen_message
    }

    /// Returns the validator public key for this node when available.
    pub fn my_public_key(&self) -> Option<&ECPoint> {
        self.my_public_key.as_ref()
    }

    /// Updates the cached validator public key reference.
    pub fn set_my_public_key(&mut self, key: Option<ECPoint>) {
        self.my_public_key = key;
    }

    /// Ensures the block header has a merkle root consistent with the current transaction hashes.
    pub fn ensure_header(&mut self) -> &Block {
        if let Some(hashes) = &self.transaction_hashes {
            if !hashes.is_empty() && self.block.header.merkle_root().is_zero() {
                let payload_hashes: Vec<Vec<u8>> =
                    hashes.iter().map(|hash| hash.as_bytes().to_vec()).collect();
                if let Some(root) = MerkleTree::compute_root(&payload_hashes) {
                    if let Ok(merkle_root) = UInt256::try_from(root.as_slice()) {
                        self.block.header.set_merkle_root(merkle_root);
                    }
                }
            }
        }
        &self.block
    }

    /// Returns the previous block header when available. Placeholder until store integration is complete.
    pub fn prev_header(&self) -> Option<neo_core::network::p2p::payloads::Header> {
        None
    }
}
