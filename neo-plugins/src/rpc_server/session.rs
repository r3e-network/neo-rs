//! RPC invocation sessions mirroring `Neo.Plugins.RpcServer.Session`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use neo_core::neo_system::NeoSystem;
use neo_core::network::p2p::payloads::signer::Signer;
use neo_core::network::p2p::payloads::transaction::Transaction;
use neo_core::network::p2p::payloads::transaction_attribute::TransactionAttribute;
use neo_core::network::p2p::payloads::witness::Witness;
use neo_core::persistence::store_cache::StoreCache;
use neo_core::smart_contract::call_flags::CallFlags;
use neo_core::smart_contract::native::ledger_contract::LedgerContract;
use neo_core::smart_contract::trigger_type::TriggerType;
use neo_core::smart_contract::ApplicationEngine;
use neo_core::IVerifiable;
use neo_vm::stack_item::InteropInterface as VmInteropInterface;
use neo_vm::stack_item::StackItem;
use rand::random;
use uuid::Uuid;

use crate::rpc_server::diagnostic::Diagnostic;

/// Trait representing an iterator stored within an RPC session.
pub trait SessionIterator: Send {
    fn next(&mut self) -> bool;
    fn value(&self) -> StackItem;
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

    fn value(&self) -> StackItem {
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
    engine: ApplicationEngine,
    snapshot: StoreCache,
    diagnostic: Option<Diagnostic>,
    iterators: HashMap<Uuid, IteratorEntry>,
    start_time: Instant,
}

impl Session {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        system: Arc<NeoSystem>,
        script: Vec<u8>,
        signers: Option<Vec<Signer>>,
        witnesses: Option<Vec<Witness>>,
        gas_limit: i64,
        diagnostic: Option<Diagnostic>,
    ) -> Result<Self, String> {
        let store_cache = system.store_cache();
        let snapshot_cache = Arc::new(store_cache.data_cache().clone());

        let tx_container = signers.as_ref().map(|signer_list| {
            let mut tx = Transaction::new();
            tx.set_version(0);
            tx.set_nonce(random());
            let valid_until = LedgerContract::new()
                .current_index(&store_cache)
                .unwrap_or(0)
                .saturating_add(system.settings().max_valid_until_block_increment);
            tx.set_valid_until_block(valid_until);
            tx.set_signers(signer_list.clone());
            tx.set_attributes(Vec::<TransactionAttribute>::new());
            tx.set_script(script.clone());
            if let Some(ws) = &witnesses {
                tx.set_witnesses(ws.clone());
            } else {
                tx.set_witnesses(vec![Witness::new(); signer_list.len()]);
            }
            Arc::new(tx) as Arc<dyn IVerifiable>
        });

        let diagnostic_box = diagnostic.as_ref().cloned().map(|diag| {
            Box::new(diag) as Box<dyn neo_core::smart_contract::i_diagnostic::IDiagnostic>
        });

        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            tx_container,
            Arc::clone(&snapshot_cache),
            None,
            system.settings().clone(),
            gas_limit,
            diagnostic_box,
        )
        .map_err(|err| err.to_string())?;

        engine
            .load_script(script.clone(), CallFlags::ALL, None)
            .map_err(|err| err.to_string())?;
        engine.execute().map_err(|err| err.to_string())?;

        Ok(Self {
            script,
            engine,
            snapshot: store_cache,
            diagnostic,
            iterators: HashMap::new(),
            start_time: Instant::now(),
        })
    }

    pub fn script(&self) -> &[u8] {
        &self.script
    }

    pub fn engine(&self) -> &ApplicationEngine {
        &self.engine
    }

    pub fn engine_mut(&mut self) -> &mut ApplicationEngine {
        &mut self.engine
    }

    pub fn diagnostic(&self) -> Option<&Diagnostic> {
        self.diagnostic.as_ref()
    }

    pub fn snapshot(&self) -> &StoreCache {
        &self.snapshot
    }

    pub fn has_iterators(&self) -> bool {
        !self.iterators.is_empty()
    }

    pub fn register_iterator_interface(
        &mut self,
        _interface: &Arc<dyn VmInteropInterface>,
    ) -> Option<Uuid> {
        // Iterator interop support not yet implemented in the VM. This method will
        // wire up iterator handles once available.
        None
    }

    pub fn traverse_iterator(
        &mut self,
        iterator_id: &Uuid,
        count: usize,
    ) -> Result<Vec<StackItem>, String> {
        let Some(entry) = self.iterators.get_mut(iterator_id) else {
            return Err("Unknown iterator".to_string());
        };

        let mut remaining = count;
        let mut values = Vec::new();
        while remaining > 0 && entry.next() {
            values.push(entry.value());
            remaining -= 1;
        }
        Ok(values)
    }

    pub fn reset_expiration(&mut self) {
        self.start_time = Instant::now();
    }

    pub fn is_expired(&self, expiration: Duration) -> bool {
        self.start_time.elapsed() >= expiration
    }
}

// SAFETY DOCUMENTATION FOR THREAD MARKER TRAITS
//
// # Why these unsafe impls exist
//
// `Session` contains `ApplicationEngine` which contains `VmEngineHost` which wraps
// `ExecutionEngine`. The `ExecutionEngine` in neo-vm contains a raw pointer
// (`*mut dyn InteropHost`) that is explicitly documented as NOT thread-safe:
//
// > "Thread Safety: The ExecutionEngine is not Send or Sync due to this raw pointer.
// >  Do not share across threads." (neo-vm/src/execution_engine.rs:121-122)
//
// These unsafe impls are required because `Session` is stored in
// `Arc<RwLock<HashMap<Uuid, Session>>>` in `RpcServer`, which requires `Send + Sync`.
//
// # Invariants that MUST be maintained
//
// 1. **Exclusive Access**: Sessions MUST only be accessed with exclusive (write) locks.
//    Using `RwLock::read()` to access sessions concurrently would cause data races.
//    All session access in the RPC server MUST use `write()` locks.
//
// 2. **Single-Threaded Mutation**: A session's `ApplicationEngine` must never be
//    mutated from multiple threads simultaneously. The `RwLock` write lock ensures this.
//
// 3. **No Concurrent Reads**: Even read-only access to `Session` is unsafe if done
//    concurrently, because `ExecutionEngine` may have interior mutability through
//    the raw pointer.
//
// # Known Risks
//
// - If code is added that uses `sessions.read()` instead of `sessions.write()`,
//   it could cause undefined behavior through data races.
// - The `ExecutionEngine`'s raw pointer could become dangling if lifetimes are
//   not properly managed.
//
// # Recommended Future Fix
//
// Consider one of these safer alternatives:
// 1. [IMPLEMENTED] Use `Arc<Mutex<HashMap<Uuid, Session>>>` instead of `RwLock`
//    to prevent accidental concurrent reads. (Changed in security audit 2025-12-09)
// 2. Use a channel-based approach where all session operations are serialized
//    to a single worker thread.
// 3. Refactor `ExecutionEngine` to use safe abstractions instead of raw pointers.
//
// # Security Audit Note (2025-12-09)
//
// This is a HIGH severity issue. The unsafe impls violate the documented thread
// safety requirements of `ExecutionEngine`. While the current code appears to use
// exclusive access patterns, this is not enforced by the type system and could
// easily be broken by future changes.
unsafe impl Send for Session {}
unsafe impl Sync for Session {}
