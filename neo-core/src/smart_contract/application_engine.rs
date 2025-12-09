//! Application engine core implementation aligned with Neo C# version.
//!
//! This module implements the Neo N3 smart contract execution engine, providing
//! the runtime environment for executing NeoVM scripts with blockchain context.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    ApplicationEngine                         │
//! │  ┌─────────────────────────────────────────────────────────┐│
//! │  │                   ExecutionEngine (VM)                   ││
//! │  │  ┌──────────┐  ┌──────────┐  ┌──────────────────────┐  ││
//! │  │  │ Script   │  │ Stack    │  │ Execution Contexts   │  ││
//! │  │  │ Loader   │  │ Manager  │  │ (call stack)         │  ││
//! │  │  └──────────┘  └──────────┘  └──────────────────────┘  ││
//! │  └─────────────────────────────────────────────────────────┘│
//! │  ┌─────────────────────────────────────────────────────────┐│
//! │  │                  Interop Services                        ││
//! │  │  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌─────────┐ ││
//! │  │  │ Runtime  │  │ Storage  │  │ Crypto   │  │ Contract│ ││
//! │  │  │ Interops │  │ Interops │  │ Interops │  │ Interops│ ││
//! │  │  └──────────┘  └──────────┘  └──────────┘  └─────────┘ ││
//! │  └─────────────────────────────────────────────────────────┘│
//! │  ┌─────────────────────────────────────────────────────────┐│
//! │  │                  Blockchain Context                      ││
//! │  │  ┌──────────┐  ┌──────────┐  ┌──────────────────────┐  ││
//! │  │  │ DataCache│  │ Settings │  │ Native Contracts     │  ││
//! │  │  │ (state)  │  │ (proto)  │  │ (NEO, GAS, Policy)   │  ││
//! │  │  └──────────┘  └──────────┘  └──────────────────────┘  ││
//! │  └─────────────────────────────────────────────────────────┘│
//! └─────────────────────────────────────────────────────────────┘
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

use crate::cryptography::crypto_utils::NeoHash;
use crate::error::{CoreError as Error, Result};
use crate::hardfork::Hardfork;
use crate::ledger::Block;
use crate::neo_config::HASH_SIZE;
use crate::neo_system::NeoSystemContext;
use crate::network::p2p::payloads::Transaction;
use crate::persistence::data_cache::DataCache;
use crate::persistence::i_read_only_store::IReadOnlyStoreGeneric;
use crate::persistence::seek_direction::SeekDirection;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::application_engine_contract::register_contract_interops;
use crate::smart_contract::application_engine_crypto::register_crypto_interops;
use crate::smart_contract::application_engine_iterator::register_iterator_interops;
use crate::smart_contract::application_engine_runtime::register_runtime_interops;
use crate::smart_contract::application_engine_storage::register_storage_interops;
use crate::smart_contract::call_flags::CallFlags;
use crate::smart_contract::contract_parameter_type::ContractParameterType;
use crate::smart_contract::contract_state::ContractState;
use crate::smart_contract::execution_context_state::ExecutionContextState;
use crate::smart_contract::find_options::FindOptions;
use crate::smart_contract::helper::Helper;
use crate::smart_contract::i_diagnostic::IDiagnostic;
use crate::smart_contract::iterators::i_iterator::IIterator;
use crate::smart_contract::iterators::StorageIterator;
use crate::smart_contract::log_event_args::LogEventArgs;
use crate::smart_contract::manifest::ContractMethodDescriptor;
use crate::smart_contract::native::ContractManagement;
use crate::smart_contract::native::{
    LedgerTransactionStates, NativeContract, NativeContractsCache, NativeRegistry, PolicyContract,
};
use crate::smart_contract::notify_event_args::NotifyEventArgs;
use crate::smart_contract::storage_context::StorageContext;
use crate::smart_contract::storage_item::StorageItem;
use crate::smart_contract::storage_key::StorageKey;
use crate::smart_contract::trigger_type::TriggerType;
use crate::IVerifiable;
use crate::{UInt160, UInt256};
use neo_vm::evaluation_stack::EvaluationStack;
use neo_vm::execution_context::ExecutionContext;
use neo_vm::execution_engine_limits::ExecutionEngineLimits;
use neo_vm::instruction::Instruction;
use neo_vm::interop_service::InteropHost;
use neo_vm::jump_table::JumpTable;
use neo_vm::script::Script;
use neo_vm::stack_item::InteropInterface as VmInteropInterface;
use neo_vm::vm_state::VMState;
use neo_vm::{ExecutionEngine, StackItem, VmError, VmResult};
use num_traits::ToPrimitive;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

pub const TEST_MODE_GAS: i64 = 20_000_000_000;
pub const MAX_EVENT_NAME: usize = 32;
pub const MAX_NOTIFICATION_SIZE: usize = 1024;
pub const MAX_NOTIFICATION_COUNT: usize = 512;
pub const CHECK_SIG_PRICE: i64 = 1 << 15;

type InteropHandler = fn(&mut ApplicationEngine, &mut ExecutionEngine) -> VmResult<()>;
type StdResult<T> = std::result::Result<T, String>;

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
    container: Arc<dyn IVerifiable>,
}

impl VerifiableInterop {
    fn new(container: Arc<dyn IVerifiable>) -> Self {
        Self { container }
    }
}

impl fmt::Debug for VerifiableInterop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VerifiableInterop")
    }
}

impl VmInteropInterface for VerifiableInterop {
    fn interface_type(&self) -> &str {
        "IVerifiable"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self.container.as_any()
    }
}

pub struct ApplicationEngine {
    trigger: TriggerType,
    script_container: Option<Arc<dyn IVerifiable>>,
    persisting_block: Option<Block>,
    protocol_settings: ProtocolSettings,
    gas_limit: i64,
    gas_consumed: i64,
    fee_amount: i64,
    fee_consumed: i64,
    exec_fee_factor: u32,
    storage_price: u32,
    call_flags: CallFlags,
    vm_engine: VmEngineHost,
    interop_handlers: HashMap<u32, InteropHandler>,
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
    entry_script_hash: Option<UInt160>,
    invocation_counter: HashMap<UInt160, u32>,
    nonce_data: [u8; 16],
    random_times: u32,
    diagnostic: Option<Box<dyn IDiagnostic>>,
    fault_exception: Option<String>,
    states: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
    runtime_context: Option<Arc<NeoSystemContext>>,
}

impl ApplicationEngine {
    pub fn new(
        trigger: TriggerType,
        script_container: Option<Arc<dyn IVerifiable>>,
        snapshot_cache: Arc<DataCache>,
        persisting_block: Option<Block>,
        protocol_settings: ProtocolSettings,
        gas_limit: i64,
        diagnostic: Option<Box<dyn IDiagnostic>>,
    ) -> Result<Self> {
        let nonce_data =
            Self::initialize_nonce_data(script_container.as_ref(), persisting_block.as_ref());
        let original_snapshot_cache = Arc::clone(&snapshot_cache);
        let engine = ExecutionEngine::new(Some(JumpTable::default()));

        let mut app = Self {
            trigger,
            script_container,
            persisting_block,
            protocol_settings,
            gas_limit,
            gas_consumed: 0,
            fee_amount: gas_limit,
            fee_consumed: 0,
            exec_fee_factor: PolicyContract::DEFAULT_EXEC_FEE_FACTOR,
            storage_price: PolicyContract::DEFAULT_STORAGE_PRICE,
            call_flags: CallFlags::ALL,
            vm_engine: VmEngineHost::new(engine),
            interop_handlers: HashMap::new(),
            snapshot_cache,
            original_snapshot_cache,
            notifications: Vec::new(),
            logs: Vec::new(),
            native_registry: NativeRegistry::new(),
            native_contract_cache: Arc::new(Mutex::new(NativeContractsCache::default())),
            contracts: HashMap::new(),
            storage_iterators: HashMap::new(),
            next_iterator_id: 1,
            current_script_hash: None,
            calling_script_hash: None,
            entry_script_hash: None,
            invocation_counter: HashMap::new(),
            nonce_data,
            random_times: 0,
            diagnostic,
            fault_exception: None,
            states: HashMap::new(),
            runtime_context: None,
        };

        app.attach_host();
        app.register_native_contracts();
        app.refresh_policy_settings();
        app.register_default_interops()?;

        if let Some(mut diagnostic) = app.diagnostic.take() {
            diagnostic.initialized(&mut app);
            app.diagnostic = Some(diagnostic);
        }

        Ok(app)
    }

