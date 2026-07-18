use super::*;

impl<P, D, B> ApplicationEngine<P, D, B>
where
    P: crate::native_contract_provider::NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    /// Loads a raw script into the VM, configuring call flags and optional script hash.
    pub fn load_script(
        &mut self,
        script: Vec<u8>,
        call_flags: CallFlags,
        script_hash: Option<UInt160>,
    ) -> CoreResult<()> {
        self.load_script_bytes(script.as_slice(), call_flags, script_hash)
    }

    /// Loads script bytes without an intermediate owned `Vec` allocation.
    ///
    /// Transaction import uses this with `tx.script()` to avoid cloning the
    /// transaction body into a temporary buffer before `Script` wraps an `Arc`.
    pub fn load_script_bytes(
        &mut self,
        script: &[u8],
        call_flags: CallFlags,
        script_hash: Option<UInt160>,
    ) -> CoreResult<()> {
        // Match Neo N3/C# semantics: scripts loaded by the host return all
        // evaluation stack items (`rvcount = -1`) so that witness invocation
        // scripts can pass parameters to verification scripts and invocation
        // results are preserved on `ResultStack`.
        self.load_script_with_state(
            Script::new_relaxed_from_slice(script),
            -1,
            0,
            move |state| {
                state.call_flags = call_flags;
                if let Some(hash) = script_hash {
                    state.script_hash = Some(hash);
                }
            },
        )?;
        Ok(())
    }

    /// Loads a contract method into the VM using the provided descriptor.
    pub fn load_contract_method(
        &mut self,
        contract: ContractState,
        method: ContractMethodDescriptor,
        call_flags: CallFlags,
    ) -> CoreResult<()> {
        let has_return_value = method.return_type != ContractParameterType::Void;
        let contract = self.cache_or_reuse_contract(contract);
        let previous_context = self.vm_engine.engine().current_context().cloned();
        let previous_hash = if let Some(ref ctx) = previous_context {
            let state_arc = ctx.state();
            let hash_from_state = state_arc.lock().script_hash;
            Some(
                hash_from_state
                    .or_else(|| UInt160::from_bytes(&ctx.script_hash()).ok())
                    .ok_or_else(|| {
                        CoreError::invalid_operation("Invalid script hash in execution context")
                    })?,
            )
        } else {
            None
        };

        let param_count = method.parameters.len();
        self.load_contract_context(
            contract,
            &method,
            call_flags,
            param_count,
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

    pub(in crate::application_engine) fn finalize_fault(&mut self) {
        self.capture_fault_exception_from_vm();
        self.fault_pending_observed_calls();
        self.notifications.clear();
    }

    /// Executes the loaded scripts until the VM halts or faults, returning the resulting VM state.
    ///
    /// This mirrors the C# engine behaviour used by RPC invocation endpoints: callers can inspect
    /// `state()` / `fault_exception()` even when execution faults.
    pub fn execute_allow_fault(&mut self) -> VMState {
        // Bind only for the callback-capable operation. The engine remains
        // movable after this method returns.
        let attached_here = self.attach_host();

        // The local neo-vm engine is the sole execution authority. It owns the
        // height-selected Neo N3 jump table and the invocation, reference,
        // exception, and result-stack limits used by consensus execution.
        let state = self.vm_engine.engine_mut().execute();
        self.detach_host(attached_here);
        if state == VMState::FAULT {
            self.finalize_fault();
        }
        state
    }

    /// Executes instructions until the invocation stack depth returns to `target_depth`
    /// or the VM halts/faults. Intended for native contract helpers that need to run
    /// a nested contract call synchronously.
    pub fn execute_until_invocation_stack_depth(&mut self, target_depth: usize) -> VMState {
        // Bind only while VM steps can invoke host callbacks.
        let attached_here = self.attach_host();

        let result = loop {
            let state = self.vm_engine.engine().state();
            if state == VMState::HALT || state == VMState::FAULT {
                if state == VMState::FAULT {
                    self.finalize_fault();
                }
                break state;
            }

            if self.vm_engine.engine().invocation_stack().len() <= target_depth {
                break state;
            }

            if let Err(err) = self.vm_engine.engine_mut().execute_next() {
                let message = err.to_string();
                self.vm_engine.engine_mut().set_uncaught_exception(Some(
                    StackItem::from_byte_string(message.into_bytes()),
                ));
                self.vm_engine.engine_mut().set_state(VMState::FAULT);
                self.finalize_fault();
                break VMState::FAULT;
            }
        };
        self.detach_host(attached_here);
        result
    }

    /// Executes the loaded scripts until the VM halts or faults.
    pub fn execute(&mut self) -> CoreResult<()> {
        let state = self.execute_allow_fault();
        if state == VMState::FAULT {
            let message = self
                .fault_exception()
                .unwrap_or("VM execution faulted during script verification");
            return Err(CoreError::invalid_operation(message.to_string()));
        }
        Ok(())
    }

    /// Emit a notification event
    pub fn emit_notification(
        &mut self,
        script_hash: &UInt160,
        event_name: &str,
        state: &[Vec<u8>],
    ) -> CoreResult<()> {
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
    pub fn check_committee_witness(&self) -> CoreResult<bool> {
        // Mirrors C# `NativeContract.CheckCommittee`: verify a witness from the
        // committee multisig address. That address is `NEO.GetCommitteeAddress`,
        // computed by NeoToken (which owns the committee cache) and reached here
        // through the native-contract seam — the engine cannot depend on
        // `neo-native-contracts` directly. C# `GetCommitteeAddress` faults if the
        // committee cache is missing, so a lookup error is propagated. When no
        // standalone engine uses `NoNativeContractProvider`, we fail closed.
        let committee_address = match self
            .native_contract_provider()
            .committee_address(self.snapshot_cache.as_ref())
            .map_err(|e| {
                CoreError::invalid_operation(format!("committee address lookup failed: {e}"))
            })? {
            Some(address) => address,
            None => return Ok(false),
        };
        self.check_witness_hash(&committee_address)
    }

    /// Clear all storage for a contract
    pub fn clear_contract_storage(&mut self, contract_hash: &UInt160) -> CoreResult<()> {
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
    pub fn get_storage_context(&self) -> CoreResult<StorageContext> {
        // 1. Get current contract hash
        let contract_hash = self
            .current_script_hash
            .ok_or_else(|| CoreError::invalid_operation("No current contract"))?;

        // Contract contexts already carry the exact ContractState that was
        // loaded for execution. Reuse its ID after a cheap existence check so
        // repeated GetContext calls do not deserialize the manifest and NEF.
        // The existence check is required because a contract can destroy
        // itself and then attempt another syscall before its frame unloads.
        if let Some(contract) = self.contracts.get(&contract_hash)
            && self
                .native_contract_provider()
                .contract_exists(self.snapshot_cache.as_ref(), &contract_hash)?
        {
            return Ok(StorageContext {
                id: contract.id,
                is_read_only: false,
            });
        }

        // Fall back to the provider for entry scripts and custom composition
        // paths that did not load the contract through the engine cache.
        let contract = self
            .native_contract_provider()
            .contract_state(self.snapshot_cache.as_ref(), &contract_hash)?
            .ok_or_else(|| {
                CoreError::not_found(format!("Contract not found: {}", contract_hash))
            })?;

        // 3. Create storage context (matches C# StorageContext creation)
        Ok(StorageContext {
            id: contract.id,
            is_read_only: false,
        })
    }

    /// Gets a read-only storage context (matches C# GetReadOnlyContext exactly).
    pub fn get_read_only_storage_context(&self) -> CoreResult<StorageContext> {
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
        &mut self,
        context: &StorageContext,
        prefix: &[u8],
        options: FindOptions,
    ) -> CoreResult<StorageIterator> {
        self.validate_find_options(options)?;

        let search_key = StorageKey::new(context.id, prefix.to_vec());
        let direction = if options.contains(FindOptions::Backwards) {
            SeekDirection::Backward
        } else {
            SeekDirection::Forward
        };

        let entries = self
            .snapshot_cache
            .find(Some(&search_key), direction)
            .collect::<Vec<_>>();
        if self.execution_observations_enabled() {
            let row_count = match u32::try_from(entries.len()) {
                Ok(row_count) => row_count,
                Err(_) => {
                    self.fail_execution_observation(
                        crate::execution_artifact::ExecutionArtifactError::NumericOverflow {
                            field: "storage range row count",
                        },
                    );
                    u32::MAX
                }
            };
            let range_direction = if options.contains(FindOptions::Backwards) {
                neo_vm::RangeDirection::Reverse
            } else {
                neo_vm::RangeDirection::Forward
            };
            let access = crate::host_access_audit::StorageRangeAccess::prefix(
                context.id,
                prefix.to_vec(),
                range_direction,
                options,
                row_count,
            );
            self.observe_storage_range(access, &entries);
        }
        Ok(StorageIterator::new(entries, prefix.len(), options))
    }

    /// Gets the storage price from the policy contract (matches C# StoragePrice property).
    pub(super) fn get_storage_price(&mut self) -> usize {
        self.storage_price as usize
    }

    /// Returns remaining gas in datoshi (matches C# `GasLeft`).
    pub(crate) fn gas_left(&self) -> i64 {
        self.fee_amount
            .saturating_sub(self.fee_consumed)
            .saturating_div(FEE_FACTOR)
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
