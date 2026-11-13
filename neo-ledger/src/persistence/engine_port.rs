//! Engine ports consumed by the ledger persistence operation.
//!
//! The ledger never constructs a concrete `ApplicationEngine`. It drives execution
//! through three inverted ports:
//!
//! * [`EngineFactory`] — opens the native and application engine ports over a supplied
//!   snapshot (steps 6, 10, 11). The `neo-system`/`neo-smart-contract` adapter supplies
//!   the concrete implementation, injecting the engine-owned native services
//!   (`EngineNativeServices`, `NativeStateReader`, `WitnessHashResolver`, ordered
//!   `NativeRegistry`) established by task-88.
//! * [`NativeEnginePort`] — the `OnPersist`/`PostPersist` native execution surface.
//! * [`ApplicationEnginePort`] — the per-transaction application execution surface.
//!
//! # Dependency direction (task-102 L.1 decision A)
//!
//! `neo-ledger` holds a *type-only* edge to `neo-smart-contract` for the canonical
//! state token [`LedgerTransactionStates`] and the shared native seed handles
//! ([`SeededNativeContracts`], [`NativeCacheHandle`]). There is no `ledger → core /
//! system / p2p / mempool / native` reverse edge: the ledger names no concrete
//! `NativeContract`, builds no native registry, and constructs no `ApplicationEngine`.
//!
//! # Alignment (handoff: task-96 / task-89)
//!
//! The exact `neo-smart-contract` paths for `NativeContractsCache`, `ContractState` and
//! the `parking_lot::Mutex` cache handle are owned by task-96's in-flight engine
//! migration. The type names below follow the task-102 contract; if task-96 exposes a
//! different final path, these three `use` lines are the single reconciliation point and
//! must be aligned by 89/96 without changing the port method shapes.

use super::error::LedgerError;
use super::receipt::ExecutionReceipt;
use neo_primitives::{UInt160, UInt256, Verifiable};
use neo_protocol::execution::Block;
use neo_smart_contract::application_engine::{LedgerTransactionStates, NativeContractsCache};
use neo_smart_contract::contract_state::ContractState;
use neo_storage::persistence::DataCache;
use neo_vm::VmState;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

/// Native contract states seeded from the `OnPersist` engine into child engines
/// (step 8). Contains *only* native contract hashes; the filtering that the legacy core
/// performed with `retain(native_hashes)` is the port implementation's responsibility.
pub type SeededNativeContracts = HashMap<UInt160, Arc<ContractState>>;

/// Opaque handle to the shared native-contracts cache threaded from the `OnPersist`
/// engine into every child engine (step 8).
pub type NativeCacheHandle = Arc<Mutex<NativeContractsCache>>;

/// `OnPersist`/`PostPersist` native execution port (steps 6, 11).
///
/// The port owns the engine's native services and the `LedgerTransactionStates` token;
/// the ledger only sequences the canonical calls and projects an [`ExecutionReceipt`].
pub trait NativeEnginePort: Send {
    /// Runs the `OnPersist` native hooks (step 6).
    fn native_on_persist(&mut self) -> Result<(), LedgerError>;

    /// Runs the `PostPersist` native hooks (step 11).
    fn native_post_persist(&mut self) -> Result<(), LedgerError>;

    /// Records that the native persist hooks completed without fault (steps 6, 11).
    fn record_native_persist_success(&mut self);

    /// Installs the carried ledger transaction state set (step 11).
    fn set_transaction_states(&mut self, states: LedgerTransactionStates);

    /// Takes the ledger transaction state set, defaulting to empty when absent
    /// (steps 9, 11).
    fn take_transaction_states(&mut self) -> LedgerTransactionStates;

    /// Returns the native-only seeded contract states for child engines (step 8).
    fn seed_native_contracts(&self) -> SeededNativeContracts;

    /// Returns the shared native-contracts cache handle for child engines (step 8).
    fn native_cache_handle(&self) -> NativeCacheHandle;

    /// Projects the pure execution receipt for this native engine (steps 7, 11).
    fn execution_receipt(&mut self) -> ExecutionReceipt;
}

/// Per-transaction application execution port (step 10).
pub trait ApplicationEnginePort: Send {
    /// Loads the transaction script with `CallFlags::ALL` (step 10).
    fn load_script(&mut self, script: Vec<u8>) -> Result<(), LedgerError>;

    /// Executes the loaded script, tolerating a VM fault (step 10).
    fn execute_allow_fault(&mut self) -> VmState;

    /// Checks any optional dual-execution verification before overlay merge.
    /// Implementations without a verification engine retain the canonical no-op.
    fn verify_execution(&self) -> Result<(), LedgerError> {
        Ok(())
    }

    /// Records the transaction's VM state unconditionally, even when detailed receipts
    /// are suppressed during fast-sync/import (step 10).
    fn record_transaction_vm_state(&mut self, transaction_hash: &UInt256, vm_state: VmState);

    /// Installs the carried ledger transaction state set (step 10).
    fn set_transaction_states(&mut self, states: LedgerTransactionStates);

    /// Takes the ledger transaction state set, defaulting to empty when absent
    /// (step 10).
    fn take_transaction_states(&mut self) -> LedgerTransactionStates;

    /// GAS/fee consumed by the execution (step 10b evidence staging).
    fn fee_consumed(&self) -> i64;

    /// Captured fault exception message, if any (step 10b classification).
    fn fault_exception(&self) -> Option<String>;

    /// Projects the pure execution receipt for this transaction engine (step 10).
    fn execution_receipt(&mut self) -> ExecutionReceipt;
}

/// Opens the native and application engine ports over a supplied snapshot.
///
/// The three openers map one-to-one onto the canonical engine constructors used by the
/// legacy pipeline: `OnPersist` (unseeded), `Application` (seeded, per-transaction) and
/// `PostPersist` (seeded). The concrete implementation injects the task-88 engine-owned
/// native services and consumes them last, preserving the existing constructor ordering.
pub trait EngineFactory: Send + Sync {
    /// Opens the `OnPersist` native engine over the base snapshot (step 6).
    fn open_on_persist(
        &self,
        snapshot: Arc<DataCache>,
        block: Arc<Block>,
    ) -> Result<Box<dyn NativeEnginePort>, LedgerError>;

    /// Opens a per-transaction application engine over the shared child overlay
    /// (step 10).
    fn open_application(
        &self,
        container: Arc<dyn Verifiable>,
        snapshot: Arc<DataCache>,
        block: Arc<Block>,
        system_fee: i64,
        seeded: &SeededNativeContracts,
        cache: &NativeCacheHandle,
    ) -> Result<Box<dyn ApplicationEnginePort>, LedgerError>;

    /// Opens the `PostPersist` native engine over the merged base snapshot (step 11).
    fn open_post_persist(
        &self,
        snapshot: Arc<DataCache>,
        block: Arc<Block>,
        seeded: &SeededNativeContracts,
        cache: &NativeCacheHandle,
    ) -> Result<Box<dyn NativeEnginePort>, LedgerError>;
}