    fn attach_host(&mut self) {
        let host_ptr = self as *mut _ as *mut dyn InteropHost;
        self.vm_engine.engine_mut().set_interop_host(host_ptr);
    }

    fn register_default_interops(&mut self) -> Result<()> {
        register_contract_interops(self)
            .map_err(|err| Self::map_vm_error("System.Contract", err))?;
        register_runtime_interops(self).map_err(|err| Self::map_vm_error("System.Runtime", err))?;
        register_storage_interops(self).map_err(|err| Self::map_vm_error("System.Storage", err))?;
        register_iterator_interops(self)
            .map_err(|err| Self::map_vm_error("System.Iterator", err))?;
        register_crypto_interops(self).map_err(|err| Self::map_vm_error("System.Crypto", err))?;
        Ok(())
    }

    fn map_vm_error(context: &str, error: VmError) -> Error {
        Error::invalid_operation(format!("{context} interop failed: {error}"))
    }

    pub(crate) fn register_host_service(
        &mut self,
        name: &str,
        price: i64,
        call_flags: CallFlags,
        handler: InteropHandler,
    ) -> VmResult<()> {
        let interop_service = self
            .vm_engine
            .engine_mut()
            .interop_service_mut()
            .ok_or_else(|| VmError::invalid_operation_msg("Interop service not configured"))?;
        let hash = interop_service.register_host_descriptor(name, price, call_flags)?;
        self.interop_handlers.insert(hash, handler);
        Ok(())
    }

    pub fn state(&self) -> VMState {
        self.vm_engine.engine().state()
    }

    pub fn fault_exception_string(&self) -> Option<&str> {
        self.fault_exception.as_deref()
    }

    pub fn set_fault_exception<S: Into<String>>(&mut self, message: S) {
        self.fault_exception = Some(message.into());
    }

    pub fn clear_fault_exception(&mut self) {
        self.fault_exception = None;
    }

    pub fn set_state<T: Any + Send + Sync>(&mut self, value: T) {
        self.states.insert(TypeId::of::<T>(), Box::new(value));
    }

    pub fn get_state<T: Any + Send + Sync>(&self) -> Option<&T> {
        self.states
            .get(&TypeId::of::<T>())
            .and_then(|boxed| boxed.downcast_ref::<T>())
    }

    pub fn get_state_mut<T: Any + Send + Sync>(&mut self) -> Option<&mut T> {
        self.states
            .get_mut(&TypeId::of::<T>())
            .and_then(|boxed| boxed.downcast_mut::<T>())
    }

    pub fn take_state<T: Any + Send + Sync>(&mut self) -> Option<T> {
        self.states
            .remove(&TypeId::of::<T>())
            .and_then(|boxed| boxed.downcast::<T>().ok())
            .map(|boxed| *boxed)
    }

    #[cfg(test)]
    pub(crate) fn force_vm_state(&mut self, state: VMState) {
        self.vm_engine.engine_mut().set_state(state);
    }

    pub fn record_transaction_vm_state(&mut self, hash: &UInt256, vm_state: VMState) -> bool {
        if let Some(states) = self.get_state_mut::<LedgerTransactionStates>() {
            states.mark_vm_state(hash, vm_state)
        } else {
            false
        }
    }

    pub fn push(&mut self, item: StackItem) -> StdResult<()> {
        self.vm_engine
            .engine_mut()
            .push(item)
            .map_err(|err| err.to_string())
    }

    pub fn pop(&mut self) -> StdResult<StackItem> {
        self.vm_engine
            .engine_mut()
            .pop()
            .map_err(|err| err.to_string())
    }

    pub fn peek(&self, index: usize) -> StdResult<&StackItem> {
        self.vm_engine
            .engine()
            .peek(index)
            .map_err(|err| err.to_string())
    }

    pub fn invocation_stack(&self) -> &[ExecutionContext] {
        self.vm_engine.engine().invocation_stack()
    }

    pub fn get_calling_script_hash(&self) -> Option<UInt160> {
        self.calling_script_hash
    }

    pub fn current_script_hash(&self) -> Option<UInt160> {
        self.current_script_hash
    }

    pub fn entry_script_hash(&self) -> Option<UInt160> {
        self.entry_script_hash
    }

    pub fn has_call_flags(&self, required: CallFlags) -> bool {
        match self.current_execution_state() {
            Ok(state_arc) => state_arc
                .lock()
                .map(|state| state.call_flags.contains(required))
                .unwrap_or(false),
            Err(_) => false,
        }
    }

    pub fn get_current_call_flags(&self) -> VmResult<CallFlags> {
        let state_arc = self.current_execution_state()?;
        let state = state_arc
            .lock()
            .map_err(|_| VmError::invalid_operation_msg("Execution context state lock poisoned"))?;
        Ok(state.call_flags)
    }

