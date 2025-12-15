use super::*;

impl ApplicationEngine {
    /// Validates that the provided hash has a matching witness in the current transaction.
    /// This matches C# ApplicationEngine.CheckWitnessInternal exactly, including witness rules.
    fn check_witness_internal(&self, hash: &UInt160) -> Result<bool> {
        // 1. Calling script hash always succeeds.
        if self.get_calling_script_hash() == Some(*hash) {
            return Ok(true);
        }

        let Some(container) = self.script_container.as_ref() else {
            return Ok(false);
        };

        // 2. Transaction container path (witness rules and scopes).
        if let Some(tx) = container.as_transaction() {
            let mut signers: Vec<_> = tx.signers().to_vec();

            // OracleResponse transactions inherit signers from the original request.
            if let Some(oracle_id) = tx.attributes().iter().find_map(|attr| match attr {
                TransactionAttribute::OracleResponse(resp) => Some(resp.id),
                _ => None,
            }) {
                let oracle = crate::smart_contract::native::OracleContract::new();
                let Some(request) = oracle.get_request(self.snapshot_cache.as_ref(), oracle_id)?
                else {
                    return Ok(false);
                };

                let ledger = crate::smart_contract::native::LedgerContract::new();
                let Some(state) = ledger
                    .get_transaction_state(self.snapshot_cache.as_ref(), &request.original_tx_id)?
                else {
                    return Ok(false);
                };

                signers = state.transaction().signers().to_vec();
            }

            let Some(signer) = signers.iter().find(|s| s.account == *hash) else {
                return Ok(false);
            };

            for rule in signer.get_all_rules() {
                if self.match_witness_condition(&rule.condition)? {
                    return Ok(rule.action == WitnessRuleAction::Allow);
                }
            }

            return Ok(false);
        }

        // 3. Non-transaction containers require ReadStates and use verifying script hashes.
        if !self.has_call_flags(CallFlags::READ_STATES) {
            return Err(Error::invalid_operation(
                "Read states not allowed".to_string(),
            ));
        }

        Ok(container
            .get_script_hashes_for_verifying(self.snapshot_cache.as_ref())
            .contains(hash))
    }

    /// Checks whether the specified hash has witnessed the current execution.
    ///
    /// Public wrapper for C# `CheckWitness(UInt160)` semantics.
    pub fn check_witness(&self, hash: &UInt160) -> Result<bool> {
        self.check_witness_internal(hash)
    }

    /// Backwards-compatible alias mirroring `check_witness`.
    pub fn check_witness_hash(&self, hash: &UInt160) -> Result<bool> {
        self.check_witness_internal(hash)
    }

