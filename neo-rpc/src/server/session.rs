//! RPC invocation sessions mirroring `Neo.Plugins.RpcServer.Session`.

use parking_lot::{Mutex, MutexGuard};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use neo_core::IVerifiable;
use neo_core::neo_system::NeoSystem;
use neo_core::network::p2p::payloads::signer::Signer;
use neo_core::network::p2p::payloads::transaction::Transaction;
use neo_core::network::p2p::payloads::transaction_attribute::TransactionAttribute;
use neo_core::network::p2p::payloads::witness::Witness;
use neo_core::persistence::store_cache::StoreCache;
use neo_core::smart_contract::ApplicationEngine;
use neo_core::smart_contract::call_flags::CallFlags;
use neo_core::smart_contract::iterators::{IIterator, IteratorInterop, StorageIterator};
use neo_core::smart_contract::native::ledger_contract::LedgerContract;
use neo_core::smart_contract::trigger_type::TriggerType;
use neo_vm::stack_item::InteropInterface as VmInteropInterface;
use neo_vm::stack_item::StackItem;
use rand::random;
use uuid::Uuid;

use crate::server::diagnostic::Diagnostic;

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
        IIterator::next(&mut self.iterator)
    }

    fn value(&self) -> StackItem {
        self.iterator.value()
    }

    fn dispose(&mut self) {
        IIterator::dispose(&mut self.iterator);
    }
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
                .saturating_add(system.max_valid_until_block_increment());
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

        let diagnostic_box = diagnostic.clone().map(|diag| {
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

    pub fn script(&self) -> &[u8] {
        &self.script
    }

    pub fn engine(&self) -> MutexGuard<'_, ApplicationEngine> {
        self.engine.lock()
    }

    pub fn engine_mut(&self) -> MutexGuard<'_, ApplicationEngine> {
        self.engine()
    }

    pub fn diagnostic(&self) -> Option<Diagnostic> {
        self.diagnostic.lock().clone()
    }

    pub const fn snapshot(&self) -> &StoreCache {
        &self.snapshot
    }

    pub fn has_iterators(&self) -> bool {
        !self.iterators.lock().is_empty()
    }

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
            values.push(entry.value());
            remaining -= 1;
        }
        Ok(values)
    }

    pub fn reset_expiration(&self) {
        let mut start_time = self.start_time.lock();
        *start_time = Instant::now();
    }

    pub fn is_expired(&self, expiration: Duration) -> bool {
        self.start_time.lock().elapsed() >= expiration
    }
}

// THREAD SAFETY
//
// `ApplicationEngine` (and the underlying `ExecutionEngine`) is now `Send`
// because `HostPtr` implements `Send + Sync` with its safety invariants
// enforced at construction time. All mutable state in `Session` is guarded
// by `parking_lot::Mutex`, so `Session` is naturally `Send + Sync` without
// manual unsafe impls.

#[cfg(test)]
mod tests {
    use super::*;
    use neo_core::persistence::{StorageItem, StorageKey};
    use neo_core::smart_contract::find_options::FindOptions;
    use neo_core::{NeoSystem, ProtocolSettings};
    use neo_vm::op_code::OpCode;

    #[tokio::test(flavor = "multi_thread")]
    async fn session_registers_and_traverses_storage_iterator() {
        let settings = ProtocolSettings::default();
        let system = NeoSystem::new(settings, None, None).expect("system");
        let session = Session::new(
            system,
            vec![OpCode::RET as u8],
            None,
            None,
            100_000_000,
            None,
        )
        .expect("session");

        let entries = vec![
            (
                StorageKey::new(1, vec![0x01]),
                StorageItem::from_bytes(vec![0xAA]),
            ),
            (
                StorageKey::new(1, vec![0x02]),
                StorageItem::from_bytes(vec![0xBB]),
            ),
        ];
        let iterator = StorageIterator::new(entries, 0, FindOptions::None);
        let iterator_id = {
            let mut engine = session.engine_mut();
            engine
                .store_storage_iterator(iterator)
                .expect("store iterator")
        };

        let interop = Arc::new(IteratorInterop::new(iterator_id)) as Arc<dyn VmInteropInterface>;
        let uuid_first = session
            .register_iterator_interface(&interop)
            .expect("iterator registered");
        let uuid_second = session
            .register_iterator_interface(&interop)
            .expect("iterator re-registered");
        assert_eq!(uuid_first, uuid_second);
        assert!(session.has_iterators());

        let values = session
            .traverse_iterator(&uuid_first, 10)
            .expect("traverse iterator");
        assert_eq!(values.len(), 2);
        assert!(matches!(values[0], StackItem::Struct(_)));

        let tail = session
            .traverse_iterator(&uuid_first, 10)
            .expect("traverse iterator exhausted");
        assert!(tail.is_empty());
    }
}
