use super::*;

// Neo N3 v3.10 registers 37 base host syscalls plus 4 Faun storage-local
// syscalls. Every transaction engine rebuilds this table, so pre-sizing avoids
// allocator churn without changing the registered protocol surface.
const HOST_SYSCALL_REGISTRATION_CAPACITY: usize = 41;

impl ApplicationEngine {
    /// Selects the VM jump table for the persisting block, mirroring C#
    /// `ApplicationEngine.Create`: `index = persistingBlock?.Index ??
    /// Ledger.CurrentIndex(snapshot)`; then:
    ///   - `HF_Gorgon ? default :`
    ///   - `HF_Echidna ? not_gorgon :`
    ///   - `not_echidna`
    ///
    /// The three-way selection is consensus-critical because `NotGorgon` and
    /// `Default` differ in how `HASKEY`/`PICKITEM`/`SETITEM`/`REMOVE` handle
    /// boundary conditions (the pre-/post-neo-vm#543 handlers). (`SHL`/`SHR` do
    /// NOT differ across these tables — their zero-shift behavior is a flat
    /// Neo.VM 3.9.0→3.10.0 change handled in the `shift` handler, not a hardfork
    /// gate.)
    fn select_jump_table(
        protocol_settings: &ProtocolSettings,
        persisting_block: Option<&Block>,
        snapshot: &DataCache,
    ) -> JumpTable {
        let index = match persisting_block {
            Some(block) => block.header.index(),
            None => {
                crate::native_contract_provider::NativeContractLookup::lookup_current_block_index(
                    snapshot,
                )
            }
        };
        if protocol_settings.is_hardfork_enabled(Hardfork::HfGorgon, index) {
            JumpTable::default()
        } else if protocol_settings.is_hardfork_enabled(Hardfork::HfEchidna, index) {
            JumpTable::not_gorgon()
        } else {
            JumpTable::not_echidna()
        }
    }

    /// Creates a new application engine using an owned optional persisting block.
    pub fn new(
        trigger: TriggerType,
        script_container: Option<Arc<dyn Verifiable>>,
        snapshot_cache: Arc<DataCache>,
        persisting_block: Option<Block>,
        protocol_settings: ProtocolSettings,
        gas_limit: i64,
        diagnostic: Option<Box<dyn Diagnostic>>,
    ) -> CoreResult<Self> {
        Self::new_with_shared_block(
            trigger,
            script_container,
            snapshot_cache,
            persisting_block.map(Arc::new),
            protocol_settings,
            gas_limit,
            diagnostic,
        )
    }

