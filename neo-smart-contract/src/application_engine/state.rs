use super::*;
use crate::error::CoreResult;

mod constructors;
mod registration;

pub(crate) use registration::InteropRegistration;

#[cfg(test)]
mod default_interop_tests;

impl ApplicationEngine {
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
    /// Records the unknown-hardfork fault raised by the native dispatch path
    /// (`Policy.activateHardfork` with an unrecognized name; C# v3.11.2
    /// `UnknownHardforkException`). Only the engine-hosted native dispatch
    /// calls this — never contract `THROW` text — mirroring the C#
    /// type-based `UnknownHardforkException.IsInstance` check that
    /// `Blockchain.Persist` rethrows to halt an outdated node.
    pub fn mark_unknown_hardfork_fault(&mut self, hardfork_name: &str) {
        self.unknown_hardfork_fault = Some(hardfork_name.to_string());
    }
    /// Returns the unknown-hardfork name recorded by the native dispatch path,
    /// if any. The ledger turns it into a fatal persistence error (the C#
    /// `Blockchain.Persist` rethrow).
    #[must_use]
    pub fn fault_unknown_hardfork_name(&self) -> Option<&str> {
        self.unknown_hardfork_fault.as_deref()
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
    /// Native OnPersist and PostPersist do not enter the VM loop. The engine
    /// is constructed in `BREAK`, while C# `ApplicationEngine.Execute` records
    /// a successful native persist as `HALT`. Leave `FAULT` and an already
    /// halted engine unchanged.
    pub fn record_native_persist_success(&mut self) {
        if self.state() == VMState::BREAK {
            self.vm_engine.engine_mut().set_state(VMState::HALT);
        }
    }
    /// Records the VM state for a transaction in the ledger state tracker.
    pub fn record_transaction_vm_state(&mut self, hash: &UInt256, vm_state: VMState) -> bool {
        if let Some(states) = self.get_state_mut::<LedgerTransactionStates>() {
            states.mark_vm_state(hash, vm_state)
        } else {
            false
        }
    }
    /// Pushes a stack item onto the evaluation stack.
    pub fn push(&mut self, item: StackItem) -> StdResult<()> {
        self.vm_engine
            .engine_mut()
            .push(item)
            .map_err(|err| err.to_string())
    }
    /// Pops a stack item from the evaluation stack.
    pub fn pop(&mut self) -> StdResult<StackItem> {
        self.vm_engine
            .engine_mut()
            .pop()
            .map_err(|err| err.to_string())
    }
    /// Peeks at a stack item at the given index without removing it.
    pub fn peek(&self, index: usize) -> StdResult<StackItem> {
        self.vm_engine
            .engine()
            .peek(index)
            .map_err(|err| err.to_string())
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
        self.native_state_reader
            .current_index(self.snapshot_cache.as_ref())
            .unwrap_or(0)
    }
    /// Selects hardfork-dependent VM semantics from the protocol settings and
    /// the block height this engine executes at. Without this, the engine used
    /// a single shift semantic regardless of the active hardfork (report R06).
    fn select_hardfork_vm_semantics(&mut self) {
        let mut limits = neo_vm::ExecutionEngineLimits::DEFAULT;
        let gorgon_enabled = self.is_hardfork_enabled(Hardfork::HfGorgon);
        limits.zero_shift_converts_to_integer = gorgon_enabled;
        limits.pre_gorgon_collection_semantics = !gorgon_enabled;
        self.vm_engine.engine_mut().set_limits(limits);
        // C# `ApplicationEngine.Create`: Huyao runs the default jump table;
        // earlier heights restore the pre-neo-vm-PR-606 SETITEM and the
        // pre-PR-575 ROLL behaviours. Like the pricing mode below, the switch
        // follows the configured Huyao height rather than Policy state.
        let jump_table = if self
            .protocol_settings
            .is_hardfork_enabled(Hardfork::HfHuyao, self.persisting_index())
        {
            JumpTable::default()
        } else {
            JumpTable::not_huyao()
        };
        self.vm_engine.engine_mut().set_jump_table(jump_table);
        self.select_pricing_mode();
    }
    /// Selects how instruction prices are charged, mirroring the C#
    /// `ApplicationEngine` constructor: before `HF_Huyao` every instruction is
    /// charged from the static price table *before* it runs, from `HF_Huyao` it is
    /// charged from the opcode's [`RunStats`](neo_vm::run_stats::RunStats)
    /// *after* it runs.
    ///
    /// The C# constructor reads `ProtocolSettings` directly here (rather than
    /// the on-chain Policy state), so the switch follows the configured Huyao
    /// height.
    fn select_pricing_mode(&mut self) {
        self.use_dynamic_pricing = self
            .protocol_settings
            .is_hardfork_enabled(Hardfork::HfHuyao, self.persisting_index());
    }
    /// Returns the timestamp of the block currently being persisted.
    pub fn current_block_timestamp(&self) -> Result<u64, String> {
        self.persisting_block
            .as_deref()
            .map(|block| block.header.timestamp())
            .ok_or_else(|| "GetTime can only be called with Application trigger.".to_string())
    }
    /// Returns the block currently being persisted, if any.
    pub fn persisting_block(&self) -> Option<&Block> {
        self.persisting_block.as_deref()
    }
    /// Returns the block currently being persisted, or an error if none.
    pub fn get_persisting_block(&self) -> Result<Block> {
        self.persisting_block()
            .cloned()
            .ok_or_else(|| Error::native_contract("No persisting block available"))
    }
    /// Checks if a hardfork is enabled at the current block height.
    pub fn is_hardfork_enabled(&self, hardfork: Hardfork) -> bool {
        self.is_hardfork_enabled_at(hardfork, self.persisting_index())
    }
    /// Returns the index of the block this engine executes against.
    ///
    /// Mirrors C# `ApplicationEngine.GetPersistingIndex()`: the block being
    /// persisted when there is one, otherwise the block after the snapshot's
    /// latest persisted one (`Ledger.CurrentIndex` already reports a persisted
    /// block, so the *persisting* index is one higher).
    pub fn persisting_index(&self) -> u32 {
        if let Some(block) = self.persisting_block.as_ref() {
            return block.header.index();
        }
        self.native_state_reader
            .current_index(self.snapshot_cache.as_ref())
            .unwrap_or(0)
            .saturating_add(1)
    }
    /// Returns the VM reference counter's current signed count.
    ///
    /// Used by Huyao handler fees that price the references created across a
    /// push (C# v3.11.2 `ReferenceCounter.Count`).
    pub(crate) fn vm_reference_count(&self) -> i64 {
        self.vm_engine.engine().reference_counter().signed_count() as i64
    }
    /// Checks if a hardfork is enabled at an explicit block height.
    ///
    /// Mirrors C# `PolicyContract.IsHardforkEnabled(settings, snapshot, hf, index)`:
    /// hardforks up to `ProtocolSettings.LastConfigManagedHardfork` are read from
    /// `ProtocolSettings` until that hardfork itself is enabled on chain, after
    /// which they come from Policy storage; later hardforks are only ever
    /// activated on chain through `Policy.activateHardfork`.
    pub fn is_hardfork_enabled_at(&self, hardfork: Hardfork, index: u32) -> bool {
        let from_policy = !hardfork.is_config_managed()
            || (index > 0
                && self
                    .protocol_settings
                    .is_hardfork_enabled(Hardfork::LAST_CONFIG_MANAGED, index - 1));
        if !from_policy {
            return self.protocol_settings.is_hardfork_enabled(hardfork, index);
        }
        self.policy_hardfork_height(hardfork)
            .is_some_and(|height| index >= height)
    }
    /// Returns the on-chain Policy activation height of a hardfork.
    ///
    /// The whole prefix-25 table is read at most once per engine, so repeated
    /// hardfork checks on the instruction hot path cost one array index.
    fn policy_hardfork_height(&self, hardfork: Hardfork) -> Option<u32> {
        let heights = self.policy_hardfork_heights.get_or_init(|| {
            let mut heights = [None; Hardfork::COUNT];
            if let Ok(entries) = self
                .native_state_reader
                .policy_hardfork_heights(self.snapshot_cache.as_ref())
            {
                for (hardfork, height) in entries {
                    heights[usize::from(hardfork.index())] = Some(height);
                }
            }
            heights
        });
        heights[usize::from(hardfork.index())]
    }
    /// Returns the trigger type for this execution.
    pub fn trigger_type(&self) -> TriggerType {
        self.trigger
    }
    /// Returns the trigger type (alias for trigger_type).
    pub fn trigger(&self) -> TriggerType {
        self.trigger
    }
    /// Returns the total GAS consumed during execution, in datoshi.
    pub fn gas_consumed(&self) -> i64 {
        self.gas_consumed
            .saturating_add(FEE_FACTOR * OPCODE_PRICE_MULTIPLIER - 1)
            .saturating_div(FEE_FACTOR * OPCODE_PRICE_MULTIPLIER)
    }
    /// Returns the total fee consumed during execution, in datoshi.
    ///
    /// Matches C# `ApplicationEngine.FeeConsumed`, which divides the femtoGAS
    /// counter by `FeeFactor * OpcodePriceMultiplier` and rounds up.
    pub fn fee_consumed(&self) -> i64 {
        self.fee_consumed
            .saturating_add(FEE_FACTOR * OPCODE_PRICE_MULTIPLIER - 1)
            .saturating_div(FEE_FACTOR * OPCODE_PRICE_MULTIPLIER)
    }
    /// Returns the raw execution fee factor (scaled into picoGAS units)
    /// cached from the Policy contract.
    #[must_use]
    pub const fn exec_fee_factor_raw(&self) -> u32 {
        self.exec_fee_factor
    }
    /// Returns the current storage price (datoshi per byte) cached from the Policy contract.
    pub fn storage_price(&self) -> u32 {
        self.storage_price
    }
    /// Returns the fault exception message if execution halted with an error.
    pub fn fault_exception(&self) -> Option<&str> {
        self.fault_exception.as_deref()
    }
    /// Returns the VM result stack left after execution.
    pub fn result_stack(&self) -> &EvaluationStack {
        self.vm_engine.engine().result_stack()
    }
    pub fn current_evaluation_stack(&self) -> Option<parking_lot::MutexGuard<'_, EvaluationStack>> {
        self.vm_engine
            .engine()
            .current_context()
            .map(|ctx| ctx.evaluation_stack())
    }
    /// Returns the protocol settings used by this engine.
    pub fn protocol_settings(&self) -> &ProtocolSettings {
        &self.protocol_settings
    }
    /// Returns the script container being executed, if any.
    pub fn script_container(&self) -> Option<&Arc<dyn Verifiable>> {
        self.script_container.as_ref()
    }
    /// Returns the VM execution limits (stack size, item size, etc.).
    pub fn execution_limits(&self) -> &ExecutionEngineLimits {
        self.vm_engine.engine().limits()
    }
    /// Replaces the VM execution limits for this engine.
    ///
    /// Intended for service-level budgets (RPC admission, probes) that must cap
    /// [`ExecutionEngineLimits::max_instructions`], and for tests that observe
    /// resource accounting. Derive the new value from [`Self::execution_limits`]
    /// so the hardfork-selected semantics selected at construction survive, and
    /// never call this mid-execution.
    pub fn set_execution_limits(&mut self, limits: ExecutionEngineLimits) {
        self.vm_engine.engine_mut().set_limits(limits);
    }
    /// Returns how many instructions this engine has executed so far.
    ///
    /// Both the canonical interpreter loop and direct native dispatch charge
    /// this counter, so it is the cross-strategy resource-accounting witness.
    pub fn instructions_executed(&self) -> u64 {
        self.vm_engine.engine().instructions_executed
    }
    /// Returns the manifest name of the contract with the given hash,
    /// falling back to the native contract's name.
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
    /// Registers an additional native contract on this engine.
    ///
    /// The standard registry only contains the 11 C# v3.10.1 native contracts;
    /// non-protocol contracts (or test doubles) can be injected here without
    /// polluting `NativeRegistry::register_standard_contracts`.
    pub fn register_native_contract(&mut self, contract: std::sync::Arc<dyn NativeContract>) {
        self.native_registry.register(contract);
    }
    /// Records a log event and notifies the attached runtime context.
    pub fn push_log(&mut self, event: LogEventArgs) {
        if let Some(context) = self.event_observer.as_ref() {
            context.notify_application_log(self, &event);
        }
        self.record_rw_log(&event.script_hash, &event.message);
        self.logs.push(event);
    }
    /// Records a notification event and notifies the attached runtime context.
    pub fn push_notification(&mut self, event: NotifyEventArgs) {
        if let Some(context) = self.event_observer.as_ref() {
            context.notify_application_notify(self, &event);
        }
        self.record_rw_notification(&event.script_hash, &event.event_name);
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
    /// Sets the event-only observer used for logging/notify callbacks.
    pub fn set_event_observer(&mut self, observer: Option<Arc<dyn EngineEventObserver>>) {
        self.event_observer = observer;
    }
    /// Attaches an observability-only profiler. `None` keeps the normal path free
    /// of per-syscall clock reads and aggregation.
    pub fn set_profiler(&mut self, profiler: Option<Arc<dyn ApplicationEngineProfiler>>) {
        self.profiler = profiler;
    }
    /// Attaches the composition-specific chain adapter to this engine only.
    pub fn attach_chain_queries(&mut self, queries: Arc<dyn NativeChainQueries>) {
        self.chain_queries = Some(queries);
    }
    /// Returns this engine to provider-free native chain fallbacks.
    pub fn detach_chain_queries(&mut self) {
        self.chain_queries = None;
    }
    /// Clones this engine's adapter handle without consulting global state.
    pub fn chain_queries(&self) -> Option<Arc<dyn NativeChainQueries>> {
        self.chain_queries.clone()
    }
    /// Returns how many times the script with the given hash has been invoked.
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
    /// Returns a shareable handle to the native-contract metadata cache.
    pub fn native_contract_cache_handle(&self) -> Arc<Mutex<NativeContractsCache>> {
        Arc::clone(&self.native_contract_cache)
    }

    /// Reuses immutable native ABI metadata across engines. Method activation
    /// is still evaluated against each engine's settings and block height.
    /// This must be called before any native dispatch on the new engine.
    #[doc(hidden)]
    pub fn with_native_contract_cache_handle(
        mut self,
        cache: Arc<Mutex<NativeContractsCache>>,
    ) -> Self {
        self.native_contract_cache = cache;
        self
    }
    /// Returns a snapshot (clone) of the deployed contract states known to this engine.
    pub fn contracts_snapshot(&self) -> HashMap<UInt160, ContractState> {
        self.contracts
            .iter()
            .map(|(hash, state)| (*hash, (**state).clone()))
            .collect()
    }
    /// Returns a cheap snapshot with shared contract states.
    ///
    /// Public for profiling; normal callers should use [`Self::contracts_snapshot`].
    #[doc(hidden)]
    pub fn contracts_shared_snapshot(&self) -> HashMap<UInt160, Arc<ContractState>> {
        self.contracts.clone()
    }
    /// Returns a clone of the current snapshot cache.
    pub fn snapshot_cache(&self) -> Arc<PersistenceDataCache> {
        Arc::clone(&self.snapshot_cache)
    }
    /// Returns the execution's original snapshot rather than a nested call snapshot.
    pub fn original_snapshot_cache(&self) -> Arc<PersistenceDataCache> {
        Arc::clone(&self.original_snapshot_cache)
    }
    pub(super) fn policy_contract(&self) -> Option<Arc<dyn NativeContract>> {
        self.native_registry.get(&self.policy_hash)
    }
    /// Returns the ordered native interfaces attached to this engine.
    pub fn native_contracts(&self) -> Vec<Arc<dyn NativeContract>> {
        self.native_registry.contracts().collect()
    }
    pub(super) fn get_contract(&self, hash: &UInt160) -> Option<&ContractState> {
        self.contracts.get(hash).map(Arc::as_ref)
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
