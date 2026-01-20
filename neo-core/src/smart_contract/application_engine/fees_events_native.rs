use super::*;

impl ApplicationEngine {
    pub(crate) fn add_runtime_fee(&mut self, fee: u64) -> Result<()> {
        self.add_fee_datoshi(
            i64::try_from(fee)
                .map_err(|_| Error::invalid_operation("Fee does not fit into i64".to_string()))?,
        )
    }

    /// Adds datoshis to `FeeConsumed` / `GasConsumed`.
    pub(crate) fn add_fee_datoshi(&mut self, datoshi: i64) -> Result<()> {
        let pico_gas = datoshi
            .checked_mul(FEE_FACTOR)
            .ok_or_else(|| Error::invalid_operation("Fee multiplication overflow".to_string()))?;
        self.add_fee_pico(pico_gas)
    }

    /// Adds picoGAS to `FeeConsumed` / `GasConsumed`.
    fn add_fee_pico(&mut self, pico_gas: i64) -> Result<()> {
        if let Ok(state_arc) = self.current_execution_state() {
            if state_arc.lock().whitelisted {
                return Ok(());
            }
        }

        if pico_gas < 0 {
            return Err(Error::invalid_operation(
                "Negative gas fee is not allowed".to_string(),
            ));
        }

        let total = self
            .fee_consumed
            .checked_add(pico_gas)
            .ok_or_else(|| Error::invalid_operation("Fee addition overflow".to_string()))?;

        self.fee_consumed = total;
        self.gas_consumed = total;

        if self.fee_consumed > self.fee_amount {
            let required = (self.fee_consumed.max(0) as u64).div_ceil(FEE_FACTOR as u64);
            let available = (self.fee_amount.max(0) as u64) / (FEE_FACTOR as u64);
            return Err(Error::insufficient_gas(required, available));
        }

        Ok(())
    }

    /// Adds a fee expressed in execution units (multiplied by `ExecFeeFactor`).
    pub(crate) fn add_cpu_fee(&mut self, fee_units: i64) -> Result<()> {
        if fee_units < 0 {
            return Err(Error::invalid_operation(
                "Negative cpu fee is not allowed".to_string(),
            ));
        }
        if fee_units == 0 {
            return Ok(());
        }

        let pico_gas = fee_units
            .checked_mul(i64::from(self.exec_fee_factor))
            .ok_or_else(|| Error::invalid_operation("CPU fee overflow".to_string()))?;
        self.add_fee_pico(pico_gas)
    }

    pub fn charge_execution_fee(&mut self, fee: u64) -> Result<()> {
        self.add_fee_datoshi(
            i64::try_from(fee)
                .map_err(|_| Error::invalid_operation("Fee does not fit into i64".to_string()))?,
        )
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
        let required_pico = required_gas
            .checked_mul(FEE_FACTOR)
            .ok_or_else(|| Error::invalid_operation("Gas multiplication overflow".to_string()))?;

        if self.gas_consumed + required_pico > self.gas_limit {
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
            let mut cache = cache_arc.lock();
            cache
                .get_or_build(native.as_ref())
                .get_method(method, args.len(), &self.protocol_settings, block_height)?
                .cloned()
        }
        .ok_or_else(|| {
            Error::invalid_operation(format!(
                "Method '{}({})' not found in native contract {} at height {}",
                method,
                args.len(),
                native.name(),
                block_height
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

        let mut is_whitelisted = false;
        if self.protocol_settings.is_hardfork_enabled(Hardfork::HfFaun, block_height) {
            let policy = PolicyContract::new();
            if policy
                .get_whitelisted_fee(
                    self.snapshot_cache.as_ref(),
                    &contract_hash,
                    method,
                    args.len() as u32,
                )?
                .is_some()
            {
                is_whitelisted = true;
            }
        }

        if !is_whitelisted {
            // Charge native contract fees upfront (matches C# NativeContract.Invoke).
            if method_meta.cpu_fee != 0 {
                self.add_cpu_fee(method_meta.cpu_fee)?;
            }
            if method_meta.storage_fee != 0 {
                let storage_fee = method_meta
                    .storage_fee
                    .checked_mul(i64::from(self.storage_price))
                    .ok_or_else(|| {
                        Error::invalid_operation("Native storage fee overflow".to_string())
                    })?;
                self.add_fee_datoshi(storage_fee)?;
            }
        }

        let result = native.invoke(self, method, args)?;

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

        let pico_gas = gas
            .checked_mul(FEE_FACTOR)
            .ok_or_else(|| Error::invalid_operation("Gas multiplication overflow".to_string()))?;

        let Some(total) = self.fee_consumed.checked_add(pico_gas) else {
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
}