    /// Creates a new application engine using a shared optional persisting block.
    pub fn new_with_shared_block(
        trigger: TriggerType,
        script_container: Option<Arc<dyn Verifiable>>,
        snapshot_cache: Arc<DataCache>,
        persisting_block: Option<Arc<Block>>,
        protocol_settings: ProtocolSettings,
        gas_limit: i64,
        diagnostic: Option<Box<dyn Diagnostic>>,
    ) -> CoreResult<Self> {
        let nonce_data =
            Self::initialize_nonce_data(script_container.as_ref(), persisting_block.as_deref());
        let original_snapshot_cache = Arc::clone(&snapshot_cache);
        let jump_table = Self::select_jump_table(
            &protocol_settings,
            persisting_block.as_deref(),
            snapshot_cache.as_ref(),
        );
        let mut engine = ExecutionEngine::new(Some(jump_table));
        // Match C# Neo: no instruction-count cap on the execution path. Bounding
        // is done by gas alone (fee consumption), so a long cheap-instruction
        // script HALTs instead of FAULTing at the upstream 1M-instruction
        // default. See ExecutionEngine::set_max_instructions for the rationale.
        engine.set_max_instructions(u64::MAX);

        let mut app = Self {
            trigger,
            script_container,
            persisting_block,
            protocol_settings,
            gas_consumed: 0,
            fee_amount: gas_limit.saturating_mul(FEE_FACTOR),
            fee_consumed: 0,
            // Safe defaults; overwritten by refresh_policy_settings().
            exec_fee_factor: 30u32 * (FEE_FACTOR as u32),
            storage_price: 100_000u32,
            call_flags: CallFlags::ALL,
            vm_engine: VmEngineHost::new(engine),
            interop_handlers: HashMap::with_capacity(HOST_SYSCALL_REGISTRATION_CAPACITY),
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
            native_calling_override: None,
            entry_script_hash: None,
            invocation_counter: HashMap::new(),
            pending_native_calls: Vec::new(),
            native_call_boundary_contexts: Vec::new(),
            host_syscall_registrations: Vec::with_capacity(HOST_SYSCALL_REGISTRATION_CAPACITY),
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
        // Ignore any fees incurred during engine initialization (native contract setup, policy reads).
        app.fee_consumed = 0;
        app.gas_consumed = 0;

        if let Some(mut diagnostic) = app.diagnostic.take() {
            diagnostic.initialized(&mut app);
            app.diagnostic = Some(diagnostic);
        }

        Ok(app)
    }

    #[allow(clippy::too_many_arguments)]
    /// Creates a new engine with preloaded native contract state and cache.
    pub fn new_with_preloaded_native(
        trigger: TriggerType,
        script_container: Option<Arc<dyn Verifiable>>,
        snapshot_cache: Arc<DataCache>,
        persisting_block: Option<Arc<Block>>,
        protocol_settings: ProtocolSettings,
        gas_limit: i64,
        contracts: HashMap<UInt160, ContractState>,
        native_contract_cache: Arc<Mutex<NativeContractsCache>>,
        diagnostic: Option<Box<dyn Diagnostic>>,
    ) -> CoreResult<Self> {
        let nonce_data =
            Self::initialize_nonce_data(script_container.as_ref(), persisting_block.as_deref());
        let original_snapshot_cache = Arc::clone(&snapshot_cache);
        let jump_table = Self::select_jump_table(
            &protocol_settings,
            persisting_block.as_deref(),
            snapshot_cache.as_ref(),
        );
        let mut engine = ExecutionEngine::new(Some(jump_table));
        // Match C# Neo: no instruction-count cap on the execution path. Bounding
        // is done by gas alone (fee consumption), so a long cheap-instruction
        // script HALTs instead of FAULTing at the upstream 1M-instruction
        // default. See ExecutionEngine::set_max_instructions for the rationale.
        engine.set_max_instructions(u64::MAX);

        let mut app = Self {
            trigger,
            script_container,
            persisting_block,
            protocol_settings,
            gas_consumed: 0,
            fee_amount: gas_limit.saturating_mul(FEE_FACTOR),
            fee_consumed: 0,
            // Safe defaults; overwritten by refresh_policy_settings().
            exec_fee_factor: 30u32 * (FEE_FACTOR as u32),
            storage_price: 100_000u32,
            call_flags: CallFlags::ALL,
            vm_engine: VmEngineHost::new(engine),
            interop_handlers: HashMap::with_capacity(HOST_SYSCALL_REGISTRATION_CAPACITY),
            snapshot_cache,
            original_snapshot_cache,
            notifications: Vec::new(),
            logs: Vec::new(),
            native_registry: NativeRegistry::new(),
            native_contract_cache,
            contracts,
            storage_iterators: HashMap::new(),
            next_iterator_id: 1,
            current_script_hash: None,
            calling_script_hash: None,
            native_calling_override: None,
            entry_script_hash: None,
            invocation_counter: HashMap::new(),
            pending_native_calls: Vec::new(),
            native_call_boundary_contexts: Vec::new(),
            host_syscall_registrations: Vec::with_capacity(HOST_SYSCALL_REGISTRATION_CAPACITY),
            nonce_data,
            random_times: 0,
            diagnostic,
            fault_exception: None,
            states: HashMap::new(),
            runtime_context: None,
        };

        app.attach_host();
        app.refresh_policy_settings();
        app.register_default_interops()?;
        // Ignore any fees incurred during engine initialization.
        app.fee_consumed = 0;
        app.gas_consumed = 0;

        if let Some(mut diagnostic) = app.diagnostic.take() {
            diagnostic.initialized(&mut app);
            app.diagnostic = Some(diagnostic);
        }

        Ok(app)
    }

    #[allow(unsafe_code)]
    pub(super) fn attach_host(&mut self) {
        let host: &mut dyn InteropHost = self;
        let host_ptr = host as *mut dyn InteropHost;
        // SAFETY: `host_ptr` is derived from `&mut self` and the
        // `ApplicationEngine` owns the `VmEngine`, so the pointer remains
        // valid for the engine's lifetime. The caller must not move the
        // `ApplicationEngine` after calling `attach_host()` until the VM
        // execution completes.
        unsafe { self.vm_engine.engine_mut().set_interop_host(host_ptr) };

        // Debug assertion: verify the stored pointer is self-referential.
        // Catches misuse where the ApplicationEngine is moved after
        // attach_host() is called, which would leave a dangling interop
        // host pointer in the VM engine.
        #[cfg(debug_assertions)]
        {
            let stored: *mut dyn InteropHost = host_ptr;
            let current: *mut dyn InteropHost = self as *mut dyn InteropHost;
            debug_assert_eq!(
                stored, current,
                "ApplicationEngine was moved after attach_host() — VM engine holds dangling interop host pointer"
            );
        }
    }

    fn register_default_interops(&mut self) -> CoreResult<()> {
        self.register_contract_interops()
            .map_err(|err| Self::map_vm_error("System.Contract", err))?;
        self.register_runtime_interops()
            .map_err(|err| Self::map_vm_error("System.Runtime", err))?;
        self.register_storage_interops()
            .map_err(|err| Self::map_vm_error("System.Storage", err))?;
        self.register_iterator_interops()
            .map_err(|err| Self::map_vm_error("System.Iterator", err))?;
        self.register_crypto_interops()
            .map_err(|err| Self::map_vm_error("System.Crypto", err))?;
        Ok(())
    }

    fn map_vm_error(context: &str, error: VmError) -> CoreError {
        CoreError::invalid_operation(format!("{context} interop failed: {error}"))
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
        self.interop_handlers.insert(
            hash,
            HostInteropHandler {
                price,
                required_call_flags: call_flags,
                handler,
            },
        );
        // Remember the registration so nested VM execution from a native frame
        // can rebuild the registry while `on_syscall` holds the original.
        self.host_syscall_registrations
            .push((name.to_string(), price, call_flags));
        Ok(())
    }

    /// Returns the current VM execution state.
    pub fn state(&self) -> VMState {
        self.vm_engine.engine().state()
    }

    /// Returns the fault exception message as a string slice, if any.
    pub fn fault_exception_string(&self) -> Option<&str> {
        self.fault_exception.as_deref()
    }

    /// Sets the fault exception message.
    pub fn set_fault_exception<S: Into<String>>(&mut self, message: S) {
        self.fault_exception = Some(message.into());
    }

    /// Clears the fault exception message.
    pub fn clear_fault_exception(&mut self) {
        self.fault_exception = None;
    }

    /// Stores a typed state value in the engine's state map.
    pub fn set_state<T: Any + Send + Sync>(&mut self, value: T) {
        self.states.insert(TypeId::of::<T>(), Box::new(value));
    }

    /// Retrieves a reference to a typed state value.
    pub fn get_state<T: Any + Send + Sync>(&self) -> Option<&T> {
        self.states
            .get(&TypeId::of::<T>())
            .and_then(|boxed| boxed.downcast_ref::<T>())
    }

    /// Retrieves a mutable reference to a typed state value.
    pub fn get_state_mut<T: Any + Send + Sync>(&mut self) -> Option<&mut T> {
        self.states
            .get_mut(&TypeId::of::<T>())
            .and_then(|boxed| boxed.downcast_mut::<T>())
    }

    /// Removes and returns a typed state value.
    pub fn take_state<T: Any + Send + Sync>(&mut self) -> Option<T> {
        self.states
            .remove(&TypeId::of::<T>())
            .and_then(|boxed| boxed.downcast::<T>().ok())
            .map(|boxed| *boxed)
    }

    /// Records the VM state for a transaction in the ledger state tracker.
    pub fn record_transaction_vm_state(&mut self, hash: &UInt256, vm_state: VMState) -> bool {
        // LedgerTransactionStates is owned by neo-native-contracts; we don't
        // have a direct reference to it from neo-execution. Return false for
        // now; this is reactivated when the state is registered via
        // set_runtime_context.
        let _ = (hash, vm_state);
        false
    }

    /// Pushes a stack item onto the evaluation stack.
    pub fn push(&mut self, item: StackItem) -> StdResult<()> {
        self.vm_engine
            .engine_mut()
            .push(item)
            .map_err(|err| CoreError::other(err.to_string()))
    }

    /// Pops a stack item from the evaluation stack.
    pub fn pop(&mut self) -> StdResult<StackItem> {
        self.vm_engine
            .engine_mut()
            .pop()
            .map_err(|err| CoreError::other(err.to_string()))
    }

    /// Peeks at a stack item at the given index without removing it.
    pub fn peek(&self, index: usize) -> StdResult<StackItem> {
        self.vm_engine
            .engine()
            .peek(index)
            .map_err(|err| CoreError::other(err.to_string()))
    }

    /// Returns the invocation stack of execution contexts.
    pub fn invocation_stack(&self) -> &[ExecutionContext] {
        self.vm_engine.engine().invocation_stack()
    }

    /// Returns the script hash of the calling contract.
    pub fn get_calling_script_hash(&self) -> Option<UInt160> {
        self.calling_script_hash
    }

    /// Returns the script hash of the currently executing contract.
    pub fn current_script_hash(&self) -> Option<UInt160> {
        self.current_script_hash
    }

    /// Returns the script hash of the entry point contract.
    pub fn entry_script_hash(&self) -> Option<UInt160> {
        self.entry_script_hash
    }

    /// Checks if the current execution context has the required call flags.
    pub fn has_call_flags(&self, required: CallFlags) -> bool {
        match self.current_execution_state() {
            Ok(state_arc) => state_arc.lock().call_flags.contains(required),
            Err(_) => false,
        }
    }

    /// Returns the call flags of the current execution context.
    pub fn get_current_call_flags(&self) -> VmResult<CallFlags> {
        let state_arc = self.current_execution_state()?;
        let call_flags = state_arc.lock().call_flags;
        Ok(call_flags)
    }

    /// Returns the execution state of the current context.
    pub fn current_execution_state(&self) -> VmResult<Arc<Mutex<ExecutionContextState>>> {
        let context = self
            .vm_engine
            .engine()
            .current_context()
            .ok_or_else(|| VmError::invalid_operation_msg("No current execution context"))?;
        Ok(context.get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new))
    }

