//! # neo-rpc::server::session
//!
//! RPC session records and connection-local state.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `tests`: Module-local tests and regression coverage.

use neo_error::{CoreError, CoreResult};
use parking_lot::{Mutex, MutexGuard};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use neo_execution::ApplicationEngine;
use neo_execution::iterators::iterator::StorageIterator as _;
use neo_execution::iterators::{IteratorInterop, StorageIterator};
use neo_manifest::CallFlags;
use neo_native_contracts::ledger_contract::LedgerContract;
use neo_native_contracts::policy_contract::PolicyContract;
use neo_payloads::signer::Signer;
use neo_payloads::transaction::Transaction;
use neo_payloads::transaction_attribute::TransactionAttribute;
use neo_payloads::witness::Witness;
use neo_payloads::{Block, Header};
use neo_primitives::TriggerType;
use neo_primitives::Verifiable;
use neo_runtime::{ConfigProvider, StoreProvider};
use neo_storage::persistence::StoreCache;
use neo_vm::stack_item::{InteropInterface as VmInteropInterface, StackItem};
use rand::random;
use uuid::Uuid;

use crate::server::diagnostic::Diagnostic;

/// Trait representing an iterator stored within an RPC session.
pub trait SessionIterator: Send {
    /// Advance the iterator to the next item.
    fn next(&mut self) -> bool;
    /// Return the current item.
    fn value(&self) -> CoreResult<StackItem>;
    /// Release any resources owned by the iterator.
    fn dispose(&mut self);
}

/// Wrapper storing iterator instances with automatic disposal.
struct IteratorEntry {
    inner: Box<dyn SessionIterator>,
}

impl IteratorEntry {
    fn next(&mut self) -> bool {
        self.inner.next()
    }

    fn value(&self) -> CoreResult<StackItem> {
        self.inner.value()
    }

    fn dispose(&mut self) {
        self.inner.dispose();
    }
}

impl Drop for IteratorEntry {
    fn drop(&mut self) {
        self.dispose();
    }
}

/// Represents an invocation session that can retain iterators between RPC calls.
pub struct Session {
    script: Vec<u8>,
    snapshot: StoreCache,
    engine: Mutex<ApplicationEngine>,
    diagnostic: Mutex<Option<Diagnostic>>,
    iterators: Mutex<HashMap<Uuid, IteratorEntry>>,
    iterator_lookup: Mutex<HashMap<u32, Uuid>>,
    start_time: Mutex<Instant>,
}

#[derive(Debug)]
struct StorageSessionIterator {
    iterator: StorageIterator,
}

impl StorageSessionIterator {
    const fn new(iterator: StorageIterator) -> Self {
        Self { iterator }
    }
}

impl SessionIterator for StorageSessionIterator {
    fn next(&mut self) -> bool {
        self.iterator.next()
    }

    fn value(&self) -> CoreResult<StackItem> {
        self.iterator.value()
    }

    fn dispose(&mut self) {
        self.iterator.dispose();
    }
}

