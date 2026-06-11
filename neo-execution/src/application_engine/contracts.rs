use super::*;
use neo_primitives::ContractBasicMethod;
use std::sync::OnceLock;

fn trace_block_range() -> Option<(u32, u32)> {
    static RANGE: OnceLock<Option<(u32, u32)>> = OnceLock::new();
    *RANGE.get_or_init(|| {
        std::env::var("NEO_TRACE_BLOCK").ok().and_then(|raw| {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return None;
            }
            if let Some((start, end)) = trimmed.split_once('-') {
                let start = start.trim().parse::<u32>().ok()?;
                let end = end.trim().parse::<u32>().ok()?;
                Some((start, end))
            } else {
                let block = trimmed.parse::<u32>().ok()?;
                Some((block, block))
            }
        })
    })
}

impl ApplicationEngine {
    pub(crate) fn should_trace_block(block_idx: u32) -> bool {
        trace_block_range()
            .map(|(start, end)| block_idx >= start && block_idx <= end)
            .unwrap_or(false)
    }
    pub(super) fn fetch_contract(&mut self, hash: &UInt160) -> Result<ContractState> {
        if let Some(contract) = self.contracts.get(hash) {
            return Ok(contract.clone());
        }

        let block_idx = self.persisting_block().map(|b| b.index()).unwrap_or(0);
        let diag = Self::should_trace_block(block_idx);

        match crate::native_contract_provider::lookup_contract_management(self.snapshot_cache.as_ref(), hash) {
            Ok(Some(contract)) => {
                if diag {
                    tracing::warn!(target: "neo", block_index = block_idx, %hash, id = contract.id, "TRACE: fetch_contract found in snapshot");
                }
                self.contracts.insert(*hash, contract.clone());
                return Ok(contract);
            }
            Ok(None) => {
                if diag {
                    tracing::warn!(target: "neo", block_index = block_idx, %hash, "TRACE: fetch_contract NOT found in snapshot (None)");
                }
            }
            Err(e) => {
                if diag {
                    tracing::warn!(target: "neo", block_index = block_idx, %hash, error = %e, "TRACE: fetch_contract snapshot error");
                }
                return Err(Error::invalid_operation(e.to_string()));
            }
        }

        if let Some(native) = self.native_registry.get(hash) {
            let block_height = self.current_block_index();
            if let Some(contract) = native.contract_state(&self.protocol_settings, block_height) {
                if diag {
                    tracing::warn!(target: "neo", block_index = block_idx, %hash, id = contract.id, "TRACE: fetch_contract found as native");
                }
                self.contracts.insert(*hash, contract.clone());
                return Ok(contract);
            }
        }

        if diag {
            tracing::warn!(target: "neo", block_index = block_idx, %hash, "TRACE: fetch_contract FAILED - not found anywhere");
        }
        Err(Error::not_found(format!("Contract not found: {hash:?}")))
    }

    fn is_contract_blocked(&mut self, contract_hash: &UInt160) -> Result<bool> {
        crate::native_contract_provider::is_contract_blocked_by_policy(self.snapshot_cache.as_ref(), contract_hash)
    }

