//! Application engine core implementation aligned with Neo C# version.
//! This module implements the Neo N3 smart contract execution engine, providing
//! the runtime environment for executing NeoVM scripts with blockchain context.
//!
//! # Architecture
//!
//! ```text
//! +-------------------------------------------------------------+
//! |                    ApplicationEngine                         |
//! |  +---------------------------------------------------------+|
//! |  |                   ExecutionEngine (VM)                   ||
//! |  |  +----------+  +----------+  +----------------------+  ||
//! |  |  | Script   |  | Stack    |  | Execution Contexts   |  ||
//! |  |  | Loader   |  | Manager  |  | (call stack)         |  ||
//! |  |  +----------+  +----------+  +----------------------+  ||
//! |  +---------------------------------------------------------+|
//! |  +---------------------------------------------------------+|
//! |  |                  Interop Services                        ||
//! |  |  +----------+  +----------+  +----------+  +---------+ ||
//! |  |  | Runtime  |  | Storage  |  | Crypto   |  | Contract| ||
//! |  |  | Interops |  | Interops |  | Interops |  | Interops| ||
//! |  |  +----------+  +----------+  +----------+  +---------+ ||
//! |  +---------------------------------------------------------+|
//! |  +---------------------------------------------------------+|
//! |  |                  Blockchain Context                      ||
//! |  |  +----------+  +----------+  +----------------------+  ||
//! |  |  | DataCache|  | Settings |  | Native Contracts     |  ||
//! |  |  | (state)  |  | (proto)  |  | (NEO, GAS, Policy)   |  ||
//! |  |  +----------+  +----------+  +----------------------+  ||
//! |  +---------------------------------------------------------+|
//! +-------------------------------------------------------------+
//! ```
//!
//! # Key Components
//!
//! - [`ApplicationEngine`]: Main execution engine wrapping the NeoVM
//! - [`TriggerType`]: Execution trigger (OnPersist, Application, PostPersist, Verification)
//! - [`CallFlags`]: Permission flags for contract calls
//! - [`NotifyEventArgs`]: Smart contract notification events
//! - [`LogEventArgs`]: Smart contract log events
//!
//! # Interop Services
//!
//! The engine provides system call (interop) services organized by category:
//! - **Runtime**: Block/transaction info, notifications, logging, gas management
//! - **Storage**: Contract storage read/write/delete/find operations
//! - **Crypto**: Hash functions, signature verification
//! - **Contract**: Contract deployment, updates, calls, native contract access
//! - **Iterator**: Storage iterator traversal
//!
//! # Execution Flow
//!
//! 1. Create engine with trigger type and blockchain snapshot
//! 2. Load script and set entry point
//! 3. Execute until completion or fault
//! 4. Collect notifications, logs, and gas consumption
//! 5. Commit or rollback state changes based on result
//!
//! # Gas Metering
//!
//! All operations consume GAS based on computational cost. The engine tracks:
//! - `gas_consumed`: Total GAS used during execution
//! - `fee_per_byte`: Network fee per transaction byte
//! - Execution limits prevent infinite loops and resource exhaustion
//!
//! The `ApplicationEngine` implementation is split across multiple files in this
//! directory to keep individual methods readable while preserving a single Rust
//! module boundary (matching the C# layout).
pub mod contract;
pub mod crypto;
pub mod helper;
pub mod iterator;
pub mod op_code_prices;
pub mod runtime;
pub mod storage;
use crate::error::{CoreError as Error, Result};
use neo_crypto::{Crypto, murmur128};
use neo_primitives::constants::HASH_SIZE;
use neo_protocol::Hardfork;
use neo_protocol::block::Block;
use neo_vm::evaluation_stack::EvaluationStack;
use neo_vm::execution_context::ExecutionContext;
use neo_vm::interop_service::InteropHost;
use neo_vm::jump_table::JumpTable;
use neo_vm::script::Script;
// InteropInterface trait removed - StackValue::Interop(u64) is used instead
// VerifiableInterop is now stored via the interop host registry, not inline in the VM stack
use self::contract::register_contract_interops;
use self::crypto::register_crypto_interops;
use self::iterator::register_iterator_interops;
use self::runtime::register_runtime_interops;
use self::storage::register_storage_interops;
use crate::contract_state::ContractState;
use crate::diagnostic::Diagnostic;
use crate::execution_context_state::ExecutionContextState;
use crate::helper::Helper;
use crate::iterators::StorageIterator;
use crate::iterators::iterator::StorageIterator as _;
use crate::manifest::ContractMethodDescriptor;
pub use execution_state::{LedgerTransactionStates, PersistedTransactionState};
pub use hardfork_activable::HardforkActivable;
pub use native_contract::{
    NativeContract, NativeContractsCache, NativeContractsCacheEntry, NativeMethod,
};
pub use native_services::{
    EngineEventObserver, EngineNativeServices, NativeChainQueries, NativeRegistry,
    NativeStateReader, WitnessHashResolver,
};
use neo_primitives::Verifiable;
use neo_primitives::{CallFlags, ContractParameterType, FindOptions, LogEventArgs, TriggerType};
use neo_primitives::{UInt160, UInt256};
use neo_protocol::ProtocolSettings;
use neo_protocol::transaction::Transaction;
use neo_protocol::transaction_attribute::TransactionAttribute;
use neo_protocol::witness_condition::WitnessCondition;
use neo_protocol::witness_rule_action::WitnessRuleAction;
use neo_storage::SeekDirection;
use neo_storage::persistence::DataCache;
use neo_storage::{StorageItem, StorageKey};
use neo_vm::ExecutionEngineLimits;
use neo_vm::Instruction;
use neo_vm::OpCode;
use neo_vm::StackValue as VmStackValue;
use neo_vm::SyscallProvider;
use neo_vm::VmState as VMState;
use neo_vm::interpret_with_stack_and_syscalls_at;
use neo_vm::interpret_with_stack_and_syscalls_at_with_result_limit;
use neo_vm::{ExecutionEngine, StackItem, VmError, VmResult};
use neo_vm::{NotifyEventArgs, StorageContext};
use num_traits::ToPrimitive;
use parking_lot::Mutex;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;
/// Maximum GAS (in datoshi) available to an engine created for testing/invoke (C# `TestModeGas`).
pub const TEST_MODE_GAS: i64 = 20_000_000_000;
/// Maximum length (in bytes) of an event name.
pub const MAX_EVENT_NAME: usize = 32;
/// Maximum serialized size (in bytes) of a single notification.
pub const MAX_NOTIFICATION_SIZE: usize = 1024;
/// Maximum number of notifications a single execution may emit (C# `MaxNotificationCounter`).
pub const MAX_NOTIFICATION_COUNT: usize = 512;
/// Fee units charged to verify one signature (C# `CheckSigPrice`).
pub const CHECK_SIG_PRICE: i64 = 1 << 15;
/// Scaling factor between raw internal fee counters and datoshi GAS amounts.
pub const FEE_FACTOR: i64 = 10000;
type InteropHandler = fn(&mut ApplicationEngine, &mut ExecutionEngine) -> VmResult<()>;
type StdResult<T> = std::result::Result<T, String>;
#[derive(Clone, Copy)]
struct HostInteropHandler {
    price: i64,
    required_call_flags: CallFlags,
    handler: InteropHandler,
}
fn map_core_error_to_vm_error(error: Error) -> VmError {
    match error {
        Error::InsufficientGas {
            required,
            available,
        } => VmError::gas_exhausted(required, available),
        other => VmError::invalid_operation_msg(other.to_string()),
    }
}
struct VmEngineHost {
    engine: ExecutionEngine,
}
impl VmEngineHost {
    fn new(engine: ExecutionEngine) -> Self {
        Self { engine }
    }
    fn engine(&self) -> &ExecutionEngine {
        &self.engine
    }
    fn engine_mut(&mut self) -> &mut ExecutionEngine {
        &mut self.engine
    }
    fn current_context(&self) -> Option<&ExecutionContext> {
        self.engine.current_context()
    }
}
/// Represents a contract call queued by a native contract.
///
/// Native contracts sometimes need to invoke a user contract (e.g., NEP-17
/// `onNEP17Payment`) but cannot safely switch the current VM context while the
/// native syscall is still executing. Instead, we queue the call and let the
/// `System.Contract.CallNative` handler load it after the native method has
/// returned its value.
#[derive(Clone, Debug)]
struct PendingNativeCall {
    calling_script_hash: UInt160,
    contract_hash: UInt160,
    method: String,
    args: Vec<StackItem>,
}
/// The Neo N3 application engine: executes smart contracts on the VM,
/// charging GAS for syscalls and collecting notifications and logs.
pub struct ApplicationEngine {
    trigger: TriggerType,
    script_container: Option<Arc<dyn Verifiable>>,
    persisting_block: Option<Arc<Block>>,
    protocol_settings: ProtocolSettings,
    gas_limit: i64,
    gas_consumed: i64,
    fee_amount: i64,
    fee_consumed: i64,
    exec_fee_factor: u32,
    /// `ExecutionContext::context_id` whose whitelist flag is cached. Zero means
    /// the cache is empty. Context ids start at 1 and are never reused.
    fee_whitelist_context: u64,
    fee_whitelist_flag: bool,
    storage_price: u32,
    call_flags: CallFlags,
    vm_engine: VmEngineHost,
    interop_handlers: HashMap<u32, HostInteropHandler>,
    snapshot_cache: Arc<DataCache>,
    original_snapshot_cache: Arc<DataCache>,
    notifications: Vec<NotifyEventArgs>,
    logs: Vec<LogEventArgs>,
    native_registry: NativeRegistry,
    native_state_reader: Arc<dyn NativeStateReader>,
    witness_hash_resolver: Arc<dyn WitnessHashResolver>,
    chain_queries: Option<Arc<dyn NativeChainQueries>>,
    policy_hash: UInt160,
    native_contract_cache: Arc<Mutex<NativeContractsCache>>,
    /// Contract states are shared via `Arc` so the per-transaction engines in a
    /// block can clone the native-contract map without copying NEF scripts and
    /// manifests. Mutations replace the `Arc` in that engine's map only.
    contracts: HashMap<UInt160, Arc<ContractState>>,
    storage_iterators: HashMap<u32, StorageIterator>,
    next_iterator_id: u32,
    current_script_hash: Option<UInt160>,
    calling_script_hash: Option<UInt160>,
    /// Explicitly set calling script hash override that persists across
    /// `refresh_context_tracking` when no execution context exists.
    native_calling_override: Option<UInt160>,
    entry_script_hash: Option<UInt160>,
    invocation_counter: HashMap<UInt160, u32>,
    pending_native_calls: Vec<PendingNativeCall>,
    nonce_data: [u8; 16],
    random_times: u32,
    diagnostic: Option<Box<dyn Diagnostic>>,
    fault_exception: Option<String>,
    states: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
    event_observer: Option<Arc<dyn EngineEventObserver>>,
}
mod contracts;
mod drop;
pub mod execution_state;
mod external_vm;
mod fees_events_native;
mod hardfork_activable;
mod interop_host;
mod load_execute_storage;
pub mod native_contract;
mod native_contract_cache;
pub mod native_interop;
pub mod native_services;
mod state;
mod storage_low_level;
mod witness_and_misc;