impl Session {
    /// Create and execute a new invocation session.
    ///
    /// The session owns the executed engine, a storage snapshot, any diagnostic
    /// output, and later any VM iterators exposed by the invocation result.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        store_provider: Arc<dyn StoreProvider>,
        config_provider: Arc<dyn ConfigProvider>,
        script: Vec<u8>,
        signers: Option<Vec<Signer>>,
        witnesses: Option<Vec<Witness>>,
        gas_limit: i64,
        diagnostic: Option<Diagnostic>,
    ) -> CoreResult<Self> {
        let store_cache = store_provider.store_cache();
        let snapshot_cache = Arc::new(store_cache.data_cache().clone());

        let settings = config_provider.settings();

        // C# `NeoSystemExtensions.GetMaxValidUntilBlockIncrement(snapshot,
        // settings)`: before HF_Echidna the static protocol setting, from
        // HF_Echidna onward the Policy storage value (falling back to the
        // setting when the key is not yet initialized). The static
        // `ConfigProvider::max_valid_until_block_increment()` is only correct
        // pre-Echidna, so read the Policy-aware value from the snapshot.
        let max_valid_until_block_increment = PolicyContract::new()
            .get_max_valid_until_block_increment_snapshot(
                store_cache.data_cache(),
                settings.as_ref(),
            )
            .unwrap_or_else(|_| config_provider.max_valid_until_block_increment());

        let tx_container = signers.as_ref().map(|signer_list| {
            let mut tx = Transaction::new();
            tx.set_version(0);
            tx.set_nonce(random());
            let valid_until = LedgerContract::new()
                .current_index(store_cache.data_cache())
                .unwrap_or(0)
                .saturating_add(max_valid_until_block_increment);
            tx.set_valid_until_block(valid_until);
            tx.set_signers(signer_list.clone());
            tx.set_attributes(Vec::<TransactionAttribute>::new());
            tx.set_script(script.clone());
            if let Some(ws) = &witnesses {
                tx.set_witnesses(ws.clone());
            } else {
                tx.set_witnesses(vec![Witness::new(); signer_list.len()]);
            }
            Arc::new(tx) as Arc<dyn Verifiable>
        });

        // C# `ApplicationEngine.Run` (invoked by the RPC invoke* methods) has no
        // persisting block, so it synthesizes one via
        // `ApplicationEngine.CreateDummyBlock(snapshot, settings)`. Without it,
        // `System.Runtime.GetTime` faults and `LedgerContract.CurrentIndex`-style
        // reads see `height` instead of the `height + 1` a real persisting block
        // would give. Build the same dummy block so stateless invoke *results*
        // (GetTime, currentindex) match C# field-for-field.
        let persisting_block = create_dummy_block(store_cache.data_cache(), settings.as_ref());

        let diagnostic_box = diagnostic
            .clone()
            .map(|diag| Box::new(diag) as Box<dyn neo_execution::diagnostic::Diagnostic>);

        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            tx_container,
            Arc::clone(&snapshot_cache),
            persisting_block,
            settings.as_ref().clone(),
            gas_limit,
            diagnostic_box,
        )
        .map_err(|err| CoreError::other(err.to_string()))?;

        engine
            .load_script(script.clone(), CallFlags::ALL, None)
            .map_err(|err| CoreError::other(err.to_string()))?;
        engine.execute_allow_fault();

        Ok(Self {
            script,
            snapshot: store_cache,
            engine: Mutex::new(engine),
            diagnostic: Mutex::new(diagnostic),
            iterators: Mutex::new(HashMap::new()),
            iterator_lookup: Mutex::new(HashMap::new()),
            start_time: Mutex::new(Instant::now()),
        })
    }

    /// Return the script executed by this session.
    pub fn script(&self) -> &[u8] {
        &self.script
    }

    /// Lock and return the session's application engine.
    pub fn engine(&self) -> MutexGuard<'_, ApplicationEngine> {
        self.engine.lock()
    }

    /// Lock and return the session's application engine for mutable use.
    pub fn engine_mut(&self) -> MutexGuard<'_, ApplicationEngine> {
        self.engine()
    }

    /// Return a clone of the diagnostic information captured during execution.
    pub fn diagnostic(&self) -> Option<Diagnostic> {
        self.diagnostic.lock().clone()
    }

    /// Return the storage snapshot associated with this session.
    pub const fn snapshot(&self) -> &StoreCache {
        &self.snapshot
    }

    /// Return whether this session currently retains any iterators.
    pub fn has_iterators(&self) -> bool {
        !self.iterators.lock().is_empty()
    }

    /// Register a VM iterator interface and return the stable RPC iterator id.
    ///
    /// Re-registering the same VM iterator returns its existing UUID.
    pub fn register_iterator_interface(
        &self,
        interface: &Arc<dyn VmInteropInterface>,
    ) -> Option<Uuid> {
        let iterator_interop = interface.as_any().downcast_ref::<IteratorInterop>()?;
        let iterator_id = iterator_interop.id();

        if let Some(existing) = self.iterator_lookup.lock().get(&iterator_id).copied() {
            return Some(existing);
        }

        let iterator = {
            let mut engine_guard = self.engine.lock();
            engine_guard.take_storage_iterator(iterator_id)?
        };

        let uuid = Uuid::new_v4();
        self.iterators.lock().insert(
            uuid,
            IteratorEntry {
                inner: Box::new(StorageSessionIterator::new(iterator)),
            },
        );
        self.iterator_lookup.lock().insert(iterator_id, uuid);

        Some(uuid)
    }

    /// Read up to `count` items from a previously registered iterator.
    pub fn traverse_iterator(
        &self,
        iterator_id: &Uuid,
        count: usize,
    ) -> Result<Vec<StackItem>, String> {
        let mut iterators = self.iterators.lock();
        let Some(entry) = iterators.get_mut(iterator_id) else {
            return Err("Unknown iterator".to_string());
        };

        let mut remaining = count;
        let mut values = Vec::new();
        while remaining > 0 && entry.next() {
            values.push(entry.value().map_err(|error| error.to_string())?);
            remaining -= 1;
        }
        Ok(values)
    }

    /// Reset the session expiration timer to the current instant.
    pub fn reset_expiration(&self) {
        let mut start_time = self.start_time.lock();
        *start_time = Instant::now();
    }

    /// Return whether the session has lived for at least `expiration`.
    pub fn is_expired(&self, expiration: Duration) -> bool {
        self.start_time.lock().elapsed() >= expiration
    }
}

