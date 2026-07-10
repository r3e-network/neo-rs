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

impl<P, D, B> ApplicationEngine<P, D, B>
where
    P: crate::native_contract_provider::NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    pub(crate) fn should_trace_block(block_idx: u32) -> bool {
        trace_block_range()
            .map(|(start, end)| block_idx >= start && block_idx <= end)
            .unwrap_or(false)
    }
    pub(super) fn fetch_contract(&mut self, hash: &UInt160) -> CoreResult<ContractState> {
        if let Some(contract) = self.contracts.get(hash) {
            return Ok(contract.clone());
        }

        let block_idx = self.persisting_block().map(|b| b.index()).unwrap_or(0);
        let diag = Self::should_trace_block(block_idx);

        match self.lookup_contract_management_state(hash) {
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
                return Err(CoreError::invalid_operation(e.to_string()));
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
        Err(CoreError::not_found(format!(
            "Contract not found: {hash:?}"
        )))
    }

    fn is_contract_blocked(&mut self, contract_hash: &UInt160) -> CoreResult<bool> {
        let provider = self.native_contract_provider().ok_or_else(|| {
            CoreError::invalid_operation(
                "PolicyContract lookup requires a native contract provider",
            )
        })?;
        provider.policy_is_blocked(self.snapshot_cache.as_ref(), contract_hash)
    }

    fn lookup_contract_management_state(
        &self,
        hash: &UInt160,
    ) -> CoreResult<Option<ContractState>> {
        let Some(provider) = self.native_contract_provider() else {
            return Ok(None);
        };
        provider.contract_state(self.snapshot_cache.as_ref(), hash)
    }

    pub(super) fn whitelisted_fee_for_policy(
        &self,
        contract_hash: &UInt160,
        method: &str,
        param_count: u32,
    ) -> CoreResult<Option<i64>> {
        let Some(provider) = self.native_contract_provider() else {
            return Ok(None);
        };
        provider.policy_whitelisted_fee(
            self.snapshot_cache.as_ref(),
            contract_hash,
            method,
            param_count,
        )
    }

    /// Refreshes the engine's per-tx contract cache after ContractManagement
    /// mutates a contract record. Without this, a queued `_deploy` invocation
    /// would re-fetch the OLD cached `ContractState` and execute the previous
    /// NEF/manifest, producing different `_initialize` static fields than C#.
    pub fn put_contract_cache(&mut self, hash: UInt160, contract: ContractState) {
        self.contracts.insert(hash, contract);
    }

    // Rationale: loading a contract context mirrors the C# VM call-frame
    // transition and keeps the protocol fields explicit at the boundary.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn load_contract_context(
        &mut self,
        contract: ContractState,
        method: ContractMethodDescriptor,
        flags: CallFlags,
        argument_count: usize,
        previous_context: Option<ExecutionContext<B>>,
        previous_hash: Option<UInt160>,
        has_return_value: bool,
    ) -> CoreResult<ExecutionContext<B>> {
        if method.offset < 0 {
            return Err(CoreError::invalid_operation(
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
                return Err(CoreError::invalid_operation(
                    "Initialization method offset cannot be negative".to_string(),
                ));
            }

            let init_context = context.clone_with_position(init.offset as usize);
            self.vm_engine
                .engine_mut()
                .load_context(init_context)
                .map_err(|e| CoreError::invalid_operation(e.to_string()))?;
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
    ) -> CoreResult<ExecutionContext<B>> {
        if self.is_contract_blocked(&contract.hash)? {
            return Err(CoreError::invalid_operation(format!(
                "The contract {} has been blocked.",
                contract.hash
            )));
        }

        if args.len() != method.parameters.len() {
            return Err(CoreError::invalid_operation(format!(
                "Method '{}' expects {} arguments but received {}.",
                method.name,
                method.parameters.len(),
                args.len()
            )));
        }

        if has_return_value != (method.return_type != ContractParameterType::Void) {
            return Err(CoreError::invalid_operation(
                "The return value type does not match.".to_string(),
            ));
        }

        let previous_context = self
            .vm_engine
            .engine()
            .current_context()
            .cloned()
            .ok_or_else(|| CoreError::invalid_operation("No current execution context"))?;

        let state_arc = previous_context.state();
        let (calling_flags, executing_contract, previous_hash_from_state) = {
            let state = state_arc.lock();
            (state.call_flags, state.contract.clone(), state.script_hash)
        };
        let previous_hash = previous_hash_from_state
            .or_else(|| UInt160::from_bytes(&previous_context.script_hash()).ok())
            .ok_or_else(|| {
                CoreError::invalid_operation("Invalid script hash in execution context")
            })?;

        if method.safe {
            flags.remove(CallFlags::WRITE_STATES | CallFlags::ALLOW_NOTIFY);
        } else {
            let executing_contract = if self.is_hardfork_enabled(Hardfork::HfDomovoi) {
                executing_contract
            } else {
                self.lookup_contract_management_state(&previous_hash)
                    .ok()
                    .flatten()
            };

            if let Some(executing_contract) = executing_contract {
                if !executing_contract.manifest.can_call(
                    &contract.manifest,
                    &contract.hash,
                    &method.name,
                ) {
                    return Err(CoreError::invalid_operation(format!(
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
            self.whitelisted_fee_for_policy(
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
            let state_arc = new_context.state();
            state_arc.lock().whitelisted = true;
        }

        {
            let engine = self.vm_engine.engine_mut();
            let context_mut = engine
                .current_context_mut()
                .ok_or_else(|| CoreError::invalid_operation("No current execution context"))?;
            for arg in args.iter().rev() {
                context_mut
                    .push(arg.clone())
                    .map_err(|err| CoreError::invalid_operation(err.to_string()))?;
            }
        }

        self.refresh_context_tracking()?;

        Ok(new_context)
    }

    /// Dynamically calls a deployed contract method with the supplied call flags and arguments.
    pub fn call_contract_dynamic(
        &mut self,
        contract_hash: &UInt160,
        method: &str,
        call_flags: CallFlags,
        args: Vec<StackItem>,
    ) -> CoreResult<()> {
        let block_idx = self.persisting_block().map(|b| b.index()).unwrap_or(0);
        let diag = Self::should_trace_block(block_idx);

        if method.starts_with('_') {
            return Err(CoreError::invalid_operation(format!(
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
                CoreError::invalid_operation(format!(
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

        let state_arc = context.state();
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
    ) -> CoreResult<()> {
        let contract = self.fetch_contract(contract_hash)?;
        let method_descriptor = contract
            .manifest
            .abi
            .get_method_ref(method, args.len())
            .cloned()
            .ok_or_else(|| {
                CoreError::invalid_operation(format!(
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

        let state_arc = context.state();
        state_arc.lock().native_calling_script_hash = Some(*calling_script_hash);
        let boundary_id = Arc::as_ptr(&state_arc) as usize;
        self.native_call_boundary_contexts.push(boundary_id);

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
    ) -> CoreResult<StackItem> {
        // Depth of the native frame (the context whose System.Contract.CallNative
        // syscall is executing). The callee is loaded above it and run until the
        // invocation stack returns to this depth.
        let target_depth = self.vm_engine.engine().invocation_stack().len();
        if target_depth == 0 {
            return Err(CoreError::invalid_operation(
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
                CoreError::invalid_operation(format!(
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

        let state_arc = context.state();
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
                        CoreError::invalid_operation(format!(
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
            return Err(CoreError::invalid_operation(message));
        }

        let depth_after = self.vm_engine.engine().invocation_stack().len();
        if depth_after != target_depth {
            // Engine invariant: the boundary hook faults escaped exceptions, so
            // the loop can only legitimately stop at the native frame's depth.
            let message = format!(
                "Contract call from a native frame to {contract_hash}::{method} unwound past the native frame"
            );
            self.vm_engine
                .engine_mut()
                .set_uncaught_exception(Some(StackItem::from_byte_string(
                    message.clone().into_bytes(),
                )));
            self.vm_engine.engine_mut().set_state(VMState::FAULT);
            self.fault_exception = Some(message.clone());
            return Err(CoreError::invalid_operation(message));
        }

        // The callee returned: its RET moved exactly one item (`rvcount = 1`)
        // onto the native frame's evaluation stack.
        self.pop()
    }

    /// Calls a void method on `contract_hash` from a native contract context
    /// and drives the VM until the callee frame unwinds.
    ///
    /// This is the void counterpart of
    /// [`Self::call_from_native_contract_returning`] and mirrors C#'s
    /// non-generic `ApplicationEngine.CallFromNativeContractAsync`: the callee
    /// is loaded with `hasReturnValue: false`, faults cross the native boundary,
    /// and the native caller resumes only after the callee context has returned.
    pub fn call_from_native_contract_void(
        &mut self,
        calling_script_hash: &UInt160,
        contract_hash: &UInt160,
        method: &str,
        args: Vec<StackItem>,
    ) -> CoreResult<()> {
        let target_depth = self.vm_engine.engine().invocation_stack().len();
        if target_depth == 0 {
            return Err(CoreError::invalid_operation(
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
                CoreError::invalid_operation(format!(
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
            false,
            &args,
        )?;

        let state_arc = context.state();
        state_arc.lock().native_calling_script_hash = Some(*calling_script_hash);
        self.refresh_context_tracking()?;

        if self.vm_engine.engine().interop_service().is_none() {
            let mut service = neo_vm::interop_service::InteropService::new();
            for (name, price, flags) in &self.host_syscall_registrations {
                service
                    .register_host_descriptor(name, *price, *flags)
                    .map_err(|e| {
                        CoreError::invalid_operation(format!(
                            "rebuilding the interop registry for a nested native call: {e}"
                        ))
                    })?;
            }
            self.vm_engine.engine_mut().set_interop_service(service);
        }

        let boundary_id = Arc::as_ptr(&state_arc) as usize;
        self.native_call_boundary_contexts.push(boundary_id);

        let vm_state = self.execute_until_invocation_stack_depth(target_depth);

        self.native_call_boundary_contexts
            .retain(|id| *id != boundary_id);

        if vm_state == VMState::FAULT {
            let message = self.fault_exception.clone().unwrap_or_else(|| {
                format!("Contract call from a native frame to {contract_hash}::{method} faulted")
            });
            return Err(CoreError::invalid_operation(message));
        }

        let depth_after = self.vm_engine.engine().invocation_stack().len();
        if depth_after != target_depth {
            let message = format!(
                "Contract call from a native frame to {contract_hash}::{method} unwound past the native frame"
            );
            self.vm_engine
                .engine_mut()
                .set_uncaught_exception(Some(StackItem::from_byte_string(
                    message.clone().into_bytes(),
                )));
            self.vm_engine.engine_mut().set_state(VMState::FAULT);
            self.fault_exception = Some(message.clone());
            return Err(CoreError::invalid_operation(message));
        }

        Ok(())
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
    pub fn process_pending_native_calls(&mut self) -> CoreResult<()> {
        if self.pending_native_calls.is_empty() {
            return Ok(());
        }

        let pending = std::mem::take(&mut self.pending_native_calls);
        // Contract calls are loaded as VM contexts on a LIFO invocation stack.
        // Loading queued calls in reverse order makes execution observe the
        // queue in FIFO order.
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
#[path = "../tests/application_engine/contracts.rs"]
mod tests;
