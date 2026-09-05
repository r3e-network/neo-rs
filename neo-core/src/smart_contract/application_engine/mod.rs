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

use crate::cryptography::{Crypto, murmur128};
use crate::error::{CoreError as Error, Result};
use crate::hardfork::Hardfork;
use crate::ledger::Block;
use crate::neo_config::HASH_SIZE;
use crate::neo_vm::evaluation_stack::EvaluationStack;
use crate::neo_vm::execution_context::ExecutionContext;
use crate::neo_vm::interop_service::InteropHost;
use crate::neo_vm::jump_table::JumpTable;
use crate::neo_vm::script::Script;
// InteropInterface trait removed - StackValue::Interop(u64) is used instead
// VerifiableInterop is now stored via the interop host registry, not inline in the VM stack
use crate::Verifiable;
use crate::neo_vm::{ExecutionEngine, StackItem, VmError, VmResult};
use crate::network::p2p::payloads::{Transaction, TransactionAttribute};
use crate::persistence::data_cache::DataCache;
use crate::persistence::seek_direction::SeekDirection;
use crate::protocol_settings::ProtocolSettings;
use crate::services::SystemContext;
use crate::smart_contract::CallFlags;
use crate::smart_contract::ContractParameterType;
use crate::smart_contract::FindOptions;
use crate::smart_contract::LogEventArgs;
use crate::smart_contract::NotifyEventArgs;
use crate::smart_contract::StorageContext;
use crate::smart_contract::StorageItem;
use crate::smart_contract::StorageKey;
use crate::smart_contract::TriggerType;
use crate::smart_contract::application_engine_contract::register_contract_interops;
use crate::smart_contract::application_engine_crypto::register_crypto_interops;
use crate::smart_contract::application_engine_iterator::register_iterator_interops;
use crate::smart_contract::application_engine_runtime::register_runtime_interops;
use crate::smart_contract::application_engine_storage::register_storage_interops;
use crate::smart_contract::contract_state::ContractState;
use crate::smart_contract::diagnostic::Diagnostic;
use crate::smart_contract::execution_context_state::ExecutionContextState;
use crate::smart_contract::helper::Helper;
use crate::smart_contract::iterators::StorageIterator;
use crate::smart_contract::iterators::iterator::StorageIterator as _;
use crate::smart_contract::manifest::ContractMethodDescriptor;
use crate::smart_contract::native::ContractManagement;
use crate::smart_contract::native::{
    LedgerContract, LedgerTransactionStates, NativeContract, NativeContractsCache, NativeRegistry,
    PolicyContract,
};
use crate::{UInt160, UInt256, WitnessCondition, WitnessRuleAction};
use neo_vm::ExecutionEngineLimits;
use neo_vm::Instruction;
use neo_vm::OpCode;
use neo_vm::StackValue as VmStackValue;
use neo_vm::SyscallProvider;
use neo_vm::VmState as VMState;
use neo_vm::interpret_with_stack_and_syscalls_at;
use neo_vm::interpret_with_stack_and_syscalls_at_with_result_limit;
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
    storage_price: u32,
    call_flags: CallFlags,
    vm_engine: VmEngineHost,
    interop_handlers: HashMap<u32, HostInteropHandler>,
    snapshot_cache: Arc<DataCache>,
    original_snapshot_cache: Arc<DataCache>,
    notifications: Vec<NotifyEventArgs>,
    logs: Vec<LogEventArgs>,
    native_registry: NativeRegistry,
    native_contract_cache: Arc<Mutex<NativeContractsCache>>,
    contracts: HashMap<UInt160, ContractState>,
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
    runtime_context: Option<Arc<dyn SystemContext>>,
}

mod contracts;
mod drop;
mod external_vm;
mod fees_events_native;
mod interop_host;
mod load_execute_storage;
mod state;
mod storage_low_level;
mod witness_and_misc;