/// Builds the dummy persisting block for a stateless RPC invoke, mirroring C#
/// `ApplicationEngine.CreateDummyBlock(IReadOnlyStore snapshot, ProtocolSettings
/// settings)`.
///
/// The block reads the current (last-persisted) block from the ledger and sets:
/// - `Version = 0`
/// - `PrevHash = LedgerContract.CurrentHash(snapshot)`
/// - `MerkleRoot = UInt256::default()` (C# `new UInt256()`)
/// - `Timestamp = currentBlock.Timestamp + GetTimePerBlock(snapshot, settings)`
///   where the per-block time is the Policy-aware `MillisecondsPerBlock`
///   (static setting pre-HF_Echidna, Policy storage value from HF_Echidna on)
/// - `Index = currentBlock.Index + 1`
/// - `NextConsensus = currentBlock.NextConsensus`
/// - `Witness = Witness.Empty`, `Transactions = []`
/// - `Nonce`/`PrimaryIndex` left at their zero defaults (C# does not set them).
///
/// Returns `None` (leaving the engine without a persisting block, as before)
/// when the ledger has no current block yet — e.g. a store without a persisted
/// genesis — matching the C# pre-genesis `KeyNotFoundException` corner where a
/// dummy block cannot be constructed.
fn create_dummy_block(
    snapshot: &neo_storage::persistence::DataCache,
    settings: &neo_config::ProtocolSettings,
) -> Option<Block> {
    let ledger = LedgerContract::new();
    let current_hash = ledger.current_hash(snapshot).ok()?;
    let current_block = ledger.get_trimmed_block(snapshot, &current_hash).ok()??;

    let milliseconds_per_block = PolicyContract::new()
        .get_milliseconds_per_block_snapshot(snapshot, settings)
        .unwrap_or(settings.milliseconds_per_block);

    let mut header = Header::new();
    header.set_version(0);
    header.set_prev_hash(current_hash);
    header.set_merkle_root(neo_primitives::UInt256::default());
    header.set_timestamp(
        current_block
            .header
            .timestamp()
            .saturating_add(u64::from(milliseconds_per_block)),
    );
    header.set_index(current_block.header.index().saturating_add(1));
    header.set_next_consensus(*current_block.header.next_consensus());
    header.witness = Witness::empty();

    Some(Block::from_parts(header, Vec::new()))
}

// THREAD SAFETY
//
// `ApplicationEngine` (and the underlying `ExecutionEngine`) is now `Send`
// because `HostPtr` implements `Send + Sync` with its safety invariants
// enforced at construction time. All mutable state in `Session` is guarded
// by `parking_lot::Mutex`, so `Session` is naturally `Send + Sync` without
// manual unsafe impls.

#[cfg(test)]
#[path = "../../tests/server/core/session.rs"]
mod tests;
