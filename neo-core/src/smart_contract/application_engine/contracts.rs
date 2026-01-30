use super::*;

impl ApplicationEngine {
    pub(super) fn fetch_contract(&mut self, hash: &UInt160) -> Result<ContractState> {
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

        if let Some(contract) =
            ContractManagement::get_contract_from_snapshot(self.snapshot_cache.as_ref(), hash)
                .map_err(|e| Error::invalid_operation(e.to_string()))?
        {
            self.contracts.insert(*hash, contract.clone());
            return Ok(contract);
        }

        Err(Error::not_found(format!("Contract not found: {hash:?}")))
    }

    fn is_contract_blocked(&mut self, contract_hash: &UInt160) -> Result<bool> {
        PolicyContract::new().is_blocked_snapshot(self.snapshot_cache.as_ref(), contract_hash)
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

        self.load_script_with_state(script_bytes, rvcount, offset, move |state| {
            state.call_flags = flags;
            state.contract = Some(contract_clone.clone());
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
            state.script_hash = Some(contract_clone.hash);
            state.calling_context = prev_context_clone.clone();
            state.calling_script_hash = prev_hash;
        })
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
        let previous_hash = UInt160::from_bytes(&previous_context.script_hash())
            .map_err(|e| Error::invalid_operation(format!("Invalid script hash: {e}")))?;

        let state_arc = previous_context
            .get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new);
        let (calling_flags, executing_contract) = {
            let state = state_arc.lock();
            (state.call_flags, state.contract.clone())
        };

        if method.safe {
            flags.remove(CallFlags::WRITE_STATES | CallFlags::ALLOW_NOTIFY);
        } else {
            let executing_contract = if self.is_hardfork_enabled(Hardfork::HfDomovoi) {
                executing_contract
            } else {
                ContractManagement::get_contract_from_snapshot(
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

        let mut whitelisted = false;
        if self
            .protocol_settings
            .is_hardfork_enabled(Hardfork::HfFaun, self.current_block_index())
        {
            let policy = PolicyContract::new();
            if let Some(fixed_fee) = policy.get_whitelisted_fee(
                self.snapshot_cache.as_ref(),
                &contract.hash,
                &method.name,
                method.parameters.len() as u32,
            )? {
                self.add_fee_datoshi(fixed_fee)?;
                whitelisted = true;
            }
        }

        if whitelisted {
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

    /// Queues a contract call requested by a native contract.
    ///
    /// The queued call will be loaded after the current native syscall finishes,
    /// ensuring the native return value is pushed to the correct context.
    pub(crate) fn queue_contract_call_from_native(
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
