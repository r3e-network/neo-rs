use super::*;

impl InteropHost for ApplicationEngine {
    fn invoke_syscall(&mut self, engine: &mut ExecutionEngine, hash: u32) -> VmResult<()> {
        if let Some(entry) = self.interop_handlers.get(&hash).copied() {
            if entry.price > 0 {
                self.add_cpu_fee(entry.price)
                    .map_err(map_core_error_to_vm_error)?;
            }
            (entry.handler)(self, engine)
        } else {
            Err(VmError::InteropService {
                service: format!("0x{hash:08x}"),
                error: "Interop handler not registered".to_string(),
            })
        }
    }

    fn on_context_loaded(
        &mut self,
        engine: &mut ExecutionEngine,
        context: &ExecutionContext,
    ) -> VmResult<()> {
        let state_arc =
            context.get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new);
        let call_flags = state_arc.lock().call_flags;
        engine.set_call_flags(call_flags);

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
        // DEADLOCK FIX: When CALL creates a callee context via clone_with_position,
        // caller and callee share the same `states: Arc<RwLock<HashMap>>`. Looking up
        // the same TypeId (ExecutionContextState) returns the same Arc<Mutex<T>>.
        // Holding the callee's lock while trying to lock the caller's state is a
        // same-thread reentrant lock on a non-reentrant parking_lot::Mutex → deadlock.
        //
        // Fix: extract all needed values from the callee state, drop the lock, THEN
        // acquire the caller state lock.
        let state_arc =
            context.get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new);

        // Phase 1: Extract values and reset under a short-lived lock
        let (snapshot_cache, notification_count, is_dynamic_call) = {
            let mut state = state_arc.lock();
            let snapshot = state.snapshot_cache.clone();
            let notif_count = state.notification_count;
            let dynamic_call = state.is_dynamic_call;
            state.notification_count = 0;
            state.is_dynamic_call = false;
            (snapshot, notif_count, dynamic_call)
        };
        // Lock is now dropped — safe to acquire caller's state lock

        // C# only applies cross-contract unload handling when the unloaded context
        // script differs from the current context script. Cloned contexts created
        // via CALL/LoadContract(_initialize) share the same script and must bypass
        // dynamic-call return-value checks.
        let is_cross_contract_unload = engine.current_context().map_or(true, |current_ctx| {
            current_ctx.script_hash() != context.script_hash()
        });

        // Phase 2: Commit snapshot and propagate state to caller (cross-contract only)
        if is_cross_contract_unload {
            if engine.uncaught_exception().is_none() {
                if let Some(snapshot) = snapshot_cache {
                    // `DataCache` is currently shared across execution contexts. Calling `commit()`
                    // on a shared snapshot clears the global change-set and can drop pending writes
                    // before the block-level persistence pipeline flushes them to storage.
                    // Only commit when this context owns the snapshot state exclusively.
                    if std::sync::Arc::strong_count(&snapshot) == 1 {
                        snapshot.commit();
                    }
                }

                if let Some(current_ctx) = engine.current_context() {
                    let current_state_arc = current_ctx
                        .get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new);
                    let mut current_state = current_state_arc.lock();
                    current_state.notification_count = current_state
                        .notification_count
                        .saturating_add(notification_count);

                    if is_dynamic_call {
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
            } else if notification_count > 0 {
                if notification_count >= self.notifications.len() {
                    self.notifications.clear();
                } else {
                    let retain = self.notifications.len() - notification_count;
                    self.notifications.truncate(retain);
                }
            }
        }

        self.refresh_context_tracking()
            .map_err(|e| VmError::invalid_operation_msg(e.to_string()))?;

        if let Some(diagnostic) = self.diagnostic.as_mut() {
            diagnostic.context_unloaded(context);
        }

        if let Some(current_context) = engine.current_context() {
            let current_state_arc = current_context
                .get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new);
            engine.set_call_flags(current_state_arc.lock().call_flags);
        } else {
            engine.set_call_flags(CallFlags::ALL);
        }

        Ok(())
    }

    fn pre_execute_instruction(
        &mut self,
        _engine: &mut ExecutionEngine,
        _context: &ExecutionContext,
        instruction: &Instruction,
    ) -> VmResult<()> {
        let opcode_price = Self::get_opcode_price(instruction.opcode as u8);
        if opcode_price > 0 {
            self.add_cpu_fee(opcode_price)
                .map_err(map_core_error_to_vm_error)?;
        }

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
            let state = state_arc.lock();
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