    /// Refreshes the engine's per-tx contract cache after ContractManagement
    /// mutates a contract record. Without this, a queued `_deploy` invocation
    /// would re-fetch the OLD cached `ContractState` and execute the previous
    /// NEF/manifest, producing different `_initialize` static fields than C#.
    pub fn put_contract_cache(&mut self, hash: UInt160, contract: ContractState) {
        self.contracts.insert(hash, contract);
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn load_contract_context(
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

        let context = self.load_script_with_state(script_bytes, rvcount, offset, move |state| {
            state.call_flags = flags;
            // UInt160 is Copy — read the hash before moving contract_clone below.
            state.script_hash = Some(contract_clone.hash);
            state.contract = Some(contract_clone);
            state.method_name = Some(method_clone.name.clone());
            state.argument_count = argument_count;
            state.return_type = Some(method_clone.return_type);
            state.parameter_types = method_clone
                .parameters
                .iter()
                .map(|param| param.param_type)
                .collect();
            state.native_calling_script_hash = None;
            state.is_dynamic_call = false;
            state.calling_context = prev_context_clone.clone();
            state.calling_script_hash = prev_hash;
        })?;

        if let Some(init) = contract.manifest.abi.get_method_ref(
            ContractBasicMethod::INITIALIZE,
            ContractBasicMethod::INITIALIZE_P_COUNT as usize,
        ) {
            if init.offset < 0 {
                return Err(Error::invalid_operation(
                    "Initialization method offset cannot be negative".to_string(),
                ));
            }

            let init_context = context.clone_with_position(init.offset as usize);
            self.vm_engine
                .engine_mut()
                .load_context(init_context)
                .map_err(|e| Error::invalid_operation(e.to_string()))?;
            self.refresh_context_tracking()?;
        }

        Ok(context)
    }

    pub(super) fn call_contract_internal(
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
            .ok_or_else(|| Error::invalid_operation("No current execution context"))?;

        let state_arc = previous_context
            .get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new);
        let (calling_flags, executing_contract, previous_hash_from_state) = {
            let state = state_arc.lock();
            (state.call_flags, state.contract.clone(), state.script_hash)
        };
        let previous_hash = previous_hash_from_state
            .or_else(|| UInt160::from_bytes(&previous_context.script_hash()).ok())
            .ok_or_else(|| Error::invalid_operation("Invalid script hash in execution context"))?;

        if method.safe {
            flags.remove(CallFlags::WRITE_STATES | CallFlags::ALLOW_NOTIFY);
        } else {
            let executing_contract = if self.is_hardfork_enabled(Hardfork::HfDomovoi) {
                executing_contract
            } else {
                crate::native_contract_provider::lookup_contract_management(
                    self.snapshot_cache.as_ref(),
                    &previous_hash,
                )
                .ok()
                .flatten()
            };

            if let Some(executing_contract) = executing_contract {
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
        }

        flags &= calling_flags;

        let whitelisted_fixed_fee = if self
            .protocol_settings
            .is_hardfork_enabled(Hardfork::HfFaun, self.current_block_index())
        {
            crate::native_contract_provider::get_whitelisted_fee_for_policy(
                self.snapshot_cache.as_ref(),
                &contract.hash,
                &method.name,
                method.parameters.len() as u32,
            )?
        } else {
            None
        };

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

        if let Some(fixed_fee) = whitelisted_fixed_fee {
            self.add_fee_datoshi(fixed_fee)?;
            let state_arc = new_context
                .get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new);
            state_arc.lock().whitelisted = true;
        }

        {
            let engine = self.vm_engine.engine_mut();
            let context_mut = engine
                .current_context_mut()
                .ok_or_else(|| Error::invalid_operation("No current execution context"))?;
            for arg in args.iter().rev() {
                context_mut
                    .push(arg.clone())
                    .map_err(|err| Error::invalid_operation(err.to_string()))?;
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
        let block_idx = self.persisting_block().map(|b| b.index()).unwrap_or(0);
        let diag = Self::should_trace_block(block_idx);

        if method.starts_with('_') {
            return Err(Error::invalid_operation(format!(
                "Method name '{}' cannot start with underscore.",
                method
            )));
        }

        let contract = self.fetch_contract(contract_hash)?;
        if diag {
            let caller = self
                .get_calling_script_hash()
                .map(|h| h.to_string())
                .unwrap_or_else(|| "none".to_string());
            let current = self
                .current_script_hash()
                .map(|h| h.to_string())
                .unwrap_or_else(|| "none".to_string());
            tracing::warn!(target: "neo", block_index = block_idx, %contract_hash, method, args_len = args.len(), caller, current, "TRACE: call_contract_dynamic");
        }
        let method_descriptor = contract
            .manifest
            .abi
            .get_method_ref(method, args.len())
            .cloned()
            .ok_or_else(|| {
                let available: Vec<String> = contract.manifest.abi.methods.iter()
                    .map(|m| format!("{}({})", m.name, m.parameters.len()))
                    .collect();
                if diag {
                    tracing::warn!(target: "neo", block_index = block_idx, %contract_hash, method, args_len = args.len(), ?available, "TRACE: method NOT FOUND in ABI");
                }
                Error::invalid_operation(format!(
                    "Method '{}' with {} parameter(s) doesn't exist in the contract {:?}.",
                    method,
                    args.len(),
                    contract_hash
                ))
            })?;

        if diag {
            tracing::warn!(target: "neo", block_index = block_idx, %contract_hash, method, offset = method_descriptor.offset, "TRACE: method found, calling internal");
        }
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
        state_arc.lock().is_dynamic_call = true;

        Ok(())
    }

    /// Calls a contract from a native contract context with the calling hash set
    /// to the native contract.
    ///
    /// This mirrors C# `ApplicationEngine.CallFromNativeContractAsync` by:
    /// - Using `CallFlags::ALL`
    /// - Bypassing the underscore method-name restriction
    /// - Setting `native_calling_script_hash` on the new context
    ///
    /// The call is loaded immediately; execution continues when the VM resumes.
    pub fn call_from_native_contract_dynamic(
        &mut self,
        calling_script_hash: &UInt160,
        contract_hash: &UInt160,
        method: &str,
        args: Vec<StackItem>,
    ) -> Result<()> {
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
            CallFlags::ALL,
            has_return_value,
            &args,
        )?;

        let state_arc =
            context.get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new);
        state_arc.lock().native_calling_script_hash = Some(*calling_script_hash);

        // Refresh cached hashes so `GetCallingScriptHash` sees the native caller.
        self.refresh_context_tracking()?;

        Ok(())
    }

