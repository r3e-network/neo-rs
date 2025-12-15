use super::*;

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
            pending_native_calls: Vec::new(),
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

    pub(super) fn attach_host(&mut self) {
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
        self.interop_handlers
            .insert(hash, HostInteropHandler { price, handler });
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
            Ok(state_arc) => state_arc.lock().call_flags.contains(required),
            Err(_) => false,
        }
    }

    pub fn get_current_call_flags(&self) -> VmResult<CallFlags> {
        let state_arc = self.current_execution_state()?;
        let call_flags = state_arc.lock().call_flags;
        Ok(call_flags)
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
        if let Some(block) = self.persisting_block.as_ref() {
            return block.header.index;
        }

        LedgerContract::new()
            .current_index(self.snapshot_cache.as_ref())
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
        self.protocol_settings
            .is_hardfork_enabled(hardfork, self.current_block_index())
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

    /// Returns true if the given hash belongs to a native contract (even if inactive).
    pub fn is_native_contract_hash(&self, hash: &UInt160) -> bool {
        self.native_registry.is_native(hash)
    }

    pub fn push_log(&mut self, event: LogEventArgs) {
        // Runtime notification callbacks moved to neo-node
        // The runtime context can implement custom notification handling
        // if let Some(context) = self.runtime_context.as_ref() {
        //     context.notify_application_log(self, &event);
        // }
        self.logs.push(event);
    }

    pub fn push_notification(&mut self, event: NotifyEventArgs) {
        // Runtime notification callbacks moved to neo-node
        // The runtime context can implement custom notification handling
        // if let Some(context) = self.runtime_context.as_ref() {
        //     context.notify_application_notify(self, &event);
        // }
        self.notifications.push(event);
    }

    pub fn notifications(&self) -> &[NotifyEventArgs] {
        &self.notifications
    }

    /// Sets the runtime context used for logging/notify callbacks.
    pub fn set_runtime_context(&mut self, context: Option<Arc<dyn SystemContext>>) {
        self.runtime_context = context;
    }

    pub fn get_invocation_counter(&self, script_hash: &UInt160) -> u32 {
        self.invocation_counter
            .get(script_hash)
            .copied()
            .unwrap_or(0)
    }

    pub(super) fn increment_invocation_counter(&mut self, script_hash: &UInt160) -> u32 {
        let counter = self.invocation_counter.entry(*script_hash).or_insert(0);
        *counter = counter.saturating_add(1);
        *counter
    }

    pub(super) fn native_contract_cache(&self) -> Arc<Mutex<NativeContractsCache>> {
        Arc::clone(&self.native_contract_cache)
    }

    /// Returns a clone of the current snapshot cache.
    pub fn snapshot_cache(&self) -> Arc<DataCache> {
        Arc::clone(&self.snapshot_cache)
    }

    pub(super) fn policy_contract(&self) -> Option<Arc<dyn NativeContract>> {
        let policy_hash = PolicyContract::new().hash();
        self.native_registry.get(&policy_hash)
    }

    pub(super) fn get_contract(&self, hash: &UInt160) -> Option<&ContractState> {
        self.contracts.get(hash)
    }

}