    /// Returns the index of the block currently being persisted.
    pub fn current_block_index(&self) -> u32 {
        if let Some(block) = self.persisting_block.as_ref() {
            return block.header.index();
        }

        crate::native_contract_provider::NativeContractLookup::lookup_current_block_index(
            self.snapshot_cache.as_ref(),
        )
    }

    /// Returns the timestamp of the block currently being persisted.
    ///
    /// Mirrors C# `ApplicationEngine.Time`, which reads the persisting block's
    /// timestamp. Fails when there is no persisting block — i.e. the engine was
    /// constructed without one (for example a verification-trigger or a bare
    /// test harness). The C# message frames this as a trigger-type guard
    /// ("Time can only be called with Application trigger"), but the actual
    /// precondition is the presence of a persisting block, which is why we
    /// phrase the error in those terms.
    pub fn current_block_timestamp(&self) -> CoreResult<u64> {
        self.persisting_block
            .as_deref()
            .map(|block| block.header.timestamp())
            .ok_or_else(|| {
                CoreError::other(
                    "GetTime requires a persisting block (no persisting block on this engine)",
                )
            })
    }

    /// Returns the block currently being persisted, if any.
    pub fn persisting_block(&self) -> Option<&Block> {
        self.persisting_block.as_deref()
    }