    /// Calls `method` on `contract_hash` from inside a currently-executing
    /// native method, drives the VM until the callee frame unwinds, and returns
    /// the callee's result.
    ///
    /// This is the awaitable counterpart of
    /// [`Self::queue_contract_call_from_native`] and ports C#
    /// `ApplicationEngine.CallFromNativeContractAsync<T>` (ApplicationEngine.cs):
    /// C# loads the callee with `CallFlags.All` and `hasReturnValue: true`, tags
    /// the new context with `ExecutionContextState.NativeCallingScriptHash`,
    /// registers it in `contractTasks`, suspends the native method on an
    /// awaiter, and resumes it with the return value when the context unloads.
    /// Rust natives are synchronous functions holding `&mut ApplicationEngine`,
    /// so instead of suspending, this method re-enters the VM step loop in
    /// place (`execute_until_invocation_stack_depth`) until the callee unwinds
    /// back to the native frame — same execution order, same observables:
    ///
    /// * **Fees** accrue into the same engine budget (the callee's opcode and
    ///   syscall fees charge the shared `fee_consumed` counters, hitting the
    ///   gas limit at exactly the same point as C#'s single engine).
    /// * **Notifications** append to the same engine list in emission order,
    ///   with the existing per-context rollback when an exception is caught.
    /// * **Snapshot**: the callee writes the engine's live snapshot, so its
    ///   effects are visible to the native afterwards exactly like a committed
    ///   C# per-context clone.
    /// * **Calling hash**: the callee observes `calling_script_hash ==
    ///   calling_script_hash` (the `NativeCallingScriptHash` rule), which is
    ///   how e.g. `PolicyContract.recoverFund` authorizes NEP-17 sweeps via the
    ///   `from == CallingScriptHash` witness bypass.
    /// * **Faults**: a callee fault — or an exception that crosses the native
    ///   boundary — faults the whole engine. The callee root is registered as a
    ///   native-call boundary; `on_context_unloaded` errors when it unloads
    ///   with an uncaught exception (C# throws `VMUnhandledException` from
    ///   `ContextUnloaded` for `contractTasks` members), so a TRY in any frame
    ///   below the native call can never catch it.
    ///
    /// Unlike the queued variant, the callee must declare a non-`Void` return
    /// type: C# passes `hasReturnValue: true` and `CallContractInternal` throws
    /// "The return value type does not match." otherwise
    /// (`call_contract_internal` enforces the identical check).
    pub fn call_from_native_contract_returning(
        &mut self,
        calling_script_hash: &UInt160,
        contract_hash: &UInt160,
        method: &str,
        args: Vec<StackItem>,
    ) -> Result<StackItem> {
        // Depth of the native frame (the context whose System.Contract.CallNative
        // syscall is executing). The callee is loaded above it and run until the
        // invocation stack returns to this depth.
        let target_depth = self.vm_engine.engine().invocation_stack().len();
        if target_depth == 0 {
            return Err(Error::invalid_operation(
                "A contract call from a native frame requires a live execution context",
            ));
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

        let context = self.call_contract_internal(
            &contract,
            &method_descriptor,
            CallFlags::ALL,
            true,
            &args,
        )?;

        let state_arc =
            context.get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new);
        state_arc.lock().native_calling_script_hash = Some(*calling_script_hash);
        // Refresh cached hashes so the callee observes the native caller.
        self.refresh_context_tracking()?;

        // The VM's `on_syscall` takes the `InteropService` out of the engine
        // while a syscall handler runs (the System.Contract.CallNative frame we
        // are inside of), so the nested steps below would find no registry.
        // Install an equivalent one rebuilt from the recorded registrations;
        // the outer `on_syscall` frame restores the original (overwriting this
        // temporary) when the native method returns.
        if self.vm_engine.engine().interop_service().is_none() {
            let mut service = neo_vm::interop_service::InteropService::new();
            for (name, price, flags) in &self.host_syscall_registrations {
                service
                    .register_host_descriptor(name, *price, *flags)
                    .map_err(|e| {
                        Error::invalid_operation(format!(
                            "rebuilding the interop registry for a nested native call: {e}"
                        ))
                    })?;
            }
            self.vm_engine.engine_mut().set_interop_service(service);
        }

        // Register the callee root as a native-call boundary (the C#
        // `contractTasks` key). `context` stays alive across the loop, so the
        // pointer identity cannot be reused while registered.
        let boundary_id = Arc::as_ptr(&state_arc) as usize;
        self.native_call_boundary_contexts.push(boundary_id);

        let vm_state = self.execute_until_invocation_stack_depth(target_depth);

        self.native_call_boundary_contexts
            .retain(|id| *id != boundary_id);

        if vm_state == VMState::FAULT {
            let message = self.fault_exception.clone().unwrap_or_else(|| {
                format!("Contract call from a native frame to {contract_hash}::{method} faulted")
            });
            return Err(Error::invalid_operation(message));
        }

        let depth_after = self.vm_engine.engine().invocation_stack().len();
        if depth_after != target_depth {
            // Engine invariant: the boundary hook faults escaped exceptions, so
            // the loop can only legitimately stop at the native frame's depth.
            let message = format!(
                "Contract call from a native frame to {contract_hash}::{method} unwound past the native frame"
            );
            self.vm_engine.engine_mut().set_uncaught_exception(Some(
                StackItem::from_byte_string(message.clone().into_bytes()),
            ));
            self.vm_engine.engine_mut().set_state(VMState::FAULT);
            self.fault_exception = Some(message.clone());
            return Err(Error::invalid_operation(message));
        }

        // The callee returned: its RET moved exactly one item (`rvcount = 1`)
        // onto the native frame's evaluation stack.
        self.pop().map_err(Error::invalid_operation)
    }

