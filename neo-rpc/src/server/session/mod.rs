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
//! - `dummy_block`: C#-compatible dummy persisting block construction.
//! - `execution`: Session construction and initial script execution.
//! - `iterators`: RPC session iterator retention and disposal helpers.
//! - `tests`: Module-local tests and regression coverage.

mod dummy_block;
mod execution;
mod iterators;

use parking_lot::{Mutex, MutexGuard};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use neo_execution::ApplicationEngine;
use neo_storage::persistence::StoreCache;
use neo_vm::stack_item::{InteropInterface as VmInteropInterface, StackItem};
use uuid::Uuid;

use crate::server::diagnostic::Diagnostic;
use neo_execution::iterators::IteratorInterop;

use iterators::{IteratorEntry, StorageSessionIterator};

pub use iterators::SessionIterator;

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

impl Session {
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