    /// Returns the block currently being persisted, or an error if none.
    pub fn get_persisting_block(&self) -> CoreResult<Block> {
        self.persisting_block()
            .cloned()
            .ok_or_else(|| CoreError::native_contract("No persisting block available"))
    }

    /// Checks if a hardfork is enabled for this engine.
    pub fn is_hardfork_enabled(&self, hardfork: Hardfork) -> bool {
        // C# `ApplicationEngine.IsHardforkEnabled`: with no persisting block
        // (notably verification engines), a configured hardfork is considered active.
        if self.persisting_block.is_none() {
            return self.protocol_settings.hardforks.contains_key(&hardfork);
        }

        self.protocol_settings
            .is_hardfork_enabled(hardfork, self.current_block_index())
    }

    /// Returns the trigger type for this execution.
    pub fn trigger_type(&self) -> TriggerType {
        self.trigger
    }

    /// Returns the trigger type (alias for trigger_type).
    pub fn trigger(&self) -> TriggerType {
        self.trigger
    }

    /// Returns the total GAS consumed during execution.
    pub fn gas_consumed(&self) -> i64 {
        (self.gas_consumed + FEE_FACTOR - 1) / FEE_FACTOR
    }

    /// Returns the total fee consumed during execution.
    pub fn fee_consumed(&self) -> i64 {
        (self.fee_consumed + FEE_FACTOR - 1) / FEE_FACTOR
    }

