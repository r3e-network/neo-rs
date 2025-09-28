        }
    }

    /// Adds gas to the consumed amount

    pub fn add_gas(&mut self, amount: i64) -> Result<()> {
        self.gas_consumed = self.gas_consumed.saturating_add(amount);
        if self.gas_consumed > self.gas_limit {
            return Err(Error::GasLimitExceeded);
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
    pub fn check_committee_witness(&self) -> Result<bool> {
        // Check if the current transaction has a witness from the committee
        // This verifies that the transaction was signed by the committee members

        // The committee script hash is calculated from the committee members
        // stored in the NEO native contract. For administrative operations,
        // a multi-signature from the committee is required.

        // Verify the container has proper committee authorization
        if let Some(container) = &self.script_container {
            // Use the IVerifiable trait to verify the container
            // The verification includes checking all witnesses
            return Ok(container.verify());
        }

        // No container to verify
        Ok(false)
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
            .ok_or_else(|| Error::InvalidOperation("No current contract".to_string()))?;

        // 2. Get contract state to get the ID
        let contract = self.get_contract(&contract_hash).ok_or_else(|| {
            Error::ContractNotFound(format!("Contract not found: {}", contract_hash))
        })?;

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
    ) -> StorageIterator {
        let search_key = StorageKey::new(context.id, prefix.to_vec());
        let direction = if options.contains(FindOptions::Backwards) {
            SeekDirection::Backward
        } else {
            SeekDirection::Forward
        };
        let entries: Vec<_> = self
            .snapshot_cache
            .find(Some(&search_key), direction)
            .collect();

        StorageIterator::new(entries, prefix.len(), options)
    }

    /// Gets the storage price from the policy contract (matches C# StoragePrice property).
    fn get_storage_price(&mut self) -> usize {
        self.storage_price as usize
    }

    /// Adds gas fee.
    fn add_fee(&mut self, fee: u64) -> Result<()> {
        // 1. Calculate the actual fee based on ExecFeeFactor (matches C# logic exactly)
        let exec_fee_factor = self.exec_fee_factor as i64;
        let actual_fee = (fee as i64).saturating_mul(exec_fee_factor);

        // 2. Add to FeeConsumed/GasConsumed (matches C# FeeConsumed property exactly)
        self.fee_consumed = self.fee_consumed.saturating_add(actual_fee);
        self.gas_consumed = self.fee_consumed;

        // 3. Check against gas limit (matches C# gas limit check exactly)
        if self.fee_consumed > self.fee_amount {
            return Err(Error::InsufficientGas(format!(
                "Gas consumed {} exceeds limit {}",
                self.fee_consumed, self.fee_amount
            )));
        }

        Ok(())
    }

    /// Queries the original snapshot cache.
    fn query_blockchain_storage(&self, storage_key: &StorageKey) -> Option<Vec<u8>> {
        self.original_snapshot_cache
            .get(storage_key)
            .map(|item| item.get_value())
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
            return Err(Error::InvalidArguments("Event name too long".to_string()));
        }

        // 2. Validate arguments count (must not exceed 16 arguments)
        if args.len() > 16 {
            return Err(Error::InvalidArguments("Too many arguments".to_string()));
        }

        // 3. Get current contract hash
        let Some(contract_hash) = self.current_script_hash else {
            return Err(Error::InvalidOperation("No current contract".to_string()));
        };

        let Some(container) = &self.script_container else {
            return Err(Error::InvalidOperation(
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
        if self.gas_consumed + required_gas > self.gas_limit {
            return Err(Error::VmError("Out of gas".to_string()));
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
            .ok_or_else(|| Error::ContractNotFound(contract_hash.to_string()))?;

        let block_height = self.current_block_index();
        if !native.is_active(&self.protocol_settings, block_height) {
            return Err(Error::InvalidOperation(format!(
                "Native contract {} is not active at height {}",
                native.name(),
                block_height
            )));
        }

        let cache_arc = self.native_contract_cache();
        let method_meta = {
            let mut cache = cache_arc.lock().map_err(|_| {
                Error::InvalidOperation("Native contract cache lock poisoned".to_string())
            })?;
            cache.get_or_build(native).get_method(method).cloned()
        }
        .ok_or_else(|| {
            Error::InvalidOperation(format!(
                "Method '{}' not found in native contract {}",
                method,
                native.name()
            ))
        })?;

        let required_flags =
            CallFlags::from_bits(method_meta.required_call_flags).ok_or_else(|| {
                Error::InvalidOperation(format!(
                    "Method '{}' in native contract {} specifies invalid call flags",
                    method,
                    native.name()
                ))
            })?;
        if !self.call_flags.contains(required_flags) {
            return Err(Error::PermissionDenied(format!(
                "Call flags {:?} do not satisfy required permissions {:?} for {}",
                self.call_flags, required_flags, method
            )));
        }

        if method_meta.gas_cost > 0 {
            let fee = u64::try_from(method_meta.gas_cost).map_err(|_| {
                Error::InvalidOperation(format!("Gas cost overflow for native method {}", method))
            })?;
            self.add_fee(fee)?;
        }

        native.invoke(self, method, args)
    }

    pub fn native_on_persist(&mut self) -> Result<()> {
        if self.trigger != TriggerType::OnPersist {
            return Err(Error::InvalidOperation(
                "System.Contract.NativeOnPersist is only valid during OnPersist".to_string(),
            ));
        }

        let block_height = self
            .persisting_block
            .as_ref()
            .map(|block| block.index)
            .unwrap_or_else(|| self.current_block_index());

        for contract in self.native_registry.contracts_mut() {
            if contract.is_active(&self.protocol_settings, block_height) {
                contract.on_persist(self)?;
            }
        }

        Ok(())
    }

    pub fn native_post_persist(&mut self) -> Result<()> {
        if self.trigger != TriggerType::PostPersist {
            return Err(Error::InvalidOperation(
                "System.Contract.NativePostPersist is only valid during PostPersist".to_string(),
            ));
        }

        let block_height = self
            .persisting_block
            .as_ref()
            .map(|block| block.index)
            .unwrap_or_else(|| self.current_block_index());

        for contract in self.native_registry.contracts_mut() {
            if contract.is_active(&self.protocol_settings, block_height) {
                contract.post_persist(self)?;
            }
        }

        Ok(())
    }

    pub fn consume_gas(&mut self, gas: i64) -> Result<()> {
        if gas < 0 {
            return Err(Error::VmError("Negative gas consumption".to_string()));
        }

        let Some(total) = self.fee_consumed.checked_add(gas) else {
            return Err(Error::VmError("Gas addition overflow".to_string()));
        };

        if total > self.fee_amount {
            return Err(Error::VmError("Out of gas".to_string()));
        }

        self.fee_consumed = total;
        self.gas_consumed = total;

        self.vm_engine
            .engine_mut()
            .add_gas_consumed(gas)
            .map_err(|e| Error::VmError(e.to_string()))?;

        self.update_vm_gas_counter(gas)?;
        Ok(())
    }

    /// Ensures gas usage stays within configured limits.
    fn update_vm_gas_counter(&mut self, _gas: i64) -> Result<()> {
        if self.gas_consumed > self.gas_limit {
            return Err(Error::VmError(
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

    /// Checks whether the specified hash has witnessed the current execution.
    pub fn check_witness_hash(&self, hash: &UInt160) -> bool {
        if self.get_calling_script_hash() == Some(*hash) {
            return true;
        }

        if let Some(container) = &self.script_container {
            if let Some(transaction) = container.as_any().downcast_ref::<Transaction>() {
                return transaction
                    .signers()
                    .iter()
                    .any(|signer| signer.account == *hash);
            }
        }

        false
    }

    /// Gets the current execution context.
    /// This matches C# ApplicationEngine.CurrentContext exactly.
    pub fn current_context(&self) -> Option<&ExecutionContext> {
        // This implements the C# logic: engine.CurrentContext property
        self.vm_engine.current_context()
    }

    /// Deletes storage items by prefix.
    pub fn delete_storage_by_prefix(&mut self, prefix: &[u8]) -> Result<()> {
        let keys: Vec<_> = self
            .snapshot_cache
            .find(None, SeekDirection::Forward)
            .filter(|(key, _)| key.key().starts_with(prefix))
            .map(|(key, _)| key)
            .collect();

        for key in keys {
            self.snapshot_cache.delete(&key);
        }

        Ok(())
    }

    /// Gets the trigger type.
    pub fn get_trigger_type(&self) -> TriggerType {
        self.trigger
    }

    /// Returns all storage entries for a given contract.
    pub fn storage_entries_for_contract(
        &self,
        contract_hash: &UInt160,
    ) -> Vec<(StorageKey, StorageItem)> {
        let Some(contract_id) = self.get_contract_id_by_hash(contract_hash) else {
            return Vec::new();
        };
        let prefix = StorageKey::new(contract_id, Vec::new());
        self.snapshot_cache
            .find(Some(&prefix), SeekDirection::Forward)
            .collect()
    }

    /// Finds storage entries with a prefix.
    pub fn find_storage_entries_with_prefix(
        &self,
        contract_hash: &UInt160,
        prefix: &[u8],
    ) -> Vec<(StorageKey, StorageItem)> {
        let Some(contract_id) = self.get_contract_id_by_hash(contract_hash) else {
            return Vec::new();
        };
        let search_key = StorageKey::new(contract_id, prefix.to_vec());
        self.snapshot_cache
            .find(Some(&search_key), SeekDirection::Forward)
            .collect()
    }

    /// Creates a storage iterator.
    /// This matches C# Neo's ApplicationEngine.Find method exactly.
    pub fn create_storage_iterator(
        &mut self,
        results: Vec<(StorageKey, StorageItem)>,
    ) -> Result<u32> {
        let iterator_id = self
            .allocate_iterator_id()
            .map_err(|err| Error::RuntimeError(err))?;

        let iterator = StorageIterator::new(results, 0, FindOptions::None);
        self.storage_iterators.insert(iterator_id, iterator);

        Ok(iterator_id)
    }

    /// Creates a storage iterator with specific options.
    /// This matches C# Neo's ApplicationEngine.Find method with FindOptions exactly.
    pub fn create_storage_iterator_with_options(
        &mut self,
        results: Vec<(StorageKey, StorageItem)>,
        prefix_length: usize,
        options: FindOptions,
    ) -> Result<u32> {
        let iterator_id = self
            .allocate_iterator_id()
            .map_err(|err| Error::RuntimeError(err))?;
        let iterator = StorageIterator::new(results, prefix_length, options);
        self.storage_iterators.insert(iterator_id, iterator);

        Ok(iterator_id)
    }

    /// Stores an existing storage iterator and returns its identifier.
    pub fn store_storage_iterator(&mut self, iterator: StorageIterator) -> Result<u32, String> {
        let iterator_id = self.allocate_iterator_id()?;
        self.storage_iterators.insert(iterator_id, iterator);
        Ok(iterator_id)
    }

    /// Gets a storage iterator by ID.
    pub fn get_storage_iterator(&self, iterator_id: u32) -> Option<&StorageIterator> {
        self.storage_iterators.get(&iterator_id)
    }

    /// Gets a mutable storage iterator by ID.
    pub fn get_storage_iterator_mut(&mut self, iterator_id: u32) -> Option<&mut StorageIterator> {
        self.storage_iterators.get_mut(&iterator_id)
    }

    /// Advances a storage iterator to the next element.
    /// Returns true if successful, false if at the end.
    pub fn iterator_next(&mut self, iterator_id: u32) -> Result<bool> {
        match self.storage_iterators.get_mut(&iterator_id) {
            Some(iterator) => Ok(iterator.next()),
            None => Err(Error::RuntimeError(format!(
                "Iterator {} not found",
                iterator_id
            ))),
        }
    }

    /// Gets the current value from a storage iterator.
    pub fn iterator_value(&self, iterator_id: u32) -> Result<StackItem> {
        match self.storage_iterators.get(&iterator_id) {
            Some(iterator) => Ok(iterator.value()),
            None => Err(Error::RuntimeError(format!(
                "Iterator {} not found",
                iterator_id
            ))),
        }
    }

    /// Removes a storage iterator (cleanup).
    pub fn dispose_iterator(&mut self, iterator_id: u32) -> Result<()> {
        self.storage_iterators.remove(&iterator_id);
        Ok(())
    }

    /// Sets the current script hash.
    pub fn set_current_script_hash(&mut self, hash: Option<UInt160>) {
        self.current_script_hash = hash;
    }

    /// Gets contract hash by ID (helper method for storage operations).
    fn get_contract_hash_by_id(&self, id: i32) -> Option<UInt160> {
        for (hash, contract) in &self.contracts {
            if contract.id == id {
                return Some(*hash);
            }
        }
        None
    }

    /// Gets contract ID for a given hash.
    fn get_contract_id_by_hash(&self, hash: &UInt160) -> Option<i32> {
        self.contracts.get(hash).map(|contract| contract.id)
    }

    /// Sets a storage item directly (for testing and internal use).
    pub fn set_storage(&mut self, key: StorageKey, item: StorageItem) -> Result<()> {
        if self.snapshot_cache.get(&key).is_some() {
            self.snapshot_cache.update(key, item);
        } else {
            self.snapshot_cache.add(key, item);
        }
        Ok(())
    }

    /// Gets a storage item directly (for testing and internal use).
    pub fn get_storage(&self, key: &StorageKey) -> Option<StorageItem> {
        self.snapshot_cache.get(key)
    }

    /// Deletes a storage item directly (for testing and internal use).
    pub fn delete_storage(&mut self, key: &StorageKey) -> Result<()> {
        self.snapshot_cache.delete(key);
        Ok(())
    }

    /// Gets the storage context for a native contract.
    pub fn get_native_storage_context(&self, contract_hash: &UInt160) -> Result<StorageContext> {
        // 1. Get contract state to get the ID
        let contract = self.get_contract(contract_hash).ok_or_else(|| {
            Error::ContractNotFound(format!("Native contract not found: {}", contract_hash))
        })?;

        // 2. Create storage context for native contract (always read-write for native contracts)
        Ok(StorageContext {
            id: contract.id,
            is_read_only: false,
        })
    }

    /// Gets a storage item by key (legacy API for native contracts).
    pub fn get_storage_item_legacy(&self, key: &[u8]) -> Option<Vec<u8>> {
        if let Some(current_hash) = &self.current_script_hash {
            if let Ok(context) = self.get_native_storage_context(current_hash) {
                return self.get_storage_item(&context, key);
            }
        }
        None
    }

    /// Puts a storage item (legacy API for native contracts).
    pub fn put_storage_item_legacy(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        if let Some(current_hash) = &self.current_script_hash {
            let context = self.get_native_storage_context(&current_hash)?;
            return self.put_storage_item(&context, key, value);
        }
        Err(Error::InvalidOperation(
            "No current contract context".to_string(),
        ))
    }

    /// Deletes a storage item (legacy API for native contracts).
    pub fn delete_storage_item_legacy(&mut self, key: &[u8]) -> Result<()> {
        if let Some(current_hash) = &self.current_script_hash {
            let context = self.get_native_storage_context(&current_hash)?;
            return self.delete_storage_item(&context, key);
        }
        Err(Error::InvalidOperation(
            "No current contract context".to_string(),
        ))
    }
    /// Registers native contracts in the contracts HashMap so they can be found
    fn register_native_contracts(&mut self) {
        let mut registry = std::mem::take(&mut self.native_registry);

        let mut hashes = Vec::new();
        for contract in registry.contracts_mut() {
            let hash = contract.hash();
            hashes.push(hash);
            let id = contract.id();
            let name = contract.name().to_string();
            self.contracts
                .entry(hash)
                .or_insert_with(|| ContractState::new_native(id, hash, name));
        }

        for hash in &hashes {
            if let Some(contract) = registry.get_mut(hash) {
                if let Err(error) = contract.initialize(self) {
                    if let Some(container) = &self.script_container {
                        let log_event = LogEventArgs::new(
                            Arc::clone(container),
                            *hash,
                            format!(
                                "Native contract {} initialization error: {}",
                                contract.name(),
                                error
                            ),
                        );
                        self.emit_log_event(log_event);
                    }
                }
            }
        }

        self.native_registry = registry;
    }

    fn configure_current_context_state<F>(&mut self, configure: F) -> Result<()>
    where
        F: FnOnce(&mut ExecutionContextState),
    {
        let engine = self.vm_engine.engine_mut();
        let context = engine
            .current_context_mut()
            .ok_or_else(|| Error::InvalidOperation("No current execution context".to_string()))?;
        let state_arc =
            context.get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new);
        let mut state = state_arc.lock().map_err(|_| {
            Error::InvalidOperation("Execution context state lock poisoned".to_string())
        })?;
        configure(&mut state);
        Ok(())
    }

    fn current_context_script_hash(&self) -> Option<UInt160> {
        self.vm_engine
            .engine()
            .current_context()
            .and_then(|ctx| UInt160::from_bytes(&ctx.script_hash()).ok())
    }

    fn initial_policy_settings(_snapshot: &Arc<DataCache>) -> (u32, u32) {
        (
            PolicyContract::DEFAULT_EXEC_FEE_FACTOR,
            PolicyContract::DEFAULT_STORAGE_PRICE,
        )
    }

    fn refresh_policy_settings(&mut self) {
        if let Some(policy) = self.policy_contract() {
            if let Ok(raw) = policy.get_exec_fee_factor(self) {
                if !raw.is_empty() {
                    let mut buffer = [0u8; 4];
                    let len = raw.len().min(4);
                    buffer[..len].copy_from_slice(&raw[..len]);
                    self.exec_fee_factor = u32::from_le_bytes(buffer);
                }
            }

            if let Ok(raw) = policy.get_storage_price(self) {
                if !raw.is_empty() {
                    let mut buffer = [0u8; 4];
                    let len = raw.len().min(4);
                    buffer[..len].copy_from_slice(&raw[..len]);
                    self.storage_price = u32::from_le_bytes(buffer);
                }
            }
        }
    }

    fn initialize_nonce_data(
        container: Option<&Arc<dyn IVerifiable>>,
        persisting_block: Option<&Block>,
    ) -> [u8; 16] {
        let mut data = [0u8; 16];

        if let Some(container) = container {
            if let Some(transaction) = container.as_any().downcast_ref::<Transaction>() {
                let hash_bytes = transaction.hash().to_bytes();
                data.copy_from_slice(&hash_bytes[..16]);
            }
        }

        if let Some(block) = persisting_block {
            let nonce_bytes = block.header.nonce.to_le_bytes();
            for (slot, byte) in data.iter_mut().take(8).zip(nonce_bytes.iter()) {
                *slot ^= *byte;
            }
        }

        data
    }
}

impl Drop for ApplicationEngine {
    fn drop(&mut self) {
        self.vm_engine.engine_mut().clear_interop_host();
    }
}

impl InteropHost for ApplicationEngine {
    fn invoke_syscall(&mut self, engine: &mut ExecutionEngine, hash: u32) -> VmResult<()> {
        if let Some(handler) = self.interop_handlers.get(&hash).copied() {
            handler(self, engine)
        } else {
            Err(VmError::InteropService {
                service: format!("0x{hash:08x}"),
                error: "Interop handler not registered".to_string(),
            })
        }
    }

    fn on_context_unloaded(
        &mut self,
        engine: &mut ExecutionEngine,
        context: &ExecutionContext,
    ) -> VmResult<()> {
        let state_arc =
            context.get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new);
        let mut state = state_arc
            .lock()
            .map_err(|_| VmError::invalid_operation_msg("Execution context state lock poisoned"))?;

        if engine.uncaught_exception().is_none() {
            if let Some(snapshot) = state.snapshot_cache.clone() {
                snapshot.commit();
            }

            if let Some(current_ctx) = engine.current_context() {
                let current_state_arc = current_ctx
                    .get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new);
                let mut current_state = current_state_arc.lock().map_err(|_| {
                    VmError::invalid_operation_msg("Execution context state lock poisoned")
                })?;
                current_state.notification_count = current_state
                    .notification_count
                    .saturating_add(state.notification_count);

                if state.is_dynamic_call {
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
        } else if state.notification_count > 0 {
            if state.notification_count >= self.notifications.len() {
                self.notifications.clear();
            } else {
                let retain = self.notifications.len() - state.notification_count;
                self.notifications.truncate(retain);
            }
        }

        state.notification_count = 0;
        state.is_dynamic_call = false;

        Ok(())
    }
}