    /// Queues a contract call requested by a native contract.
    ///
    /// The queued call will be loaded after the current native syscall finishes,
    /// ensuring the native return value is pushed to the correct context. This is
    /// the faithful equivalent of C# `ApplicationEngine.CallFromNativeContractAsync`
    /// (used by NEP-17 `transfer` to invoke the recipient's `onNEP17Payment`):
    /// the call runs after the native method returns, not synchronously within it.
    pub fn queue_contract_call_from_native(
        &mut self,
        calling_script_hash: UInt160,
        contract_hash: UInt160,
        method: impl Into<String>,
        args: Vec<StackItem>,
    ) {
        self.pending_native_calls.push(PendingNativeCall {
            calling_script_hash,
            contract_hash,
            method: method.into(),
            args,
        });
    }

    /// Loads any queued native calls (FIFO order) into the VM.
    ///
    /// This is invoked by `System.Contract.CallNative` after a native method
    /// returns, and may also be called directly by tests.
    pub fn process_pending_native_calls(&mut self) -> Result<()> {
        if self.pending_native_calls.is_empty() {
            return Ok(());
        }

        let pending = std::mem::take(&mut self.pending_native_calls);
        for call in pending.into_iter().rev() {
            self.call_from_native_contract_dynamic(
                &call.calling_script_hash,
                &call.contract_hash,
                &call.method,
                call.args,
            )?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_manifest::{
        ContractAbi, ContractManifest, ContractMethodDescriptor,
        ContractParameterDefinition, NefFile, ContractPermission,
        WildCardContainer,
    };
    use neo_primitives::ContractParameterType;
    use neo_vm_rs::OpCode;
    use std::collections::HashMap;
    use parking_lot::Mutex as PlMutex;

    /// Builds a small synthetic contract with a single `balanceOf(account)`
    /// method that returns immediately. Used by the dynamic-call tests so
    /// they do not depend on the GAS native contract being installed via
    /// the global `NativeContractProvider`.
    fn build_mock_contract(hash: UInt160) -> ContractState {
        let script = vec![OpCode::RET.byte()];
        let nef = NefFile::new("test".to_string(), script);

        let param = ContractParameterDefinition::new(
            "account".to_string(),
            ContractParameterType::Hash160,
        )
        .expect("parameter");

        let method = ContractMethodDescriptor::new(
            "balanceOf".to_string(),
            vec![param],
            ContractParameterType::Integer,
            0,
            true,
        )
        .expect("descriptor");

        let abi = ContractAbi::new(vec![method], Vec::new());

        let manifest = ContractManifest {
            name: "MockContract".to_string(),
            groups: Vec::new(),
            features: std::collections::HashMap::new(),
            supported_standards: Vec::new(),
            abi,
            permissions: vec![ContractPermission::default_wildcard()],
            trusts: WildCardContainer::default(),
            extra: None,
        };

        ContractState::new(1, hash, nef, manifest)
    }

    #[test]
    fn call_contract_uses_execution_state_script_hash_for_caller() {
        let snapshot = Arc::new(DataCache::new(false));

        // Pre-load a mock contract directly into the engine so the test
        // is self-contained (does not rely on a globally-installed
        // NativeContractProvider).
        let target_hash =
            UInt160::parse("0xa1b2c3d4e5f60718293a4b5c6d7e8f0102030405").expect("target hash");
        let mut contracts: HashMap<UInt160, ContractState> = HashMap::new();
        contracts.insert(target_hash, build_mock_contract(target_hash));

        let mut engine = ApplicationEngine::new_with_preloaded_native(
            TriggerType::Application,
            None,
            snapshot,
            None,
            ProtocolSettings::default(),
            TEST_MODE_GAS,
            contracts,
            Arc::new(PlMutex::new(NativeContractsCache::default())),
            None,
        )
        .expect("engine");

        engine
            .load_script(vec![OpCode::RET.byte()], CallFlags::ALL, None)
            .expect("load entry script");

        let entry_context = engine.current_context().cloned().expect("entry context");
        let vm_script_hash =
            UInt160::from_bytes(&entry_context.script_hash()).expect("entry vm script hash");
        let logical_contract_hash =
            UInt160::parse("0xc198d687cc67e244662c3b9c1325f095f8e663b1").expect("hash");
        assert_ne!(logical_contract_hash, vm_script_hash);

        let state_arc = entry_context
            .get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new);
        state_arc.lock().script_hash = Some(logical_contract_hash);
        engine
            .refresh_context_tracking()
            .expect("refresh context tracking");

        engine
            .call_contract_dynamic(
                &target_hash,
                "balanceOf",
                CallFlags::READ_STATES | CallFlags::ALLOW_CALL,
                vec![StackItem::from_byte_string(UInt160::zero().to_bytes())],
            )
            .expect("load mock balanceOf call");

        let called_context = engine.current_context().cloned().expect("called context");
        let called_state_arc = called_context
            .get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new);
        let called_state = called_state_arc.lock();

        assert_eq!(called_state.calling_script_hash, Some(logical_contract_hash));
        assert_eq!(
            engine.get_calling_script_hash(),
            Some(logical_contract_hash)
        );
    }

