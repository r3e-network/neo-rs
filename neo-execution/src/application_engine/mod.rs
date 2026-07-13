//! # neo-execution::application_engine
//!
//! ApplicationEngine interop groups and execution-facing syscall handlers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-execution`. This execution crate owns VM/native
//! interop behavior and must not own durable storage engines, P2P sync, or
//! application startup.
//!
//! ## Contents
//!
//! - `contracts`: Contract metadata, manifests, deployed-state records, and
//!   contract parameter types.
//! - `drop`: VM drop-stack opcode handlers.
//! - `external_vm`: external VM execution bridge.
//! - `fees_events_native`: fee, event, and native-contract syscall handlers.
//! - `host_state`: host-side VM wrapper, syscall metadata, and queued native calls.
//! - `interop_host`: ApplicationEngine interop host.
//! - `load_execute_storage`: load, execute, and storage syscall grouping.
//! - `state`: domain state records for the surrounding workflow.
//! - `storage_low_level`: low-level storage syscall handlers.
//! - `witness_and_misc`: witness and miscellaneous syscall handlers.

use crate::execution_context_state::{
    ApplicationExecutionContext as ExecutionContext, ApplicationExecutionEngine as ExecutionEngine,
    ApplicationJumpTable as JumpTable,
};
use neo_config::hardfork::Hardfork;
use neo_crypto::{Crypto, ECCurve, ECPoint, murmur};
use neo_error::{CoreError, CoreResult};
use neo_payloads::Block;
use neo_primitives::constants::HASH_SIZE;
use neo_vm::evaluation_stack::EvaluationStack;
use neo_vm::interop_service::InteropHost;
use neo_vm::script::Script;
// InteropInterface trait removed - StackValue::Interop(u64) is used instead
// Verifiable script containers are passed via the interop host registry, not inline in the VM stack
use crate::contract_state::ContractState;
use crate::diagnostic::{Diagnostic, NoDiagnostic};
use crate::execution_context_state::ExecutionContextState;
use crate::helper::Helper;
use crate::iterators::StorageIterator;
use crate::iterators::iterator::StorageIterator as _;
use neo_config::ProtocolSettings;
use neo_manifest::CallFlags;
use neo_manifest::ContractMethodDescriptor;
use neo_payloads::{LogEventArgs, TransactionAttribute, VerifiableContainer};
use neo_primitives::ContractParameterType;
use neo_primitives::FindOptions;
use neo_storage::SeekDirection;
use neo_storage::{DataCache, EmptyCacheBacking};
use neo_vm::{StackItem, VmError, VmResult};
// ContractManagement is now accessed via the native contract provider
// LedgerContract and PolicyContract are now accessed via the native contract provider

use crate::NotifyEventArgs;
use crate::StorageContext;
use crate::native_contract_provider::{NativeContractProvider, NoNativeContractProvider};
use crate::{NativeContract, NativeContractsCache, NativeRegistry};
use neo_payloads::WitnessCondition;
use neo_primitives::TriggerType;
use neo_primitives::WitnessRuleAction;
use neo_primitives::{UInt160, UInt256};
use neo_storage::StorageItem;
use neo_storage::StorageKey;
use neo_vm_rs::ExecutionEngineLimits;
use neo_vm_rs::Instruction;
use neo_vm_rs::OpCode;
use neo_vm_rs::StackValue as VmStackValue;
use neo_vm_rs::SyscallProvider;
use neo_vm_rs::VmState as VMState;
use neo_vm_rs::interpret_with_stack_and_syscalls_at;
use neo_vm_rs::interpret_with_stack_and_syscalls_at_with_result_limit;
use num_traits::ToPrimitive;
use parking_lot::Mutex;
use rustc_hash::FxHashMap;
use std::collections::HashMap;
use std::sync::Arc;

/// GAS limit used for test-mode invocations, in datoshi.
pub const TEST_MODE_GAS: i64 = 20_000_000_000;
/// Maximum native/runtime event name length.
pub const MAX_EVENT_NAME: usize = 32;
/// Maximum serialized notification payload size.
pub const MAX_NOTIFICATION_SIZE: usize = 1024;
/// Maximum number of notifications emitted by one engine execution.
pub const MAX_NOTIFICATION_COUNT: usize = 512;
/// Execution fee charged for signature verification after Aspidochelone.
pub const CHECK_SIG_PRICE: i64 = 1 << 15;
/// PicoGAS-to-datoshi fee scale factor used by C# `ApplicationEngine`.
pub const FEE_FACTOR: i64 = 10000;

type InteropHandler<P, D, B> =
    fn(&mut ApplicationEngine<P, D, B>, &mut ExecutionEngine<B>) -> VmResult<()>;

// Keys are the same closed Neo syscall IDs validated by InteropService. Keep
// this fast map local to trusted protocol dispatch; other engine maps retain
// their existing hashers and iteration contracts.
type InteropHandlerMap<P, D, B> = FxHashMap<u32, HostInteropHandler<P, D, B>>;

/// Neo N3 application engine that hosts VM execution, syscalls, and native contracts.
pub struct ApplicationEngine<P = NoNativeContractProvider, D = NoDiagnostic, B = EmptyCacheBacking>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
{
    trigger: TriggerType,
    script_container: Option<Arc<VerifiableContainer>>,
    persisting_block: Option<Arc<Block>>,
    protocol_settings: Arc<ProtocolSettings>,
    gas_consumed: i64,
    fee_amount: i64,
    fee_consumed: i64,
    exec_fee_factor: u32,
    storage_price: u32,
    call_flags: CallFlags,
    vm_engine: VmEngineHost<B>,
    interop_handlers: InteropHandlerMap<P, D, B>,
    snapshot_cache: Arc<DataCache<B>>,
    original_snapshot_cache: Arc<DataCache<B>>,
    notifications: Vec<NotifyEventArgs>,
    logs: Vec<LogEventArgs>,
    native_registry: NativeRegistry<P>,
    native_contract_provider: Arc<P>,
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
    /// Identities of contexts loaded by `call_from_native_contract_returning`
    /// (the `Arc` pointer of each context's `ExecutionContextState`), mirroring
    /// the keys of C# `ApplicationEngine.contractTasks`: when such a context
    /// unloads with an uncaught exception, the unload hook faults the whole
    /// engine (C# throws `VMUnhandledException`) so no frame below the native
    /// call can catch the exception.
    native_call_boundary_contexts: Vec<usize>,
    nonce_data: [u8; 16],
    random_times: u32,
    diagnostic: D,
    fault_exception: Option<String>,
    native_arg_null_mask: u32,
    native_return_null: bool,
}

mod contracts;
mod drop;
// Retained for differential experiments; canonical execution does not dispatch
// here until hardfork-aware NeoVM parity is proven.
#[allow(dead_code)]
mod external_vm;
mod fees_events_native;
mod host_state;
mod interop_host;
mod state;
mod storage_ops;
mod witness_and_misc;

use host_state::{
    HostInteropHandler, PendingNativeCall, StdResult, VmEngineHost, map_core_error_to_vm_error,
};