    /// Returns the raw picoGAS fee consumed during execution.
    pub const fn fee_consumed_pico(&self) -> i64 {
        self.fee_consumed
    }

    /// Returns the raw picoGAS execution fee limit.
    pub const fn fee_amount_pico(&self) -> i64 {
        self.fee_amount
    }

    #[must_use]
    /// Returns the raw execution fee factor cached from the Policy contract.
    pub const fn exec_fee_factor_raw(&self) -> u32 {
        self.exec_fee_factor
    }

    /// Returns the current storage price (datoshi per byte) cached from the Policy contract.
    pub fn storage_price(&self) -> u32 {
        self.storage_price
    }

    /// Returns the VM fault exception message, if execution has faulted.
    pub fn fault_exception(&self) -> Option<&str> {
        self.fault_exception.as_deref()
    }

    /// Returns the VM result stack.
    pub fn result_stack(&self) -> &EvaluationStack {
        self.vm_engine.engine().result_stack()
    }

    /// Returns the protocol settings used by this engine.
    pub fn protocol_settings(&self) -> &ProtocolSettings {
        &self.protocol_settings
    }

    /// Returns the script container associated with this execution, if any.
    pub fn script_container(&self) -> Option<&Arc<dyn Verifiable>> {
        self.script_container.as_ref()
    }

    /// Returns the VM execution limits active for this engine.
    pub fn execution_limits(&self) -> &ExecutionEngineLimits {
        self.vm_engine.engine().limits()
    }

    /// Returns a display name for a deployed or native contract hash.
    pub fn contract_display_name(&self, hash: &UInt160) -> Option<String> {
        if let Some(contract) = self.contracts.get(hash) {
            return Some(contract.manifest.name.clone());
        }

        self.native_registry
            .get(hash)
            .map(|native| native.name().to_string())
    }

    /// Returns true if the given hash belongs to a native contract (even if inactive).
    pub fn is_native_contract_hash(&self, hash: &UInt160) -> bool {
        self.native_registry.is_native(hash)
    }

    /// Records a log event emitted by runtime interop.
    pub fn push_log(&mut self, event: LogEventArgs) {
        // The runtime context is type-erased; downcasting is not done here.
        // Callers that want log notification can iterate `logs()` after
        // the engine finishes.
        let _ = self.runtime_context.as_ref();
        self.logs.push(event);
    }

    /// Records a notification event emitted by runtime interop.
    pub fn push_notification(&mut self, event: NotifyEventArgs) {
        // See `push_log` — runtime context is type-erased.
        let _ = self.runtime_context.as_ref();
        self.notifications.push(event);
    }

    /// Returns all notification events emitted during execution.
    pub fn notifications(&self) -> &[NotifyEventArgs] {
        &self.notifications
    }