    /// Builds a synthetic contract whose single method executes `script` from
    /// offset 0. Used by the `call_from_native_contract_returning` tests.
    fn build_returning_mock(
        hash: UInt160,
        method_name: &str,
        return_type: ContractParameterType,
        script: Vec<u8>,
    ) -> ContractState {
        let nef = NefFile::new("test".to_string(), script);
        let method = ContractMethodDescriptor::new(
            method_name.to_string(),
            Vec::new(),
            return_type,
            0,
            false,
        )
        .expect("descriptor");
        let abi = ContractAbi::new(vec![method], Vec::new());
        let manifest = ContractManifest {
            name: "ReturningMock".to_string(),
            groups: Vec::new(),
            features: std::collections::HashMap::new(),
            supported_standards: Vec::new(),
            abi,
            permissions: vec![ContractPermission::default_wildcard()],
            trusts: WildCardContainer::default(),
            extra: None,
        };
        ContractState::new(2, hash, nef, manifest)
    }

    /// Builds an engine preloaded with `contracts` and an entry context (a bare
    /// RET script) standing in for the native frame the primitive is called
    /// from.
    fn engine_with_entry(contracts: HashMap<UInt160, ContractState>) -> ApplicationEngine {
        let snapshot = Arc::new(DataCache::new(false));
        let mut engine = ApplicationEngine::new_with_preloaded_native(
            TriggerType::Application,
            None,
            snapshot,
            None,
            ProtocolSettings::default(),
            TEST_MODE_GAS,
            contracts,
            Arc::new(PlMutex::new(NativeContractsCache::default())),
            None,
        )
        .expect("engine");
        engine
            .load_script(vec![OpCode::RET.byte()], CallFlags::ALL, None)
            .expect("load entry script");
        engine
    }