    pub fn current_execution_state(&self) -> VmResult<Arc<Mutex<ExecutionContextState>>> {
        let context = self
            .vm_engine
            .engine()
            .current_context()
            .ok_or_else(|| VmError::invalid_operation_msg("No current execution context"))?;
        Ok(context.get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new))
    }

    pub fn current_block_index(&self) -> u32 {
        self.persisting_block
            .as_ref()
            .map(|block| block.header.index)
            .unwrap_or(0)
    }

    pub fn current_block_timestamp(&self) -> Result<u64, String> {
        self.persisting_block
            .as_ref()
            .map(|block| block.header.timestamp)
            .ok_or_else(|| "No persisting block available".to_string())
    }

    /// Returns the block currently being persisted, if any.
    pub fn persisting_block(&self) -> Option<&Block> {
        self.persisting_block.as_ref()
    }

    pub fn is_hardfork_enabled(&self, hardfork: Hardfork) -> bool {
        crate::hardfork::is_hardfork_enabled(hardfork, self.current_block_index())
    }

    pub fn trigger_type(&self) -> TriggerType {
        self.trigger
    }

    pub fn trigger(&self) -> TriggerType {
        self.trigger
    }

    pub fn gas_consumed(&self) -> i64 {
        self.gas_consumed
    }

    pub fn fee_consumed(&self) -> i64 {
        self.fee_consumed
    }

    /// Returns the current storage price (datoshi per byte) cached from the Policy contract.
    pub fn storage_price(&self) -> u32 {
        self.storage_price
    }

    pub fn fault_exception(&self) -> Option<&str> {
        self.fault_exception.as_deref()
    }

    pub fn result_stack(&self) -> &EvaluationStack {
        self.vm_engine.engine().result_stack()
    }

    pub(crate) fn current_evaluation_stack(&self) -> Option<&EvaluationStack> {
        self.vm_engine
            .engine()
            .current_context()
            .map(|ctx| ctx.evaluation_stack())
    }

    pub fn protocol_settings(&self) -> &ProtocolSettings {
        &self.protocol_settings
    }

    pub fn script_container(&self) -> Option<&Arc<dyn IVerifiable>> {
        self.script_container.as_ref()
    }

    pub fn execution_limits(&self) -> &ExecutionEngineLimits {
        self.vm_engine.engine().limits()
    }

    pub fn contract_display_name(&self, hash: &UInt160) -> Option<String> {
        if let Some(contract) = self.contracts.get(hash) {
            return Some(contract.manifest.name.clone());
        }

        self.native_registry
            .get(hash)
            .map(|native| native.name().to_string())
    }

    pub fn push_log(&mut self, event: LogEventArgs) {
        if let Some(context) = self.runtime_context.as_ref() {
            context.notify_application_log(self, &event);
        }
        self.logs.push(event);
    }

    pub fn push_notification(&mut self, event: NotifyEventArgs) {
        if let Some(context) = self.runtime_context.as_ref() {
            context.notify_application_notify(self, &event);
        }
        self.notifications.push(event);
    }

    pub fn notifications(&self) -> &[NotifyEventArgs] {
        &self.notifications
    }

    /// Sets the runtime context used for logging/notify callbacks.
    pub fn set_runtime_context(&mut self, context: Option<Arc<NeoSystemContext>>) {
        self.runtime_context = context;
    }

    pub fn get_invocation_counter(&self, script_hash: &UInt160) -> u32 {
        self.invocation_counter
            .get(script_hash)
            .copied()
            .unwrap_or(0)
    }

    fn increment_invocation_counter(&mut self, script_hash: &UInt160) -> u32 {
        let counter = self.invocation_counter.entry(*script_hash).or_insert(0);
        *counter = counter.saturating_add(1);
        *counter
    }

    fn native_contract_cache(&self) -> Arc<Mutex<NativeContractsCache>> {
        Arc::clone(&self.native_contract_cache)
    }

    /// Returns a clone of the current snapshot cache.
    pub fn snapshot_cache(&self) -> Arc<DataCache> {
        Arc::clone(&self.snapshot_cache)
    }

    fn policy_contract(&self) -> Option<Arc<dyn NativeContract>> {
        let policy_hash = PolicyContract::new().hash();
        self.native_registry.get(&policy_hash)
    }

    fn get_contract(&self, hash: &UInt160) -> Option<&ContractState> {
        self.contracts.get(hash)
    }

    pub fn get_storage_item(&self, context: &StorageContext, key: &[u8]) -> Option<Vec<u8>> {
        let storage_key = StorageKey::new(context.id, key.to_vec());
        if let Some(item) = self.snapshot_cache.get(&storage_key) {
            return Some(item.get_value());
        }

        self.original_snapshot_cache
            .get(&storage_key)
            .map(|item| item.get_value())
    }

    fn validate_find_options(&self, options: FindOptions) -> Result<(), String> {
        if options.bits() & !FindOptions::All.bits() != 0 {
            return Err(format!("Invalid FindOptions value: {options:?}"));
        }

        let keys_only = options.contains(FindOptions::KeysOnly);
        let values_only = options.contains(FindOptions::ValuesOnly);
        let deserialize = options.contains(FindOptions::DeserializeValues);
        let pick_field0 = options.contains(FindOptions::PickField0);
        let pick_field1 = options.contains(FindOptions::PickField1);

        if keys_only && (values_only || deserialize || pick_field0 || pick_field1) {
            return Err("KeysOnly cannot be used with ValuesOnly, DeserializeValues, PickField0, or PickField1".to_string());
        }

        if values_only && (keys_only || options.contains(FindOptions::RemovePrefix)) {
            return Err("ValuesOnly cannot be used with KeysOnly or RemovePrefix".to_string());
        }

        if pick_field0 && pick_field1 {
            return Err("PickField0 and PickField1 cannot be used together".to_string());
        }

        if (pick_field0 || pick_field1) && !deserialize {
            return Err("PickField0 or PickField1 requires DeserializeValues".to_string());
        }

        Ok(())
    }

    pub fn put_storage_item(
        &mut self,
        context: &StorageContext,
        key: &[u8],
        value: &[u8],
    ) -> Result<()> {
        let storage_key = StorageKey::new(context.id, key.to_vec());
        let existing = self.snapshot_cache.get(&storage_key);
        let value_len = value.len();
        let new_data_size = if let Some(existing_item) = &existing {
            let old_len = existing_item.size();
            if value_len == 0 {
                0
            } else if value_len <= old_len && value_len > 0 {
                ((value_len.saturating_sub(1)) / 4) + 1
            } else if old_len == 0 {
                value_len
            } else {
                ((old_len.saturating_sub(1)) / 4) + 1 + value_len.saturating_sub(old_len)
            }
        } else {
            key.len() + value_len
        };

        if new_data_size > 0 {
            let fee_units = new_data_size as u64;
            let storage_price = self.get_storage_price() as u64;
            self.add_runtime_fee(fee_units.saturating_mul(storage_price))?;
        }

        let item = StorageItem::from_bytes(value.to_vec());
        if existing.is_some() {
            self.snapshot_cache.update(storage_key, item);
        } else {
            self.snapshot_cache.add(storage_key, item);
        }
        Ok(())
    }

    pub fn delete_storage_item(&mut self, context: &StorageContext, key: &[u8]) -> Result<()> {
        let storage_key = StorageKey::new(context.id, key.to_vec());
        self.snapshot_cache.delete(&storage_key);
        Ok(())
    }

    pub fn push_interop_container(
        &mut self,
        container: Arc<dyn IVerifiable>,
    ) -> Result<(), String> {
        let interop = VerifiableInterop::new(container);
        self.push(StackItem::from_interface(interop))
    }

    pub fn pop_iterator_id(&mut self) -> Result<u32, String> {
        let item = self.pop()?;
        let identifier = item
            .as_int()
            .map_err(|e| e.to_string())?
            .to_u32()
            .ok_or_else(|| "Iterator identifier out of range".to_string())?;
        Ok(identifier)
    }

    pub fn iterator_next_internal(&mut self, iterator_id: u32) -> Result<bool, String> {
        let iterator = self
            .storage_iterators
            .get_mut(&iterator_id)
            .ok_or_else(|| format!("Iterator {} not found", iterator_id))?;
        Ok(iterator.next())
    }

    pub fn iterator_value_internal(&self, iterator_id: u32) -> Result<StackItem, String> {
        let iterator = self
            .storage_iterators
            .get(&iterator_id)
            .ok_or_else(|| format!("Iterator {} not found", iterator_id))?;
        Ok(iterator.value())
    }

    pub(crate) fn load_script_with_state<F>(
        &mut self,
        script_bytes: Vec<u8>,
        rvcount: i32,
        initial_position: usize,
        configure: F,
    ) -> Result<ExecutionContext>
    where
        F: FnOnce(&mut ExecutionContextState),
    {
        // Ensure the VM has a valid host pointer in case the engine has moved since creation.
        self.attach_host();

        let script = Script::from(script_bytes)
            .map_err(|e| Error::invalid_operation(format!("Invalid script: {e}")))?;

        {
            let engine = self.vm_engine.engine_mut();
            let context = engine.create_context(script, rvcount, initial_position);

            let script_hash = UInt160::from_bytes(&context.script_hash())
                .map_err(|e| Error::invalid_operation(format!("Invalid script hash: {e}")))?;
            let state_arc = context
                .get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new);
            {
                let mut state = state_arc.lock().map_err(|_| {
                    Error::invalid_operation("Execution context state lock poisoned".to_string())
                })?;
                state.snapshot_cache = Some(Arc::clone(&self.snapshot_cache));
                configure(&mut state);
                if state.script_hash.is_none() {
                    state.script_hash = Some(script_hash);
                }
            }

            engine
                .load_context(context)
                .map_err(|e| Error::invalid_operation(e.to_string()))?;
        }

        let new_context = self
            .vm_engine
            .engine()
            .current_context()
            .cloned()
            .ok_or_else(|| {
                Error::invalid_operation("Failed to load execution context".to_string())
            })?;

        self.refresh_context_tracking()?;

        Ok(new_context)
    }

    fn fetch_contract(&mut self, hash: &UInt160) -> Result<ContractState> {
        if let Some(contract) = self.contracts.get(hash) {
            return Ok(contract.clone());
        }

        let management_hash = ContractManagement::new().hash();
        if let Some(native) = self.native_registry.get(&management_hash) {
            if let Some(manager) = native.as_any().downcast_ref::<ContractManagement>() {
                let contract = manager
                    .get_contract(hash)
                    .map_err(|e| Error::invalid_operation(e.to_string()))?;
                if let Some(contract) = contract {
                    self.contracts.insert(*hash, contract.clone());
                    return Ok(contract);
                }
            }
        }

        Err(Error::not_found(format!("Contract not found: {hash:?}")))
    }

    fn is_contract_blocked(&mut self, contract_hash: &UInt160) -> Result<bool> {
        let policy_hash = PolicyContract::new().hash();
        let args = vec![contract_hash.to_bytes()];
        let original_flags = self.call_flags;
        self.call_flags |= CallFlags::READ_STATES;
        let result = self.call_native_contract(policy_hash, "isBlocked", &args);
        self.call_flags = original_flags;
        match result {
            Ok(value) => Ok(!value.is_empty() && value[0] != 0),
            Err(Error::NotFound { .. }) => Ok(false),
            Err(err) => Err(err),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn load_contract_context(
        &mut self,
        contract: ContractState,
        method: ContractMethodDescriptor,
        flags: CallFlags,
        argument_count: usize,
        previous_context: Option<ExecutionContext>,
        previous_hash: Option<UInt160>,
        has_return_value: bool,
    ) -> Result<ExecutionContext> {
        if method.offset < 0 {
            return Err(Error::invalid_operation(
                "Method offset cannot be negative".to_string(),
            ));
        }

        let script_bytes = contract.nef.script.clone();
        let rvcount = if has_return_value { 1 } else { 0 };
        let offset = method.offset as usize;

        let contract_clone = contract.clone();
        let method_clone = method.clone();
        let prev_context_clone = previous_context.clone();
        let prev_hash = previous_hash;

        self.load_script_with_state(script_bytes, rvcount, offset, move |state| {
            state.call_flags = flags;
            state.contract = Some(contract_clone.clone());
            state.method_name = Some(method_clone.name.clone());
            state.argument_count = argument_count;
            state.return_type = Some(method_clone.return_type);
            state.native_calling_script_hash = None;
            state.is_dynamic_call = false;
            state.script_hash = Some(contract_clone.hash);
            state.calling_context = prev_context_clone.clone();
            state.calling_script_hash = prev_hash;
        })
    }

    fn call_contract_internal(
        &mut self,
        contract: &ContractState,
        method: &ContractMethodDescriptor,
        mut flags: CallFlags,
        has_return_value: bool,
        args: &[StackItem],
    ) -> Result<ExecutionContext> {
        if args.len() != method.parameters.len() {
            return Err(Error::invalid_operation(format!(
                "Method '{}' expects {} arguments but received {}.",
                method.name,
                method.parameters.len(),
                args.len()
            )));
        }

        if has_return_value != (method.return_type != ContractParameterType::Void) {
            return Err(Error::invalid_operation(
                "The return value type does not match.".to_string(),
            ));
        }

        if self.is_contract_blocked(&contract.hash)? {
            return Err(Error::invalid_operation(format!(
                "The contract {} has been blocked.",
                contract.hash
            )));
        }

        let previous_context = self
            .vm_engine
            .engine()
            .current_context()
            .cloned()
            .ok_or_else(|| Error::invalid_operation("No current execution context".to_string()))?;
        let previous_hash = UInt160::from_bytes(&previous_context.script_hash())
            .map_err(|e| Error::invalid_operation(format!("Invalid script hash: {e}")))?;

        let state_arc = previous_context
            .get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new);
        let (calling_flags, executing_contract) = {
            let state = state_arc.lock().map_err(|_| {
                Error::invalid_operation("Execution context state lock poisoned".to_string())
            })?;
            (state.call_flags, state.contract.clone())
        };

        if method.safe {
            flags.remove(CallFlags::WRITE_STATES | CallFlags::ALLOW_NOTIFY);
        } else if let Some(executing_contract) = executing_contract {
            if !executing_contract.manifest.can_call(
                &contract.manifest,
                &contract.hash,
                &method.name,
            ) {
                return Err(Error::invalid_operation(format!(
                    "Cannot call method {} of contract {} from contract {}.",
                    method.name, contract.hash, previous_hash
                )));
            }
        }

        flags &= calling_flags;

        self.increment_invocation_counter(&contract.hash);

        let new_context = self.load_contract_context(
            contract.clone(),
            method.clone(),
            flags,
            args.len(),
            Some(previous_context.clone()),
            Some(previous_hash),
            has_return_value,
        )?;

        {
            let engine = self.vm_engine.engine_mut();
            let context_mut = engine.current_context_mut().ok_or_else(|| {
                Error::invalid_operation("No current execution context".to_string())
            })?;
            for arg in args.iter().rev() {
                context_mut.evaluation_stack_mut().push(arg.clone());
            }
        }

        self.refresh_context_tracking()?;

        Ok(new_context)
    }

    pub fn call_contract_dynamic(
        &mut self,
        contract_hash: &UInt160,
        method: &str,
        call_flags: CallFlags,
        args: Vec<StackItem>,
    ) -> Result<()> {
        if method.starts_with('_') {
            return Err(Error::invalid_operation(format!(
                "Method name '{}' cannot start with underscore.",
                method
            )));
        }

        let contract = self.fetch_contract(contract_hash)?;
        let method_descriptor = contract
            .manifest
            .abi
            .get_method_ref(method, args.len())
            .cloned()
            .ok_or_else(|| {
                Error::invalid_operation(format!(
                    "Method '{}' with {} parameter(s) doesn't exist in the contract {:?}.",
                    method,
                    args.len(),
                    contract_hash
                ))
            })?;

        let has_return_value = method_descriptor.return_type != ContractParameterType::Void;
        let context = self.call_contract_internal(
            &contract,
            &method_descriptor,
            call_flags,
            has_return_value,
            &args,
        )?;

        let state_arc =
            context.get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new);
        if let Ok(mut state) = state_arc.lock() {
            state.is_dynamic_call = true;
        }

        Ok(())
    }

    /// Loads a raw script into the VM, configuring call flags and optional script hash.
    pub fn load_script(
        &mut self,
        script: Vec<u8>,
        call_flags: CallFlags,
        script_hash: Option<UInt160>,
    ) -> Result<()> {
        self.load_script_with_state(script, 0, 0, move |state| {
            state.call_flags = call_flags;
            if let Some(hash) = script_hash {
                state.script_hash = Some(hash);
            }
        })?;
        Ok(())
    }

    /// Loads a contract method into the VM using the provided descriptor.
    pub fn load_contract_method(
        &mut self,
        contract: ContractState,
        method: ContractMethodDescriptor,
        call_flags: CallFlags,
    ) -> Result<()> {
        let has_return_value = method.return_type != ContractParameterType::Void;
        let previous_context = self.vm_engine.engine().current_context().cloned();
        let previous_hash = if let Some(ref ctx) = previous_context {
            Some(
                UInt160::from_bytes(&ctx.script_hash())
                    .map_err(|e| Error::invalid_operation(format!("Invalid script hash: {e}")))?,
            )
        } else {
            None
        };

        self.load_contract_context(
            contract.clone(),
            method.clone(),
            call_flags,
            method.parameters.len(),
            previous_context,
            previous_hash,
            has_return_value,
        )?;
        Ok(())
    }

    /// Executes the loaded scripts until the VM halts or faults.
    pub fn execute(&mut self) -> Result<()> {
        // Keep the engine host pointer aligned with this instance across moves.
        self.attach_host();

        let state = self.vm_engine.engine_mut().execute();
        if state == VMState::FAULT {
            if self.vm_engine.engine().uncaught_exception().is_some() {
                self.set_fault_exception("VM execution fault during verification");
            }
            return Err(Error::invalid_operation(
                "VM execution faulted during script verification".to_string(),
            ));
        }
        Ok(())
    }

    /// Adds gas to the consumed amount
    pub fn add_gas(&mut self, amount: i64) -> Result<()> {
        self.gas_consumed = self.gas_consumed.saturating_add(amount);
        if self.gas_consumed > self.gas_limit {
            return Err(Error::invalid_operation("Gas limit exceeded".to_string()));
        }
        Ok(())
    }

    /// Emit a notification event
    pub fn emit_notification(
        &mut self,
        script_hash: &UInt160,
        event_name: &str,
        state: &[Vec<u8>],
    ) -> Result<()> {
        if let Some(container) = &self.script_container {
            let state_items = state
                .iter()
                .cloned()
                .map(StackItem::from_byte_string)
                .collect::<Vec<_>>();

            let notification = NotifyEventArgs::new(
                Arc::clone(container),
                *script_hash,
                event_name.to_string(),
                state_items,
            );
            self.emit_notify_event(notification);
        }
        Ok(())
    }

    /// Check if committee witness is present
    ///
    /// # Security
    /// This method verifies that the current execution context has a valid
    /// witness from the committee multi-signature address. This is required
    /// for administrative operations like setting gas per block, register price,
    /// minimum deployment fee, etc.
    ///
    /// The committee address is computed from the current committee members
    /// (or standby committee if not yet initialized) using a multi-signature
    /// script requiring majority approval.
    pub fn check_committee_witness(&self) -> Result<bool> {
        // SECURITY FIX: Previously this just called container.verify() which
        // always returned true. Now we properly verify against the committee
        // multi-signature address.

        // Get the committee multi-signature address
        // This computes the script hash from committee public keys with M-of-N threshold
        let committee_address = crate::smart_contract::native::NativeHelpers::committee_address(
            self.protocol_settings(),
            Some(self.snapshot_cache.as_ref()),
        );

        // Verify that the committee address has witnessed this execution
        // This checks if the transaction signers include the committee multi-sig
        Ok(self.check_witness_hash(&committee_address))
    }

    /// Clear all storage for a contract
    pub fn clear_contract_storage(&mut self, contract_hash: &UInt160) -> Result<()> {
        let Some(contract_id) = self.get_contract_id_by_hash(contract_hash) else {
            return Ok(());
        };
        let search_prefix = StorageKey::new(contract_id, Vec::new());
        let keys: Vec<_> = self
            .snapshot_cache
            .find(Some(&search_prefix), SeekDirection::Forward)
            .map(|(key, _)| key)
            .collect();
        for key in keys {
            self.snapshot_cache.delete(&key);
        }
        Ok(())
    }

    /// Gets the storage context for the current contract (matches C# GetStorageContext exactly).
    pub fn get_storage_context(&self) -> Result<StorageContext> {
        // 1. Get current contract hash
        let contract_hash = self
            .current_script_hash
            .ok_or_else(|| Error::invalid_operation("No current contract".to_string()))?;

        // 2. Get contract state to get the ID
        let contract = self
            .get_contract(&contract_hash)
            .ok_or_else(|| Error::not_found(format!("Contract not found: {}", contract_hash)))?;

        // 3. Create storage context (matches C# StorageContext creation)
        Ok(StorageContext {
            id: contract.id,
            is_read_only: false,
        })
    }

    /// Gets a read-only storage context (matches C# GetReadOnlyContext exactly).
    pub fn get_read_only_storage_context(&self) -> Result<StorageContext> {
        let mut context = self.get_storage_context()?;
        context.is_read_only = true;
        Ok(context)
    }

    /// Converts a storage context to read-only (matches C# AsReadOnly exactly).
    pub fn as_read_only_storage_context(&self, context: StorageContext) -> StorageContext {
        StorageContext {
            id: context.id,
            is_read_only: true,
        }
    }

    /// Finds storage entries with options (matches C# Find method exactly).
    pub fn find_storage_entries(
        &self,
        context: &StorageContext,
        prefix: &[u8],
        options: FindOptions,
    ) -> Result<StorageIterator, String> {
        self.validate_find_options(options)?;

        let search_key = StorageKey::new(context.id, prefix.to_vec());
        let direction = if options.contains(FindOptions::Backwards) {
            SeekDirection::Backward
        } else {
            SeekDirection::Forward
        };

        let mut entries_map: HashMap<StorageKey, StorageItem> = HashMap::new();
        for (key, value) in self.snapshot_cache.find(Some(&search_key), direction) {
            entries_map.insert(key, value);
        }

        for (key, value) in self
            .original_snapshot_cache
            .find(Some(&search_key), direction)
        {
            entries_map.entry(key).or_insert(value);
        }

        let mut entries: Vec<_> = entries_map.into_iter().collect();
        entries.sort_by(|a, b| a.0.suffix().cmp(b.0.suffix()));
        if direction == SeekDirection::Backward {
            entries.reverse();
        }

        Ok(StorageIterator::new(entries, prefix.len(), options))
    }

    /// Gets the storage price from the policy contract (matches C# StoragePrice property).
    fn get_storage_price(&mut self) -> usize {
        self.storage_price as usize
    }

    pub(crate) fn gas_left(&self) -> i64 {
        self.fee_amount.saturating_sub(self.fee_consumed)
    }

    pub(crate) fn nonce_bytes(&self) -> &[u8; 16] {
        &self.nonce_data
    }

    pub(crate) fn set_nonce_bytes(&mut self, value: [u8; 16]) {
        self.nonce_data = value;
    }

    pub(crate) fn random_counter(&self) -> u32 {
        self.random_times
    }

    pub(crate) fn increment_random_counter(&mut self) {
        self.random_times = self.random_times.wrapping_add(1);
    }

    pub(crate) fn add_runtime_fee(&mut self, fee: u64) -> Result<()> {
        self.add_fee(fee)
    }

    /// Adds gas fee.
    fn add_fee(&mut self, fee: u64) -> Result<()> {
        // 1. Calculate the actual fee based on ExecFeeFactor (matches C# logic exactly)
        let exec_fee_factor = self.exec_fee_factor as i64;
        let actual_fee = (fee as i64).saturating_mul(exec_fee_factor);

        // 2. Add to FeeConsumed/GasConsumed (matches C# FeeConsumed property exactly)
        self.fee_consumed = self.fee_consumed.saturating_add(actual_fee);
        self.gas_consumed = self.fee_consumed;

        // 3. Check against gas limit (matches C# gas limit check exactly)
        if self.fee_consumed > self.fee_amount {
            let required = self.fee_consumed.max(0) as u64;
            let available = self.fee_amount.max(0) as u64;
            return Err(Error::insufficient_gas(required, available));
        }

        Ok(())
    }

    pub fn charge_execution_fee(&mut self, fee: u64) -> Result<()> {
        self.add_fee(fee)
    }

    /// Emits a notification event.
    pub fn notify(&mut self, event_name: String, state: Vec<u8>) -> Result<()> {
        if let (Some(container), Some(contract_hash)) =
            (self.script_container.as_ref(), self.current_script_hash)
        {
            let event = NotifyEventArgs::new(
                Arc::clone(container),
                contract_hash,
                event_name,
                vec![StackItem::from_byte_string(state)],
            );
            self.emit_notify_event(event);
        }
        Ok(())
    }

    /// Emits a log event.
    pub fn log(&mut self, message: String) -> Result<()> {
        if let (Some(container), Some(contract_hash)) =
            (self.script_container.as_ref(), self.current_script_hash)
        {
            let log_event = LogEventArgs::new(Arc::clone(container), contract_hash, message);
            self.emit_log_event(log_event);
        }
        Ok(())
    }

    /// Emits an event.
    pub fn emit_event(&mut self, event_name: &str, args: Vec<Vec<u8>>) -> Result<()> {
        // 1. Validate event name length (must not exceed HASH_SIZE bytes)
        if event_name.len() > HASH_SIZE {
            return Err(Error::invalid_operation("Event name too long".to_string()));
        }

        // 2. Validate arguments count (must not exceed 16 arguments)
        if args.len() > 16 {
            return Err(Error::invalid_operation("Too many arguments".to_string()));
        }

        // 3. Get current contract hash
        let Some(contract_hash) = self.current_script_hash else {
            return Err(Error::invalid_operation("No current contract".to_string()));
        };

        let Some(container) = &self.script_container else {
            return Err(Error::invalid_operation(
                "Cannot emit event without a script container".to_string(),
            ));
        };

        let state_items = args
            .into_iter()
            .map(StackItem::from_byte_string)
            .collect::<Vec<_>>();

        let notification = NotifyEventArgs::new(
            Arc::clone(container),
            contract_hash,
            event_name.to_string(),
            state_items.clone(),
        );
        self.emit_notify_event(notification);

        Ok(())
    }

    /// Gets the calling script hash.
    pub fn calling_script_hash(&self) -> UInt160 {
        self.calling_script_hash.unwrap_or_else(UInt160::zero)
    }

    /// Checks if enough gas is available for an operation.
    pub fn check_gas(&self, required_gas: i64) -> Result<()> {
        if self.gas_consumed + required_gas > self.gas_limit {
            return Err(Error::invalid_operation("Out of gas".to_string()));
        }
        Ok(())
    }

    /// Calls a native contract method.
    pub fn call_native_contract(
        &mut self,
        contract_hash: UInt160,
        method: &str,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        let native = self
            .native_registry
            .get(&contract_hash)
            .ok_or_else(|| Error::not_found(contract_hash.to_string()))?;

        let block_height = self.current_block_index();
        if !native.is_active(&self.protocol_settings, block_height) {
            return Err(Error::invalid_operation(format!(
                "Native contract {} is not active at height {}",
                native.name(),
                block_height
            )));
        }

        let cache_arc = self.native_contract_cache();
        let method_meta = {
            let mut cache = cache_arc.lock().map_err(|_| {
                Error::invalid_operation("Native contract cache lock poisoned".to_string())
            })?;
            cache
                .get_or_build(native.as_ref())
                .get_method(method)
                .cloned()
        }
        .ok_or_else(|| {
            Error::invalid_operation(format!(
                "Method '{}' not found in native contract {}",
                method,
                native.name()
            ))
        })?;

        let required_flags =
            CallFlags::from_bits(method_meta.required_call_flags).ok_or_else(|| {
                Error::invalid_operation(format!(
                    "Method '{}' in native contract {} specifies invalid call flags",
                    method,
                    native.name()
                ))
            })?;
        if !self.call_flags.contains(required_flags) {
            return Err(Error::invalid_operation(format!(
                "Call flags {:?} do not satisfy required permissions {:?} for {}",
                self.call_flags, required_flags, method
            )));
        }

        let additional_fee = if method_meta.gas_cost > 0 {
            Some(u64::try_from(method_meta.gas_cost).map_err(|_| {
                Error::invalid_operation(format!("Gas cost overflow for native method {}", method))
            })?)
        } else {
            None
        };

        let result = native.invoke(self, method, args)?;

        if let Some(fee) = additional_fee {
            self.add_fee(fee)?;
        }

        Ok(result)
    }

    pub fn native_on_persist(&mut self) -> Result<()> {
        if self.trigger != TriggerType::OnPersist {
            return Err(Error::invalid_operation(
                "System.Contract.NativeOnPersist is only valid during OnPersist".to_string(),
            ));
        }

        let block_height = self
            .persisting_block
            .as_ref()
            .map(|block| block.header.index)
            .unwrap_or_else(|| self.current_block_index());

        let active_contracts: Vec<Arc<dyn NativeContract>> = self
            .native_registry
            .contracts()
            .filter(|contract| contract.is_active(&self.protocol_settings, block_height))
            .collect();

        for contract in active_contracts {
            contract.on_persist(self)?;
        }

        Ok(())
    }

    pub fn native_post_persist(&mut self) -> Result<()> {
        if self.trigger != TriggerType::PostPersist {
            return Err(Error::invalid_operation(
                "System.Contract.NativePostPersist is only valid during PostPersist".to_string(),
            ));
        }

        let block_height = self
            .persisting_block
            .as_ref()
            .map(|block| block.header.index)
            .unwrap_or_else(|| self.current_block_index());

        let active_contracts: Vec<Arc<dyn NativeContract>> = self
            .native_registry
            .contracts()
            .filter(|contract| contract.is_active(&self.protocol_settings, block_height))
            .collect();

        for contract in active_contracts {
            contract.post_persist(self)?;
        }

        Ok(())
    }

    pub fn consume_gas(&mut self, gas: i64) -> Result<()> {
        if gas < 0 {
            return Err(Error::invalid_operation(
                "Negative gas consumption".to_string(),
            ));
        }

        let Some(total) = self.fee_consumed.checked_add(gas) else {
            return Err(Error::invalid_operation(
                "Gas addition overflow".to_string(),
            ));
        };

        if total > self.fee_amount {
            return Err(Error::invalid_operation("Out of gas".to_string()));
        }

        self.fee_consumed = total;
        self.gas_consumed = total;

        self.vm_engine
            .engine_mut()
            .add_gas_consumed(gas)
            .map_err(|e| Error::invalid_operation(e.to_string()))?;

        self.update_vm_gas_counter(gas)?;
        Ok(())
    }

    /// Ensures gas usage stays within configured limits.
    fn update_vm_gas_counter(&mut self, _gas: i64) -> Result<()> {
        if self.gas_consumed > self.gas_limit {
            return Err(Error::invalid_operation(
                "VM exceeded gas limit during execution".to_string(),
            ));
        }

        Ok(())
    }

    /// Gets the script container (transaction or block).
    pub fn get_script_container(&self) -> Option<&Arc<dyn IVerifiable>> {
        self.script_container.as_ref()
    }

    /// Gets the transaction sender if the container is a transaction.
    /// This matches C# ApplicationEngine.GetTransactionSender exactly.
    pub fn get_transaction_sender(&self) -> Option<UInt160> {
        // 1. Check if we have a container
        let container = self.script_container.as_ref()?;

        // 2. Try to downcast to Transaction
        if let Some(transaction) = container.as_any().downcast_ref::<Transaction>() {
            // 3. Get the first signer's script hash (matches C# logic)
            if let Some(first_signer) = transaction.signers().first() {
                return Some(first_signer.account);
            }
        }

        // 4. Not a transaction or no signers
        None
    }

    /// Validates that the provided hash has a matching witness in the current transaction.
    ///
    /// # Security
    /// Ensures we only approve hashes that have a corresponding witness entry.
    /// Signature verification will be added once ECC integration (C-4) is available.
    pub fn check_witness(&self, hash: &UInt160) -> Result<bool> {
        let tx = self
            .script_container
            .as_ref()
            .and_then(|container| container.as_transaction())
            .ok_or_else(|| Error::invalid_operation("No transaction context".to_string()))?;

        if let Some((idx, _)) = tx
            .signers()
            .iter()
            .enumerate()
            .find(|(_, signer)| signer.account == *hash)
        {
            // TODO: Verify the witness signature using ECC once integrated
            if tx.witnesses().get(idx).is_some() {
                return Ok(true);
            }

            return Ok(false);
        }

        Ok(false)
    }

    /// Checks whether the specified hash has witnessed the current execution.
    pub fn check_witness_hash(&self, hash: &UInt160) -> bool {
        if self.get_calling_script_hash() == Some(*hash) {
            return true;
        }

        self.check_witness(hash).unwrap_or(false)
    }

    /// Gets the current execution context.
    /// This matches C# ApplicationEngine.CurrentContext exactly.
    pub fn current_context(&self) -> Option<&ExecutionContext> {
        // This implements the C# logic: engine.CurrentContext property
        self.vm_engine.current_context()
    }

    /// Deletes storage items by prefix.
    pub fn delete_storage_by_prefix(&mut self, prefix: &[u8]) -> Result<()> {
        let keys: Vec<_> = self
            .snapshot_cache
            .find(None, SeekDirection::Forward)
            .filter(|(key, _)| key.suffix().starts_with(prefix))
            .map(|(key, _)| key)
            .collect();

        for key in keys {
            self.snapshot_cache.delete(&key);
        }

        Ok(())
    }

    /// Gets the trigger type.
    pub fn get_trigger_type(&self) -> TriggerType {
        self.trigger
    }

    /// Returns all storage entries for a given contract.
    pub fn storage_entries_for_contract(
        &self,
        contract_hash: &UInt160,
    ) -> Vec<(StorageKey, StorageItem)> {
        let Some(contract_id) = self.get_contract_id_by_hash(contract_hash) else {
            return Vec::new();
        };
        let prefix = StorageKey::new(contract_id, Vec::new());
        self.snapshot_cache
            .find(Some(&prefix), SeekDirection::Forward)
            .collect()
    }

    /// Finds storage entries with a prefix.
    pub fn find_storage_entries_with_prefix(
        &self,
        contract_hash: &UInt160,
        prefix: &[u8],
    ) -> Vec<(StorageKey, StorageItem)> {
        let Some(contract_id) = self.get_contract_id_by_hash(contract_hash) else {
            return Vec::new();
        };
        let search_key = StorageKey::new(contract_id, prefix.to_vec());
        self.snapshot_cache
            .find(Some(&search_key), SeekDirection::Forward)
            .collect()
    }

    /// Creates a storage iterator.
    /// This matches C# Neo's ApplicationEngine.Find method exactly.
    pub fn create_storage_iterator(
        &mut self,
        results: Vec<(StorageKey, StorageItem)>,
    ) -> Result<u32> {
        let iterator_id = self.allocate_iterator_id()?;

        let iterator = StorageIterator::new(results, 0, FindOptions::None);
        self.storage_iterators.insert(iterator_id, iterator);

        Ok(iterator_id)
    }

    /// Creates a storage iterator with specific options.
    /// This matches C# Neo's ApplicationEngine.Find method with FindOptions exactly.
    pub fn create_storage_iterator_with_options(
        &mut self,
        results: Vec<(StorageKey, StorageItem)>,
        prefix_length: usize,
        options: FindOptions,
    ) -> Result<u32> {
        let iterator_id = self.allocate_iterator_id()?;
        let iterator = StorageIterator::new(results, prefix_length, options);
        self.storage_iterators.insert(iterator_id, iterator);

        Ok(iterator_id)
    }

    /// Stores an existing storage iterator and returns its identifier.
    pub fn store_storage_iterator(&mut self, iterator: StorageIterator) -> Result<u32, String> {
        let iterator_id = self.allocate_iterator_id().map_err(|err| err.to_string())?;
        self.storage_iterators.insert(iterator_id, iterator);
        Ok(iterator_id)
    }

    /// Gets a storage iterator by ID.
    pub fn get_storage_iterator(&self, iterator_id: u32) -> Option<&StorageIterator> {
        self.storage_iterators.get(&iterator_id)
    }

    /// Gets a mutable storage iterator by ID.
    pub fn get_storage_iterator_mut(&mut self, iterator_id: u32) -> Option<&mut StorageIterator> {
        self.storage_iterators.get_mut(&iterator_id)
    }

    /// Advances a storage iterator to the next element.
    /// Returns true if successful, false if at the end.
    pub fn iterator_next(&mut self, iterator_id: u32) -> Result<bool> {
        match self.storage_iterators.get_mut(&iterator_id) {
            Some(iterator) => Ok(iterator.next()),
            None => Err(Error::not_found(format!(
                "Iterator {} not found",
                iterator_id
            ))),
        }
    }

    /// Gets the current value from a storage iterator.
    pub fn iterator_value(&self, iterator_id: u32) -> Result<StackItem> {
        match self.storage_iterators.get(&iterator_id) {
            Some(iterator) => Ok(iterator.value()),
            None => Err(Error::not_found(format!(
                "Iterator {} not found",
                iterator_id
            ))),
        }
    }

    /// Removes a storage iterator (cleanup).
    pub fn dispose_iterator(&mut self, iterator_id: u32) -> Result<()> {
        self.storage_iterators.remove(&iterator_id);
        Ok(())
    }

    /// Sets the current script hash.
    pub fn set_current_script_hash(&mut self, hash: Option<UInt160>) {
        self.current_script_hash = hash;
    }

    fn allocate_iterator_id(&mut self) -> Result<u32> {
        let id = self.next_iterator_id;
        self.next_iterator_id = self
            .next_iterator_id
            .checked_add(1)
            .ok_or_else(|| Error::invalid_operation("Iterator identifier overflow"))?;
        Ok(id)
    }

    /// Gets contract ID for a given hash.
    fn get_contract_id_by_hash(&self, hash: &UInt160) -> Option<i32> {
        self.contracts.get(hash).map(|contract| contract.id)
    }

    /// Sets a storage item directly (for testing and internal use).
    pub fn set_storage(&mut self, key: StorageKey, item: StorageItem) -> Result<()> {
        if self.snapshot_cache.get(&key).is_some() {
            self.snapshot_cache.update(key, item);
        } else {
            self.snapshot_cache.add(key, item);
        }
        Ok(())
    }

    /// Gets a storage item directly (for testing and internal use).
    pub fn get_storage(&self, key: &StorageKey) -> Option<StorageItem> {
        self.snapshot_cache.get(key)
    }

    /// Deletes a storage item directly (for testing and internal use).
    pub fn delete_storage(&mut self, key: &StorageKey) -> Result<()> {
        self.snapshot_cache.delete(key);
        Ok(())
    }

    /// Gets the storage context for a native contract.
    pub fn get_native_storage_context(&self, contract_hash: &UInt160) -> Result<StorageContext> {
        // 1. Get contract state to get the ID
        let contract = self.get_contract(contract_hash).ok_or_else(|| {
            Error::not_found(format!("Native contract not found: {}", contract_hash))
        })?;

        // 2. Create storage context for native contract (always read-write for native contracts)
        Ok(StorageContext {
            id: contract.id,
            is_read_only: false,
        })
    }

    /// Gets a storage item by key (legacy API for native contracts).
    pub fn get_storage_item_legacy(&self, key: &[u8]) -> Option<Vec<u8>> {
        if let Some(current_hash) = &self.current_script_hash {
            if let Ok(context) = self.get_native_storage_context(current_hash) {
                return self.get_storage_item(&context, key);
            }
        }
        None
    }

    /// Puts a storage item (legacy API for native contracts).
    pub fn put_storage_item_legacy(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        if let Some(current_hash) = &self.current_script_hash {
            let context = self.get_native_storage_context(current_hash)?;
            return self.put_storage_item(&context, key, value);
        }
        Err(Error::invalid_operation(
            "No current contract context".to_string(),
        ))
    }

    pub fn create_standard_account(&mut self, public_key: &[u8]) -> Result<UInt160> {
        if public_key.len() != 33 {
            return Err(Error::invalid_operation(
                "Public key must be 33 bytes".to_string(),
            ));
        }

        let fee = if self.is_hardfork_enabled(Hardfork::HfAspidochelone) {
            CHECK_SIG_PRICE
        } else {
            1 << 8
        };

        self.add_fee(fee as u64)?;

        let script = Helper::signature_redeem_script(public_key);
        let hash = UInt160::from_bytes(&NeoHash::hash160(&script))
            .map_err(|e| Error::invalid_operation(format!("Invalid script hash: {}", e)))?;

        Ok(hash)
    }

    pub fn create_multisig_account(
        &mut self,
        required_signatures: i32,
        public_keys_items: Vec<StackItem>,
    ) -> Result<UInt160> {
        if required_signatures <= 0 {
            return Err(Error::invalid_operation(
                "Multisig threshold must be positive".to_string(),
            ));
        }

        let m = required_signatures as usize;
        if public_keys_items.is_empty()
            || public_keys_items.len() > 16
            || m > public_keys_items.len()
        {
            return Err(Error::invalid_operation(
                "Invalid multisig public key count".to_string(),
            ));
        }

        let mut public_keys = Vec::with_capacity(public_keys_items.len());
        for item in public_keys_items {
            let bytes = item
                .as_bytes()
                .map_err(|e| Error::invalid_operation(e.to_string()))?;
            if bytes.len() != 33 {
                return Err(Error::invalid_operation(
                    "Each multisig public key must be 33 bytes".to_string(),
                ));
            }
            public_keys.push(bytes);
        }

        let fee = if self.is_hardfork_enabled(Hardfork::HfAspidochelone) {
            CHECK_SIG_PRICE
                .checked_mul(public_keys.len() as i64)
                .ok_or_else(|| Error::invalid_operation("Multisig fee overflow".to_string()))?
        } else {
            1 << 8
        };

        self.add_fee(fee as u64)?;

        let script = Helper::multi_sig_redeem_script(m, &public_keys);
        let hash = UInt160::from_bytes(&NeoHash::hash160(&script))
            .map_err(|e| Error::invalid_operation(format!("Invalid script hash: {}", e)))?;

        Ok(hash)
    }

    /// Deletes a storage item (legacy API for native contracts).
    pub fn delete_storage_item_legacy(&mut self, key: &[u8]) -> Result<()> {
        if let Some(current_hash) = &self.current_script_hash {
            let context = self.get_native_storage_context(current_hash)?;
            return self.delete_storage_item(&context, key);
        }
        Err(Error::invalid_operation(
            "No current contract context".to_string(),
        ))
    }
    fn refresh_context_tracking(&mut self) -> Result<()> {
        if let Some(current_context) = self.vm_engine.engine().current_context() {
            let current_hash = UInt160::from_bytes(&current_context.script_hash())
                .map_err(|e| Error::invalid_operation(format!("Invalid script hash: {e}")))?;
            self.current_script_hash = Some(current_hash);
            if self.entry_script_hash.is_none() {
                self.entry_script_hash = Some(current_hash);
            }

            let state_arc = current_context
                .get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new);
            let state = state_arc.lock().map_err(|_| {
                Error::invalid_operation("Execution context state lock poisoned".to_string())
            })?;

            self.call_flags = state.call_flags;
            self.calling_script_hash = state
                .native_calling_script_hash
                .or(state.calling_script_hash)
                .or_else(|| {
                    state
                        .calling_context
                        .as_ref()
                        .and_then(|ctx| UInt160::from_bytes(&ctx.script_hash()).ok())
                });
        } else {
            self.current_script_hash = None;
            self.calling_script_hash = None;
            self.entry_script_hash = None;
            self.call_flags = CallFlags::ALL;
        }

        Ok(())
    }

    /// Registers native contracts in the contracts HashMap so they can be found
    fn register_native_contracts(&mut self) {
        let contracts: Vec<Arc<dyn NativeContract>> = self.native_registry.contracts().collect();

        for contract in &contracts {
            let hash = contract.hash();
            let id = contract.id();
            let name = contract.name().to_string();
            self.contracts
                .entry(hash)
                .or_insert_with(|| ContractState::new_native(id, hash, name));
        }

        for contract in contracts {
            if let Err(error) = contract.initialize(self) {
                if let Some(container) = &self.script_container {
                    let log_event = LogEventArgs::new(
                        Arc::clone(container),
                        contract.hash(),
                        format!(
                            "Native contract {} initialization error: {}",
                            contract.name(),
                            error
                        ),
                    );
                    self.emit_log_event(log_event);
                }
            }
        }
    }

    fn refresh_policy_settings(&mut self) {
        if let Some(policy) = self.policy_contract() {
            if let Ok(raw) = policy.invoke(self, "getExecFeeFactor", &[]) {
                if !raw.is_empty() {
                    let mut buffer = [0u8; 4];
                    let len = raw.len().min(4);
                    buffer[..len].copy_from_slice(&raw[..len]);
                    self.exec_fee_factor = u32::from_le_bytes(buffer);
                }
            }

            if let Ok(raw) = policy.invoke(self, "getStoragePrice", &[]) {
                if !raw.is_empty() {
                    let mut buffer = [0u8; 4];
                    let len = raw.len().min(4);
                    buffer[..len].copy_from_slice(&raw[..len]);
                    self.storage_price = u32::from_le_bytes(buffer);
                }
            }
        }
    }

    fn initialize_nonce_data(
        container: Option<&Arc<dyn IVerifiable>>,
        persisting_block: Option<&Block>,
    ) -> [u8; 16] {
        let mut data = [0u8; 16];

        if let Some(container) = container {
            if let Some(transaction) = container.as_any().downcast_ref::<Transaction>() {
                let hash_bytes = transaction.hash().to_bytes();
                data.copy_from_slice(&hash_bytes[..16]);
            }
        }

        if let Some(block) = persisting_block {
            let nonce_bytes = block.header.nonce.to_le_bytes();
            for (slot, byte) in data.iter_mut().take(8).zip(nonce_bytes.iter()) {
                *slot ^= *byte;
            }
        }

        data
    }

    /// Converts a VM stack item into bytes, mirroring the C# helper.
    pub fn stack_item_to_bytes(item: StackItem) -> Result<Vec<u8>, String> {
        item.as_bytes().map_err(|e| e.to_string())
    }
}