    /// Evaluates a witness condition in the current execution context.
    /// Matches all Neo N3 witness condition types.
    fn match_witness_condition(&self, condition: &WitnessCondition) -> Result<bool> {
        match condition {
            WitnessCondition::Boolean { value } => Ok(*value),
            WitnessCondition::Not { condition } => {
                Ok(!self.match_witness_condition(condition.as_ref())?)
            }
            WitnessCondition::And { conditions } => {
                for sub in conditions {
                    if !self.match_witness_condition(sub)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            WitnessCondition::Or { conditions } => {
                for sub in conditions {
                    if self.match_witness_condition(sub)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            WitnessCondition::ScriptHash { hash } => Ok(self.current_script_hash() == Some(*hash)),
            WitnessCondition::CalledByContract { hash } => {
                Ok(self.get_calling_script_hash() == Some(*hash))
            }
            WitnessCondition::CalledByEntry => {
                let calling_context = {
                    let state_arc = self
                        .current_execution_state()
                        .map_err(|e| Error::invalid_operation(e.to_string()))?;
                    let state = state_arc.lock();
                    state.calling_context.clone()
                };

                let Some(calling_ctx) = calling_context else {
                    return Ok(true);
                };

                let calling_state_arc = calling_ctx
                    .get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new);
                let calling_state = calling_state_arc.lock();
                Ok(calling_state.calling_context.is_none())
            }
            WitnessCondition::Group { group } => {
                if !self.has_call_flags(CallFlags::READ_STATES) {
                    return Err(Error::invalid_operation(
                        "Read states not allowed".to_string(),
                    ));
                }

                let current_hash = self.current_script_hash().ok_or_else(|| {
                    Error::invalid_operation("No current script hash".to_string())
                })?;

                let Some(contract) = ContractManagement::get_contract_from_snapshot(
                    self.snapshot_cache.as_ref(),
                    &current_hash,
                )?
                else {
                    return Ok(false);
                };

                let group_point = crate::neo_cryptography::ECPoint::from_bytes(group)
                    .map_err(|e| Error::invalid_data(format!("Invalid witness group: {e}")))?;
                Ok(contract
                    .manifest
                    .groups
                    .iter()
                    .any(|g| g.pub_key == group_point))
            }
            WitnessCondition::CalledByGroup { group } => {
                if !self.has_call_flags(CallFlags::READ_STATES) {
                    return Err(Error::invalid_operation(
                        "Read states not allowed".to_string(),
                    ));
                }

                let Some(calling_hash) = self.get_calling_script_hash() else {
                    return Ok(false);
                };

                let Some(contract) = ContractManagement::get_contract_from_snapshot(
                    self.snapshot_cache.as_ref(),
                    &calling_hash,
                )?
                else {
                    return Ok(false);
                };

                let group_point = crate::neo_cryptography::ECPoint::from_bytes(group)
                    .map_err(|e| Error::invalid_data(format!("Invalid witness group: {e}")))?;
                Ok(contract
                    .manifest
                    .groups
                    .iter()
                    .any(|g| g.pub_key == group_point))
            }
        }
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
            .filter(|(key, _)| key.suffix().starts_with(prefix))
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
        let iterator_id = self.allocate_iterator_id()?;

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
        let iterator_id = self.allocate_iterator_id()?;
        let iterator = StorageIterator::new(results, prefix_length, options);
        self.storage_iterators.insert(iterator_id, iterator);

        Ok(iterator_id)
    }

    /// Stores an existing storage iterator and returns its identifier.
    pub fn store_storage_iterator(&mut self, iterator: StorageIterator) -> Result<u32, String> {
        let iterator_id = self.allocate_iterator_id().map_err(|err| err.to_string())?;
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

    /// Removes a storage iterator from the engine and returns it.
    ///
    /// This is primarily used by the RPC layer to persist iterators across requests
    /// without keeping them attached to the engine's internal iterator table.
    pub fn take_storage_iterator(&mut self, iterator_id: u32) -> Option<StorageIterator> {
        self.storage_iterators.remove(&iterator_id)
    }

    /// Advances a storage iterator to the next element.
    /// Returns true if successful, false if at the end.
    pub fn iterator_next(&mut self, iterator_id: u32) -> Result<bool> {
        match self.storage_iterators.get_mut(&iterator_id) {
            Some(iterator) => Ok(iterator.next()),
            None => Err(Error::not_found(format!(
                "Iterator {} not found",
                iterator_id
            ))),
        }
    }

    /// Gets the current value from a storage iterator.
    pub fn iterator_value(&self, iterator_id: u32) -> Result<StackItem> {
        match self.storage_iterators.get(&iterator_id) {
            Some(iterator) => Ok(iterator.value()),
            None => Err(Error::not_found(format!(
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

    fn allocate_iterator_id(&mut self) -> Result<u32> {
        let id = self.next_iterator_id;
        self.next_iterator_id = self
            .next_iterator_id
            .checked_add(1)
            .ok_or_else(|| Error::invalid_operation("Iterator identifier overflow"))?;
        Ok(id)
    }

    /// Gets contract ID for a given hash.
    pub(super) fn get_contract_id_by_hash(&self, hash: &UInt160) -> Option<i32> {
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
            Error::not_found(format!("Native contract not found: {}", contract_hash))
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
            let context = self.get_native_storage_context(current_hash)?;
            return self.put_storage_item(&context, key, value);
        }
        Err(Error::invalid_operation(
            "No current contract context".to_string(),
        ))
    }

    pub fn create_standard_account(&mut self, public_key: &[u8]) -> Result<UInt160> {
        if public_key.len() != 33 {
            return Err(Error::invalid_operation(
                "Public key must be 33 bytes".to_string(),
            ));
        }

        let fee = if self.is_hardfork_enabled(Hardfork::HfAspidochelone) {
            CHECK_SIG_PRICE
        } else {
            1 << 8
        };

        self.add_cpu_fee(fee)?;

        let script = Helper::signature_redeem_script(public_key);
        let hash = UInt160::from_bytes(&NeoHash::hash160(&script))
            .map_err(|e| Error::invalid_operation(format!("Invalid script hash: {}", e)))?;

        Ok(hash)
    }

    pub fn create_multisig_account(
        &mut self,
        required_signatures: i32,
        public_keys_items: Vec<StackItem>,
    ) -> Result<UInt160> {
        if required_signatures <= 0 {
            return Err(Error::invalid_operation(
                "Multisig threshold must be positive".to_string(),
            ));
        }

        let m = required_signatures as usize;
        if public_keys_items.is_empty()
            || public_keys_items.len() > 16
            || m > public_keys_items.len()
        {
            return Err(Error::invalid_operation(
                "Invalid multisig public key count".to_string(),
            ));
        }

        let mut public_keys = Vec::with_capacity(public_keys_items.len());
        for item in public_keys_items {
            let bytes = item
                .as_bytes()
                .map_err(|e| Error::invalid_operation(e.to_string()))?;
            if bytes.len() != 33 {
                return Err(Error::invalid_operation(
                    "Each multisig public key must be 33 bytes".to_string(),
                ));
            }
            public_keys.push(bytes);
        }

        let fee = if self.is_hardfork_enabled(Hardfork::HfAspidochelone) {
            CHECK_SIG_PRICE
                .checked_mul(public_keys.len() as i64)
                .ok_or_else(|| Error::invalid_operation("Multisig fee overflow".to_string()))?
        } else {
            1 << 8
        };

        self.add_cpu_fee(fee)?;

        let script = Helper::multi_sig_redeem_script(m, &public_keys);
        let hash = UInt160::from_bytes(&NeoHash::hash160(&script))
            .map_err(|e| Error::invalid_operation(format!("Invalid script hash: {}", e)))?;

        Ok(hash)
    }

    /// Deletes a storage item (legacy API for native contracts).
    pub fn delete_storage_item_legacy(&mut self, key: &[u8]) -> Result<()> {
        if let Some(current_hash) = &self.current_script_hash {
            let context = self.get_native_storage_context(current_hash)?;
            return self.delete_storage_item(&context, key);
        }
        Err(Error::invalid_operation(
            "No current contract context".to_string(),
        ))
    }
    pub(super) fn refresh_context_tracking(&mut self) -> Result<()> {
        if let Some(current_context) = self.vm_engine.engine().current_context() {
            let current_hash = UInt160::from_bytes(&current_context.script_hash())
                .map_err(|e| Error::invalid_operation(format!("Invalid script hash: {e}")))?;
            self.current_script_hash = Some(current_hash);
            if self.entry_script_hash.is_none() {
                self.entry_script_hash = Some(current_hash);
            }

            let state_arc = current_context
                .get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new);
            let state = state_arc.lock();

            self.call_flags = state.call_flags;
            self.calling_script_hash = state
                .native_calling_script_hash
                .or(state.calling_script_hash)
                .or_else(|| {
                    state
                        .calling_context
                        .as_ref()
                        .and_then(|ctx| UInt160::from_bytes(&ctx.script_hash()).ok())
                });
        } else {
            self.current_script_hash = None;
            self.calling_script_hash = None;
            self.entry_script_hash = None;
            self.call_flags = CallFlags::ALL;
        }

        Ok(())
    }

    /// Registers native contracts in the contracts HashMap so they can be found
    pub(super) fn register_native_contracts(&mut self) {
        let contracts: Vec<Arc<dyn NativeContract>> = self.native_registry.contracts().collect();

        for contract in &contracts {
            let hash = contract.hash();
            let id = contract.id();
            let name = contract.name().to_string();
            let block_height = self.current_block_index();
            if let Some(state) = contract.contract_state(&self.protocol_settings, block_height) {
                self.contracts.entry(hash).or_insert(state);
            } else {
                tracing::debug!(
                    "Native contract {} (id {}) inactive at height {}",
                    name,
                    id,
                    block_height
                );
            }
        }

        let block_height = self.current_block_index();
        for contract in contracts {
            if !contract.is_active(&self.protocol_settings, block_height) {
                continue;
            }
            if let Err(error) = contract.initialize(self) {
                if let Some(container) = &self.script_container {
                    let log_event = LogEventArgs::new(
                        Arc::clone(container),
                        contract.hash(),
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

    pub(super) fn refresh_policy_settings(&mut self) {
        if let Some(policy) = self.policy_contract() {
            if let Ok(raw) = policy.invoke(self, "getExecFeeFactor", &[]) {
                if !raw.is_empty() {
                    let mut buffer = [0u8; 4];
                    let len = raw.len().min(4);
                    buffer[..len].copy_from_slice(&raw[..len]);
                    self.exec_fee_factor = u32::from_le_bytes(buffer);
                }
            }

            if let Ok(raw) = policy.invoke(self, "getStoragePrice", &[]) {
                if !raw.is_empty() {
                    let mut buffer = [0u8; 4];
                    let len = raw.len().min(4);
                    buffer[..len].copy_from_slice(&raw[..len]);
                    self.storage_price = u32::from_le_bytes(buffer);
                }
            }
        }
    }

    pub(super) fn initialize_nonce_data(
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

    /// Converts a VM stack item into bytes, mirroring the C# helper.
    pub fn stack_item_to_bytes(item: StackItem) -> Result<Vec<u8>, String> {
        match item.as_bytes() {
            Ok(bytes) => Ok(bytes),
            Err(_) => crate::smart_contract::binary_serializer::BinarySerializer::serialize(
                &item,
                &ExecutionEngineLimits::default(),
            ),
        }
    }
}