    /// The returning call yields the callee's result and the callee observes
    /// the supplied calling script hash (C# `NativeCallingScriptHash`): the
    /// callee script returns `System.Runtime.GetCallingScriptHash`.
    #[test]
    fn returning_call_yields_result_and_native_calling_hash() {
        let target_hash = UInt160::from_bytes(&[0xCD; 20]).expect("hash");
        let calling_hash = UInt160::from_bytes(&[0xAB; 20]).expect("hash");

        let mut script = vec![OpCode::SYSCALL.byte()];
        script.extend_from_slice(
            &neo_vm_rs::interop_hash("System.Runtime.GetCallingScriptHash").to_le_bytes(),
        );
        script.push(OpCode::RET.byte());

        let mut contracts = HashMap::new();
        contracts.insert(
            target_hash,
            build_returning_mock(target_hash, "whoCalls", ContractParameterType::Hash160, script),
        );
        let mut engine = engine_with_entry(contracts);

        let result = engine
            .call_from_native_contract_returning(&calling_hash, &target_hash, "whoCalls", vec![])
            .expect("returning call succeeds");

        assert_eq!(result.as_bytes().expect("hash bytes"), calling_hash.to_bytes());
        // The invocation stack is back at the native frame and nothing faulted.
        assert_eq!(engine.invocation_stack().len(), 1);
        assert_ne!(engine.state(), VMState::FAULT);
        // The result was consumed from the native frame's evaluation stack.
        assert_eq!(
            engine
                .current_context()
                .expect("entry context")
                .evaluation_stack()
                .len(),
            0
        );
    }

    /// C# `CallFromNativeContractAsync<T>` passes `hasReturnValue: true`, so a
    /// `Void` callee is rejected with "The return value type does not match."
    #[test]
    fn returning_call_rejects_void_method() {
        let target_hash = UInt160::from_bytes(&[0xCE; 20]).expect("hash");
        let mut contracts = HashMap::new();
        contracts.insert(
            target_hash,
            build_returning_mock(
                target_hash,
                "voidMethod",
                ContractParameterType::Void,
                vec![OpCode::RET.byte()],
            ),
        );
        let mut engine = engine_with_entry(contracts);

        let err = engine
            .call_from_native_contract_returning(
                &UInt160::zero(),
                &target_hash,
                "voidMethod",
                vec![],
            )
            .expect_err("void method must be rejected");
        assert!(
            err.to_string().contains("return value type does not match"),
            "unexpected error: {err}"
        );
    }

