//! Application engine core implementation aligned with Neo C# version.
//!
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

use neo_crypto::{Crypto, murmur128};
use neo_error::{CoreError as Error, Result};
use neo_config::hardfork::Hardfork;
use neo_payloads::Block;
use neo_primitives::constants::HASH_SIZE;
use neo_vm::evaluation_stack::EvaluationStack;
use neo_vm::execution_context::ExecutionContext;
use neo_vm::interop_service::InteropHost;
use neo_vm::jump_table::JumpTable;
use neo_vm::script::Script;
// InteropInterface trait removed - StackValue::Interop(u64) is used instead
// VerifiableInterop is now stored via the interop host registry, not inline in the VM stack
use neo_vm::{ExecutionEngine, StackItem, VmError, VmResult};
use neo_payloads::{Transaction, TransactionAttribute};
use neo_data_cache::DataCache;
use neo_storage::SeekDirection;
use neo_config::ProtocolSettings;
// use neo_services::SystemContext; // will be replaced with trait object
use crate::application_engine_contract::register_contract_interops;
use crate::application_engine_crypto::register_crypto_interops;
use crate::application_engine_iterator::register_iterator_interops;
use crate::application_engine_runtime::register_runtime_interops;
use crate::application_engine_storage::register_storage_interops;
use neo_manifest::CallFlags;
use neo_primitives::ContractParameterType;
use crate::contract_state::ContractState;
use crate::execution_context_state::ExecutionContextState;
use neo_primitives::FindOptions;
use crate::helper::Helper;
use crate::diagnostic::Diagnostic;
use crate::iterators::iterator::StorageIterator as _;
use crate::iterators::StorageIterator;
use neo_primitives::LogEventArgs;
use neo_manifest::ContractMethodDescriptor;
// ContractManagement is now accessed via the native contract provider
// LedgerContract and PolicyContract are now accessed via the native contract provider

use crate::{NativeContract, NativeContractsCache, NativeRegistry};
use crate::NotifyEventArgs;
use crate::StorageContext;
use neo_storage::StorageItem;
use neo_storage::StorageKey;
use neo_primitives::TriggerType;
use neo_primitives::Verifiable;
use neo_p2p::WitnessCondition;
use neo_primitives::{UInt160, UInt256};
use neo_primitives::WitnessRuleAction;
use neo_vm_rs::interpret_with_stack_and_syscalls_at;
use neo_vm_rs::interpret_with_stack_and_syscalls_at_with_result_limit;
use neo_vm_rs::ExecutionEngineLimits;
use neo_vm_rs::Instruction;
use neo_vm_rs::OpCode;
use neo_vm_rs::StackValue as VmStackValue;
use neo_vm_rs::SyscallProvider;
use neo_vm_rs::VmState as VMState;
use num_traits::ToPrimitive;
use parking_lot::Mutex;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

pub const TEST_MODE_GAS: i64 = 20_000_000_000;
pub const MAX_EVENT_NAME: usize = 32;
pub const MAX_NOTIFICATION_SIZE: usize = 1024;
pub const MAX_NOTIFICATION_COUNT: usize = 512;
pub const CHECK_SIG_PRICE: i64 = 1 << 15;
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

#[derive(Clone)]
struct VerifiableInterop {
    container: Arc<dyn Verifiable>,
}

impl VerifiableInterop {
    fn new(container: Arc<dyn Verifiable>) -> Self {
        Self { container }
    }
}

impl fmt::Debug for VerifiableInterop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VerifiableInterop")
    }
}

impl VerifiableInterop {
    fn interface_type(&self) -> &str {
        "Verifiable"
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
    runtime_context: Option<Arc<dyn std::any::Any + Send + Sync>>, // SystemContext trait object
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
