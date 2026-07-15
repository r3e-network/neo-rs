use super::*;

// Neo N3 v3.10 registers 37 base host syscalls plus 4 Faun storage-local
// syscalls. Every transaction engine rebuilds this table, so pre-sizing avoids
// allocator churn without changing the registered protocol surface.
const HOST_SYSCALL_REGISTRATION_CAPACITY: usize = 41;

impl<P, D, B> ApplicationEngine<P, D, B>
where
    P: crate::native_contract_provider::NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    /// Selects the VM jump table for the persisting block, mirroring C#
    /// `ApplicationEngine.Create`: `index = persistingBlock?.Index ??
    /// Ledger.CurrentIndex(snapshot)`; then:
    ///   - `HF_Gorgon ? default :`
    ///   - `HF_Echidna ? not_gorgon :`
    ///   - `not_echidna`
    ///
    /// The three-way selection is consensus-critical because `NotGorgon` and
    /// `Default` differ in how `HASKEY`/`PICKITEM`/`SETITEM`/`REMOVE` handle
    /// boundary conditions (the pre-/post-neo-vm#543 handlers) and whether a
    /// zero shift consumes and coerces its value operand (neo-vm#567).
    fn select_jump_table(protocol_settings: &ProtocolSettings, current_index: u32) -> JumpTable<B> {
        if protocol_settings.is_hardfork_enabled(Hardfork::HfGorgon, current_index) {
            JumpTable::default()
        } else if protocol_settings.is_hardfork_enabled(Hardfork::HfEchidna, current_index) {
            JumpTable::not_gorgon()
        } else {
            JumpTable::not_echidna()
        }
    }

    fn engine_current_index(
        persisting_block: Option<&Block>,
        snapshot: &DataCache<B>,
        native_contract_provider: &P,
    ) -> u32 {
        persisting_block
            .map(|block| block.header.index())
            .or_else(|| native_contract_provider.current_block_index(snapshot).ok())
            .unwrap_or(0)
    }

    /// Creates a new application engine using an owned optional persisting block
    /// and a concrete native-contract provider type.
    ///
    /// Homogeneous node pipelines should use this direct generic constructor so
    /// the concrete provider and its associated contract-handle type remain
    /// visible through the entire engine.
    pub fn new_with_native_contract_provider(
        trigger: TriggerType,
        script_container: Option<Arc<VerifiableContainer>>,
        snapshot_cache: Arc<DataCache<B>>,
        persisting_block: Option<Block>,
        protocol_settings: ProtocolSettings,
        gas_limit: i64,
        diagnostic: D,
        native_contract_provider: Arc<P>,
    ) -> CoreResult<Self> {
        Self::new_with_shared_block_and_native_contract_provider(
            trigger,
            script_container,
            snapshot_cache,
            persisting_block.map(Arc::new),
            protocol_settings,
            gas_limit,
            diagnostic,
            native_contract_provider,
        )
    }

    /// Creates a new application engine with a shared optional persisting block
    /// and a concrete native-contract provider type.
    ///
    /// This constructor is the normal provider-aware entry point for
    /// import/persistence code that owns a single concrete provider for the
    /// whole batch. It keeps that type visible at call sites and through the
    /// engine's native-dispatch boundary.
    // Rationale: engine construction must thread the full protocol, snapshot,
    // gas, diagnostic, and provider context without hiding state in globals.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_shared_block_and_native_contract_provider(
        trigger: TriggerType,
        script_container: Option<Arc<VerifiableContainer>>,
        snapshot_cache: Arc<DataCache<B>>,
        persisting_block: Option<Arc<Block>>,
        protocol_settings: impl Into<Arc<ProtocolSettings>>,
        gas_limit: i64,
        diagnostic: D,
        native_contract_provider: Arc<P>,
    ) -> CoreResult<Self> {
        let protocol_settings = protocol_settings.into();
        let nonce_data =
            Self::initialize_nonce_data(script_container.as_ref(), persisting_block.as_deref());
        let original_snapshot_cache = Arc::clone(&snapshot_cache);
        let current_index = Self::engine_current_index(
            persisting_block.as_deref(),
            snapshot_cache.as_ref(),
            native_contract_provider.as_ref(),
        );
        let fee_whitelist_enabled =
            protocol_settings.is_hardfork_enabled(Hardfork::HfFaun, current_index);
        let jump_table = Self::select_jump_table(protocol_settings.as_ref(), current_index);
        let mut engine = ExecutionEngine::new(Some(jump_table));
        engine.set_interop_service(neo_vm::InteropService::with_capacity(
            HOST_SYSCALL_REGISTRATION_CAPACITY,
        ));
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
            fee_whitelist_enabled,
            // Safe defaults; overwritten by refresh_policy_settings().
            exec_fee_factor: 30u32 * (FEE_FACTOR as u32),
            storage_price: 100_000u32,
            call_flags: CallFlags::ALL,
            vm_engine: VmEngineHost::new(engine),
            interop_handlers: FxHashMap::with_capacity_and_hasher(
                HOST_SYSCALL_REGISTRATION_CAPACITY,
                Default::default(),
            ),
            snapshot_cache,
            original_snapshot_cache,
            notifications: Vec::new(),
            logs: Vec::new(),
            native_registry: NativeRegistry::new(),
            native_contract_provider,
            native_contract_cache: Arc::new(Mutex::new(NativeContractsCache::default())),
            contracts: HashMap::new(),
            contract_scripts: HashMap::new(),
            storage_iterators: HashMap::new(),
            next_iterator_id: 1,
            current_script_hash: None,
            calling_script_hash: None,
            native_calling_override: None,
            entry_script_hash: None,
            invocation_counter: HashMap::new(),
            pending_native_calls: Vec::new(),
            native_call_boundary_contexts: Vec::new(),
            nonce_data,
            random_times: 0,
            diagnostic,
            fault_exception: None,
            native_arg_null_mask: 0,
            native_return_null: false,
        };

        app.register_native_contracts();
        app.refresh_policy_settings();
        app.register_default_interops()?;
        // Ignore any fees incurred during engine initialization (native contract setup, policy reads).
        app.fee_consumed = 0;
        app.gas_consumed = 0;

        app.diagnostic.initialized();

        Ok(app)
    }

    /// Creates a new engine with preloaded native contract state and a concrete
    /// native-contract provider type.
    ///
    /// Transaction persistence uses this path to reuse the typed provider and
    /// preloaded native cache from the `OnPersist` engine without adding local
    /// trait-object adapters in the blockchain pipeline.
    // Rationale: this constructor is the explicit test/import composition
    // seam for preloaded native state plus provider-owned execution context.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_preloaded_native_and_native_contract_provider(
        trigger: TriggerType,
        script_container: Option<Arc<VerifiableContainer>>,
        snapshot_cache: Arc<DataCache<B>>,
        persisting_block: Option<Arc<Block>>,
        protocol_settings: impl Into<Arc<ProtocolSettings>>,
        gas_limit: i64,
        contracts: HashMap<UInt160, ContractState>,
        native_contract_cache: Arc<Mutex<NativeContractsCache>>,
        diagnostic: D,
        native_contract_provider: Arc<P>,
    ) -> CoreResult<Self> {
        let protocol_settings = protocol_settings.into();
        let nonce_data =
            Self::initialize_nonce_data(script_container.as_ref(), persisting_block.as_deref());
        let original_snapshot_cache = Arc::clone(&snapshot_cache);
        let current_index = Self::engine_current_index(
            persisting_block.as_deref(),
            snapshot_cache.as_ref(),
            native_contract_provider.as_ref(),
        );
        let fee_whitelist_enabled =
            protocol_settings.is_hardfork_enabled(Hardfork::HfFaun, current_index);
        let contract_scripts = contracts
            .iter()
            .map(|(hash, contract)| (*hash, Script::new_relaxed(contract.nef.script.clone())))
            .collect();
        let contracts = contracts
            .into_iter()
            .map(|(hash, contract)| (hash, Arc::new(contract)))
            .collect();
        let jump_table = Self::select_jump_table(protocol_settings.as_ref(), current_index);
        let mut engine = ExecutionEngine::new(Some(jump_table));
        engine.set_interop_service(neo_vm::InteropService::with_capacity(
            HOST_SYSCALL_REGISTRATION_CAPACITY,
        ));
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
            fee_whitelist_enabled,
            // Safe defaults; overwritten by refresh_policy_settings().
            exec_fee_factor: 30u32 * (FEE_FACTOR as u32),
            storage_price: 100_000u32,
            call_flags: CallFlags::ALL,
            vm_engine: VmEngineHost::new(engine),
            interop_handlers: FxHashMap::with_capacity_and_hasher(
                HOST_SYSCALL_REGISTRATION_CAPACITY,
                Default::default(),
            ),
            snapshot_cache,
            original_snapshot_cache,
            notifications: Vec::new(),
            logs: Vec::new(),
            native_registry: NativeRegistry::new(),
            native_contract_provider,
            native_contract_cache,
            contracts,
            contract_scripts,
            storage_iterators: HashMap::new(),
            next_iterator_id: 1,
            current_script_hash: None,
            calling_script_hash: None,
            native_calling_override: None,
            entry_script_hash: None,
            invocation_counter: HashMap::new(),
            pending_native_calls: Vec::new(),
            native_call_boundary_contexts: Vec::new(),
            nonce_data,
            random_times: 0,
            diagnostic,
            fault_exception: None,
            native_arg_null_mask: 0,
            native_return_null: false,
        };

        app.refresh_policy_settings();
        app.register_default_interops()?;
        // Ignore any fees incurred during engine initialization.
        app.fee_consumed = 0;
        app.gas_consumed = 0;

        app.diagnostic.initialized();

        Ok(app)
    }

    /// Rebinds an existing engine for the next transaction in the same block.
    ///
    /// Keeps the jump table, interop registration, protocol settings, native
    /// provider, and native contract cache. Resets VM session state and
    /// per-transaction bookkeeping so multi-tx blocks avoid rebuilding the
    /// expensive ApplicationEngine construction path for every transaction.
    pub fn prepare_next_transaction(
        &mut self,
        script_container: Option<Arc<VerifiableContainer>>,
        snapshot_cache: Arc<DataCache<B>>,
        gas_limit: i64,
    ) {
        self.script_container = script_container;
        self.snapshot_cache = Arc::clone(&snapshot_cache);
        self.original_snapshot_cache = snapshot_cache;
        self.fee_amount = gas_limit.saturating_mul(FEE_FACTOR);
        self.fee_consumed = 0;
        self.gas_consumed = 0;
        self.call_flags = CallFlags::ALL;
        self.notifications.clear();
        self.logs.clear();
        self.fault_exception = None;
        // Keep per-block contract and script instruction caches warm across
        // multi-tx blocks. ContractManagement updates go through
        // `put_contract_cache` / `remove_contract_cache` so cache identity
        // (id + update_counter) stays correct without a full clear.
        self.storage_iterators.clear();
        self.next_iterator_id = 1;
        self.current_script_hash = None;
        self.calling_script_hash = None;
        self.native_calling_override = None;
        self.entry_script_hash = None;
        self.invocation_counter.clear();
        self.pending_native_calls.clear();
        self.native_call_boundary_contexts.clear();
        self.random_times = 0;
        self.native_arg_null_mask = 0;
        self.native_return_null = false;
        self.nonce_data =
            Self::initialize_nonce_data(self.script_container.as_ref(), self.persisting_block.as_deref());
        // Policy fees are height/protocol-stable within a block; skip the
        // Policy-contract re-read that new engine construction performs.
        self.vm_engine.engine_mut().reset_execution_session();
    }

    /// Binds this engine as the VM interop host for one callback-capable operation.
    ///
    /// The caller must clear the VM host before returning so the application
    /// engine remains movable between operations. Constructors deliberately do
    /// not bind the pointer because they return `Self` by value. The return
    /// value is `true` only when this call installed the binding; nested
    /// callback operations must leave their caller's binding in place.
    // Rationale: VM interop callbacks need one self-referential host pointer;
    // the unsafe block below documents and confines that short-lived invariant.
    #[allow(unsafe_code)]
    pub(super) fn attach_host(&mut self) -> bool {
        let host_ptr = self as *mut Self;
        if let Some(attached) = self.vm_engine.engine().interop_host_ptr() {
            debug_assert_eq!(attached, host_ptr.cast::<()>());
            return false;
        }

        // SAFETY: `host_ptr` is derived from `&mut self` and the
        // `ApplicationEngine` owns the `VmEngine`. Callers clear the binding
        // before the callback-capable operation returns, so `self` cannot move
        // while the VM may dereference the pointer.
        unsafe {
            self.vm_engine
                .engine_mut()
                .set_interop_host::<Self>(host_ptr)
        };
        true
    }

    /// Clears a host binding installed by the matching [`Self::attach_host`].
    pub(super) fn detach_host(&mut self, attached_here: bool) {
        if attached_here {
            self.vm_engine.engine_mut().clear_interop_host();
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
        name: &'static str,
        price: i64,
        call_flags: CallFlags,
        handler: InteropHandler<P, D, B>,
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
        Ok(())
    }

    /// Returns the current VM execution state.
    pub fn state(&self) -> VMState {
        self.vm_engine.engine().state()
    }

    /// Returns the number of VM instructions executed by this engine.
    pub fn instructions_executed(&self) -> u64 {
        self.vm_engine.engine().instructions_executed
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

    /// Signals that the current native method returned a VM `Null` value.
    pub fn set_native_return_null(&mut self) {
        self.native_return_null = true;
    }

    /// Returns whether native-call argument `index` was originally VM `Null`.
    ///
    /// Native handlers receive serialized byte vectors, where nullable values
    /// can otherwise be ambiguous. The dispatcher owns this bitmask and clears
    /// it at the end of every call, including failed calls.
    #[must_use]
    pub fn native_arg_is_null(&self, index: usize) -> bool {
        index < u32::BITS as usize && self.native_arg_null_mask & (1u32 << index) != 0
    }

    pub(crate) fn begin_native_call(&mut self, null_mask: u32) {
        self.native_arg_null_mask = null_mask;
        self.native_return_null = false;
    }

    pub(crate) fn finish_native_call(&mut self) -> bool {
        self.native_arg_null_mask = 0;
        std::mem::take(&mut self.native_return_null)
    }

    /// Records the VM state for a transaction in the ledger state tracker.
    pub fn record_transaction_vm_state(&mut self, hash: &UInt256, vm_state: VMState) -> bool {
        // LedgerTransactionStates is owned by neo-native-contracts; we don't
        // have a direct reference to it from neo-execution. Return false for
        // now; this is reactivated when the state is passed through a typed
        // ledger-state provider.
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
    pub fn invocation_stack(&self) -> &[ExecutionContext<B>] {
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
        // Prefer VM-synced flags (updated on context load/unload and every
        // load_script_with_state) so the syscall hot path avoids locking
        // ExecutionContextState on every System.* invocation.
        self.vm_engine.engine().has_call_flags(required)
    }

    /// Returns the call flags of the current execution context.
    pub fn get_current_call_flags(&self) -> VmResult<CallFlags> {
        Ok(self.vm_engine.engine().call_flags())
    }

    /// Returns the execution state of the current context.
    pub fn current_execution_state(&self) -> VmResult<Arc<Mutex<ExecutionContextState<B>>>> {
        let context = self
            .vm_engine
            .engine()
            .current_context()
            .ok_or_else(|| VmError::invalid_operation_msg("No current execution context"))?;
        Ok(context.state())
    }

    /// Returns the index of the block currently being persisted.
    pub fn current_block_index(&self) -> u32 {
        if let Some(block) = self.persisting_block.as_ref() {
            return block.header.index();
        }

        self.native_contract_provider()
            .current_block_index(self.snapshot_cache.as_ref())
            .unwrap_or(0)
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
        self.protocol_settings.as_ref()
    }

    /// Returns the script container associated with this execution, if any.
    pub fn script_container(&self) -> Option<&Arc<VerifiableContainer>> {
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
        self.logs.push(event);
    }

    /// Records a notification event emitted by runtime interop.
    pub fn push_notification(&mut self, event: NotifyEventArgs) {
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

    pub(super) fn native_contract_provider(&self) -> &P {
        self.native_contract_provider.as_ref()
    }

    pub(super) fn native_contract_by_hash(&self, hash: &UInt160) -> Option<P::Contract> {
        self.native_registry
            .get(hash)
            .or_else(|| self.native_contract_provider().get_native_contract(hash))
    }

    /// Returns a shared handle to the native-contract cache.
    pub fn native_contract_cache_handle(&self) -> Arc<Mutex<NativeContractsCache>> {
        Arc::clone(&self.native_contract_cache)
    }

    /// Returns a cloned snapshot of contract states known to this engine.
    pub fn contracts_snapshot(&self) -> HashMap<UInt160, ContractState> {
        self.contracts
            .iter()
            .map(|(hash, contract)| (*hash, contract.as_ref().clone()))
            .collect()
    }

    /// Returns a clone of the current snapshot cache.
    pub fn snapshot_cache(&self) -> Arc<DataCache<B>> {
        Arc::clone(&self.snapshot_cache)
    }

    pub(super) fn get_contract(&self, hash: &UInt160) -> Option<&ContractState> {
        self.contracts.get(hash).map(AsRef::as_ref)
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
    /// let engine = ApplicationEngine::new_with_native_contract_provider(...)?;
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

#[cfg(test)]
#[path = "../tests/application_engine/csharp_differential.rs"]
mod csharp_differential_tests;