    /// A callee that throws (and nothing inside the callee catches) faults the
    /// whole engine — the primitive surfaces an error and the VM is FAULTed,
    /// mirroring C#'s `VMUnhandledException` for `contractTasks` contexts.
    #[test]
    fn returning_call_propagates_callee_throw_as_engine_fault() {
        let target_hash = UInt160::from_bytes(&[0xCF; 20]).expect("hash");
        let mut contracts = HashMap::new();
        contracts.insert(
            target_hash,
            build_returning_mock(
                target_hash,
                "explode",
                ContractParameterType::Integer,
                vec![OpCode::PUSH1.byte(), OpCode::THROW.byte()],
            ),
        );
        let mut engine = engine_with_entry(contracts);

        let result = engine.call_from_native_contract_returning(
            &UInt160::zero(),
            &target_hash,
            "explode",
            vec![],
        );
        assert!(result.is_err(), "callee throw must surface as an error");
        assert_eq!(engine.state(), VMState::FAULT);
    }

    /// Hash for the test-only interop the boundary test uses to invoke the
    /// primitive from inside a script (standing in for a native method).
    const BOUNDARY_TEST_SYSCALL: &str = "Test.NativeCallReturning";

    fn boundary_test_handler(
        app: &mut ApplicationEngine,
        _engine: &mut neo_vm::ExecutionEngine,
    ) -> neo_vm::VmResult<()> {
        let target_hash = UInt160::from_bytes(&[0xDF; 20]).expect("hash");
        match app.call_from_native_contract_returning(
            &UInt160::zero(),
            &target_hash,
            "explode",
            vec![],
        ) {
            Ok(_) => Err(neo_vm::VmError::invalid_operation_msg(
                "boundary test: callee unexpectedly returned",
            )),
            Err(err) => Err(neo_vm::VmError::invalid_operation_msg(err.to_string())),
        }
    }

    /// A TRY armed below the native frame cannot catch an exception escaping a
    /// returning native call: C# throws `VMUnhandledException` when the
    /// registered context unloads, before any lower TRY is consulted. The entry
    /// script arms TRY/CATCH around the call; the engine must FAULT (a broken
    /// boundary would run the CATCH and HALT with `2` on the result stack).
    #[test]
    fn returning_call_exception_cannot_be_caught_below_native_frame() {
        let target_hash = UInt160::from_bytes(&[0xDF; 20]).expect("hash");
        let mut contracts = HashMap::new();
        contracts.insert(
            target_hash,
            build_returning_mock(
                target_hash,
                "explode",
                ContractParameterType::Integer,
                vec![OpCode::PUSH1.byte(), OpCode::THROW.byte()],
            ),
        );

        let snapshot = Arc::new(DataCache::new(false));
        let mut engine = ApplicationEngine::new_with_preloaded_native(
            TriggerType::Application,
            None,
            snapshot,
            None,
            ProtocolSettings::default(),
            TEST_MODE_GAS,
            contracts,
            Arc::new(PlMutex::new(NativeContractsCache::default())),
            None,
        )
        .expect("engine");
        engine
            .register_host_service(
                BOUNDARY_TEST_SYSCALL,
                0,
                CallFlags::NONE,
                boundary_test_handler,
            )
            .expect("register test interop");

        // ip0: TRY catch=+10 (-> ip10), no finally
        // ip3: SYSCALL Test.NativeCallReturning
        // ip8: ENDTRY +4 (-> ip12)
        // ip10: PUSH2; RET            <- catch handler (must NOT run)
        // ip12: PUSH1; RET
        let mut script = vec![OpCode::TRY.byte(), 10, 0, OpCode::SYSCALL.byte()];
        script.extend_from_slice(
            &neo_vm_rs::interop_hash(BOUNDARY_TEST_SYSCALL).to_le_bytes(),
        );
        script.extend_from_slice(&[
            OpCode::ENDTRY.byte(),
            4,
            OpCode::PUSH2.byte(),
            OpCode::RET.byte(),
            OpCode::PUSH1.byte(),
            OpCode::RET.byte(),
        ]);

        engine
            .load_script(script, CallFlags::ALL, None)
            .expect("load entry script");
        let state = engine.execute_allow_fault();

        assert_eq!(
            state,
            VMState::FAULT,
            "the exception must fault the engine, not reach the CATCH"
        );
        assert_eq!(
            engine.result_stack().len(),
            0,
            "the CATCH handler must not have produced a result"
        );
    }
}