    /// Returns all log events emitted during execution.
    pub fn logs(&self) -> &[LogEventArgs] {
        &self.logs
    }

    /// Sets the runtime context used for logging/notify callbacks.
    pub fn set_runtime_context(&mut self, context: Option<Arc<dyn std::any::Any + Send + Sync>>) {
        self.runtime_context = context;
    }

    /// Returns how many times a script hash has been invoked in this engine.
    pub fn get_invocation_counter(&self, script_hash: &UInt160) -> u32 {
        self.invocation_counter
            .get(script_hash)
            .copied()
            .unwrap_or(0)
    }

    pub(crate) fn get_or_init_invocation_counter(&mut self, script_hash: &UInt160) -> u32 {
        *self.invocation_counter.entry(*script_hash).or_insert(1)
    }

    pub(super) fn increment_invocation_counter(&mut self, script_hash: &UInt160) -> u32 {
        let counter = self.invocation_counter.entry(*script_hash).or_insert(0);
        *counter = counter.saturating_add(1);
        *counter
    }

    pub(super) fn native_contract_cache(&self) -> Arc<Mutex<NativeContractsCache>> {
        Arc::clone(&self.native_contract_cache)
    }

    /// Returns a shared handle to the native-contract cache.
    pub fn native_contract_cache_handle(&self) -> Arc<Mutex<NativeContractsCache>> {
        Arc::clone(&self.native_contract_cache)
    }

    /// Returns a cloned snapshot of contract states known to this engine.
    pub fn contracts_snapshot(&self) -> HashMap<UInt160, ContractState> {
        self.contracts.clone()
    }

    /// Returns a clone of the current snapshot cache.
    pub fn snapshot_cache(&self) -> Arc<DataCache> {
        Arc::clone(&self.snapshot_cache)
    }

    pub(super) fn policy_contract(&self) -> Option<Arc<dyn NativeContract>> {
        // Native contracts are served by the globally-installed provider; the
        // engine's own `native_registry` is empty in the normal execution path
        // (only `call_native_contract` consulted the provider as a fallback).
        // Without the same fallback here, `policy_contract()` returns `None`,
        // `refresh_policy_settings` silently no-ops, and execution keeps the
        // hardcoded default ExecFeeFactor/StoragePrice instead of the values in
        // state — a consensus divergence once governance changes them (e.g.
        // MainNet lowered ExecFeeFactor 30 -> 15).
        let policy =
            crate::native_contract_provider::NativeContractLookup::lookup_policy_contract()?;
        Some(self.native_registry.get(&policy.hash()).unwrap_or(policy))
    }

    pub(super) fn get_contract(&self, hash: &UInt160) -> Option<&ContractState> {
        self.contracts.get(hash)
    }

    /// Extracts all storage changes from the execution as raw key-value pairs.
    ///
    /// This method returns the state changes accumulated during contract execution,
    /// which can be used for state root calculation via MPT trie.
    ///
    /// # Returns
    /// A vector of tuples where:
    /// - First element: serialized storage key bytes (contract_id + key_suffix)
    /// - Second element: `Some(value_bytes)` for additions/updates, `None` for deletions
    ///
    /// # Example
    /// ```ignore
    /// let engine = ApplicationEngine::new(...)?;
    /// engine.execute();
    /// let changes = engine.extract_storage_changes();
    /// for (key_bytes, value_opt) in changes {
    ///     // Process state changes for state root calculation
    /// }
    /// ```
    pub fn extract_storage_changes(&self) -> Vec<(Vec<u8>, Option<Vec<u8>>)> {
        self.snapshot_cache.extract_raw_changes()
    }

    /// Returns the number of pending storage changes.
    pub fn pending_storage_change_count(&self) -> usize {
        self.snapshot_cache.pending_change_count()
    }

    /// Returns true if there are any pending storage changes.
    pub fn has_pending_storage_changes(&self) -> bool {
        self.snapshot_cache.has_pending_changes()
    }
}

#[cfg(test)]
#[path = "../tests/application_engine/state.rs"]
mod tests;