impl Drop for ApplicationEngine {
    fn drop(&mut self) {
        self.vm_engine.engine_mut().clear_interop_host();
        if let Some(diagnostic) = self.diagnostic.as_mut() {
            diagnostic.disposed();
        }
    }
}

impl InteropHost for ApplicationEngine {
    fn invoke_syscall(&mut self, engine: &mut ExecutionEngine, hash: u32) -> VmResult<()> {
        if let Some(handler) = self.interop_handlers.get(&hash).copied() {
            handler(self, engine)
        } else {
            Err(VmError::InteropService {
                service: format!("0x{hash:08x}"),
                error: "Interop handler not registered".to_string(),
            })
        }
    }

    fn on_context_loaded(
        &mut self,
        _engine: &mut ExecutionEngine,
        context: &ExecutionContext,
    ) -> VmResult<()> {
        if let Some(diagnostic) = self.diagnostic.as_mut() {
            diagnostic.context_loaded(context);
        }
        Ok(())
    }

    fn on_context_unloaded(
        &mut self,
        engine: &mut ExecutionEngine,
        context: &ExecutionContext,
    ) -> VmResult<()> {
        let state_arc =
            context.get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new);
        let mut state = state_arc
            .lock()
            .map_err(|_| VmError::invalid_operation_msg("Execution context state lock poisoned"))?;

