use super::*;

impl ApplicationEngine {
    /// Loads a raw script into the VM, configuring call flags and optional script hash.
    pub fn load_script(
        &mut self,
        script: Vec<u8>,
        call_flags: CallFlags,
        script_hash: Option<UInt160>,
    ) -> Result<()> {
        // Match Neo N3/C# semantics: scripts loaded by the host return all
        // evaluation stack items (`rvcount = -1`) so that witness invocation
        // scripts can pass parameters to verification scripts and invocation
        // results are preserved on `ResultStack`.
        let context = self.load_script_with_state(script, -1, 0, move |state| {
            state.call_flags = call_flags;
            if let Some(hash) = script_hash {
                state.script_hash = Some(hash);
            }
        })?;

        let script_hash = UInt160::from_bytes(&context.script_hash())
            .map_err(|e| Error::invalid_operation(format!("Invalid script hash: {e}")))?;
        self.increment_invocation_counter(&script_hash);
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

    fn capture_fault_exception_from_vm(&mut self) {
        if self.fault_exception.is_some() {
            return;
        }

        let Some(exception) = self.vm_engine.engine().uncaught_exception() else {
            return;
        };

        if let Ok(bytes) = exception.as_bytes() {
            let message = String::from_utf8_lossy(&bytes).to_string();
            if !message.is_empty() {
                self.fault_exception = Some(message);
                return;
            }
        }

        self.fault_exception = Some(format!("{exception:?}"));
    }

    /// Executes the loaded scripts until the VM halts or faults, returning the resulting VM state.
    ///
    /// This mirrors the C# engine behaviour used by RPC invocation endpoints: callers can inspect
    /// `state()` / `fault_exception()` even when execution faults.
    pub fn execute_allow_fault(&mut self) -> VMState {
        // Keep the engine host pointer aligned with this instance across moves.
        self.attach_host();

        let state = self.vm_engine.engine_mut().execute();
        if state == VMState::FAULT {
            self.capture_fault_exception_from_vm();
        }
        state
    }

    /// Executes instructions until the invocation stack depth returns to `target_depth`
    /// or the VM halts/faults. Intended for native contract helpers that need to run
    /// a nested contract call synchronously.
    pub fn execute_until_invocation_stack_depth(&mut self, target_depth: usize) -> VMState {
        // Keep the engine host pointer aligned with this instance across moves.
        self.attach_host();

        loop {
            let state = self.vm_engine.engine().state();
            if state == VMState::HALT || state == VMState::FAULT {
                if state == VMState::FAULT {
                    self.capture_fault_exception_from_vm();
                }
                return state;
            }

            if self.vm_engine.engine().invocation_stack().len() <= target_depth {
                return state;
            }

            let step = self.vm_engine.engine_mut().execute_next();
            if let Err(err) = step {
                let message = err.to_string();
                self.vm_engine.engine_mut().set_uncaught_exception(Some(
                    StackItem::from_byte_string(message.clone().into_bytes()),
                ));
                self.vm_engine.engine_mut().set_state(VMState::FAULT);
                self.capture_fault_exception_from_vm();
                return VMState::FAULT;
            }
        }
    }

    /// Executes the loaded scripts until the VM halts or faults.
    pub fn execute(&mut self) -> Result<()> {
        let state = self.execute_allow_fault();
        if state == VMState::FAULT {
            let message = self
                .fault_exception()
                .unwrap_or("VM execution faulted during script verification");
            return Err(Error::invalid_operation(message.to_string()));
        }
        Ok(())
    }

    /// Adds gas to the consumed amount
    pub fn add_gas(&mut self, amount: i64) -> Result<()> {
        self.gas_consumed = self.gas_consumed.saturating_add(amount);
        if self.gas_consumed > self.gas_limit {
            return Err(Error::invalid_operation("Gas limit exceeded"));
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
        self.check_witness_hash(&committee_address)
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
            .ok_or_else(|| Error::invalid_operation("No current contract"))?;

        // 2. Get contract state to get the ID (matches C# snapshot lookup)
        let contract = ContractManagement::get_contract_from_snapshot(
            self.snapshot_cache.as_ref(),
            &contract_hash,
        )?
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
    pub(super) fn get_storage_price(&mut self) -> usize {
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
}