        if engine.uncaught_exception().is_none() {
            if let Some(snapshot) = state.snapshot_cache.clone() {
                snapshot.commit();
            }

            if let Some(current_ctx) = engine.current_context() {
                let current_state_arc = current_ctx
                    .get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new);
                let mut current_state = current_state_arc.lock().map_err(|_| {
                    VmError::invalid_operation_msg("Execution context state lock poisoned")
                })?;
                current_state.notification_count = current_state
                    .notification_count
                    .saturating_add(state.notification_count);

                if state.is_dynamic_call {
                    let return_count = context.evaluation_stack().len();
                    match return_count {
                        0 => {
                            engine.push(StackItem::null())?;
                        }
                        1 => {
                            // Single return value is already on the evaluation stack and will be
                            // propagated by the VM according to the configured return count.
                        }
                        _ => {
                            return Err(VmError::invalid_operation_msg(
                                "Multiple return values are not allowed in cross-contract calls.",
                            ));
                        }
                    }
                }
            }
        } else if state.notification_count > 0 {
            if state.notification_count >= self.notifications.len() {
                self.notifications.clear();
            } else {
                let retain = self.notifications.len() - state.notification_count;
                self.notifications.truncate(retain);
            }
        }

        state.notification_count = 0;
        state.is_dynamic_call = false;

        self.refresh_context_tracking()
            .map_err(|e| VmError::invalid_operation_msg(e.to_string()))?;

        if let Some(diagnostic) = self.diagnostic.as_mut() {
            diagnostic.context_unloaded(context);
        }

        Ok(())
    }

    fn pre_execute_instruction(
        &mut self,
        _engine: &mut ExecutionEngine,
        _context: &ExecutionContext,
        instruction: &Instruction,
    ) -> VmResult<()> {
        if let Some(diagnostic) = self.diagnostic.as_mut() {
            diagnostic.pre_execute_instruction(instruction);
        }
        Ok(())
    }

    fn post_execute_instruction(
        &mut self,
        _engine: &mut ExecutionEngine,
        _context: &ExecutionContext,
        instruction: &Instruction,
    ) -> VmResult<()> {
        if let Some(diagnostic) = self.diagnostic.as_mut() {
            diagnostic.post_execute_instruction(instruction);
        }
        Ok(())
    }

    /// Handles CALLT opcode - calls a contract method via method token.
    ///
    /// This implements the C# ApplicationEngine.OnCallT logic:
    /// 1. Validates call flags (ReadStates | AllowCall)
    /// 2. Gets the current contract's NEF tokens
    /// 3. Looks up the method token by index
    /// 4. Pops the required arguments from the stack
    /// 5. Performs the cross-contract call
    fn on_callt(&mut self, engine: &mut ExecutionEngine, token_id: u16) -> VmResult<()> {
        // 1. Validate call flags - need ReadStates | AllowCall
        let required_flags = CallFlags::READ_STATES | CallFlags::ALLOW_CALL;
        let current_flags = self.get_current_call_flags().map_err(|e| {
            VmError::invalid_operation_msg(format!("Failed to get call flags: {}", e))
        })?;

        if !current_flags.contains(required_flags) {
            return Err(VmError::invalid_operation_msg(format!(
                "CALLT requires {:?} but current context has {:?}",
                required_flags, current_flags
            )));
        }

        // 2. Get the current execution context and contract state
        let context = engine
            .current_context()
            .ok_or_else(|| VmError::invalid_operation_msg("No current execution context"))?;

        let state_arc =
            context.get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new);
        let contract = {
            let state = state_arc
                .lock()
                .map_err(|_| VmError::invalid_operation_msg("State lock poisoned"))?;
            state.contract.clone().ok_or_else(|| {
                VmError::invalid_operation_msg("No contract in current execution context")
            })?
        };

        // 3. Validate token index and get the method token
        let token_idx = token_id as usize;
        if token_idx >= contract.nef.tokens.len() {
            return Err(VmError::invalid_operation_msg(format!(
                "Token index {} out of range (max: {})",
                token_idx,
                contract.nef.tokens.len()
            )));
        }
        let token = contract.nef.tokens[token_idx].clone();

        // 4. Validate stack has enough parameters
        let stack_count = context.evaluation_stack().len();
        if (token.parameters_count as usize) > stack_count {
            return Err(VmError::invalid_operation_msg(format!(
                "CALLT token requires {} parameters but stack has {}",
                token.parameters_count, stack_count
            )));
        }

        // 5. Pop arguments from the stack (in reverse order)
        let mut args = Vec::with_capacity(token.parameters_count as usize);
        for _ in 0..token.parameters_count {
            args.push(engine.pop()?);
        }

        // 6. Look up the target contract
        let target_contract = self.fetch_contract(&token.hash).map_err(|e| {
            VmError::invalid_operation_msg(format!(
                "Failed to fetch contract {}: {}",
                token.hash, e
            ))
        })?;

        // 7. Find the method descriptor in the target contract's ABI
        let method = target_contract
            .manifest
            .abi
            .get_method_ref(&token.method, token.parameters_count as usize)
            .cloned()
            .ok_or_else(|| {
                VmError::invalid_operation_msg(format!(
                    "Method '{}' with {} parameters not found in contract {}",
                    token.method, token.parameters_count, token.hash
                ))
            })?;

        // 8. Execute the cross-contract call
        self.call_contract_internal(
            &target_contract,
            &method,
            token.call_flags,
            token.has_return_value,
            &args,
        )
        .map_err(|e| VmError::invalid_operation_msg(format!("CALLT failed: {}", e)))?;

        Ok(())
    }
}
