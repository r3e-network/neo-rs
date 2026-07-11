use super::*;

impl<P, D, B> ApplicationEngine<P, D, B>
where
    P: crate::native_contract_provider::NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    /// Reads a raw storage item from the snapshot using a storage context and key.
    pub fn get_storage_item(&self, context: &StorageContext, key: &[u8]) -> Option<Vec<u8>> {
        let storage_key = StorageKey::new(context.id, key.to_vec());
        self.snapshot_cache
            .get(&storage_key)
            .map(|item| item.value_bytes().into_owned())
    }

    pub(super) fn validate_find_options(&self, options: FindOptions) -> CoreResult<()> {
        if options.bits() & !FindOptions::All.bits() != 0 {
            return Err(CoreError::other(format!(
                "Invalid FindOptions value: {options:?}"
            )));
        }

        let keys_only = options.contains(FindOptions::KeysOnly);
        let values_only = options.contains(FindOptions::ValuesOnly);
        let deserialize = options.contains(FindOptions::DeserializeValues);
        let pick_field0 = options.contains(FindOptions::PickField0);
        let pick_field1 = options.contains(FindOptions::PickField1);

        if keys_only && (values_only || deserialize || pick_field0 || pick_field1) {
            return Err(CoreError::other(
                "KeysOnly cannot be used with ValuesOnly, DeserializeValues, PickField0, or PickField1",
            ));
        }

        if values_only && (keys_only || options.contains(FindOptions::RemovePrefix)) {
            return Err(CoreError::other(
                "ValuesOnly cannot be used with KeysOnly or RemovePrefix",
            ));
        }

        if pick_field0 && pick_field1 {
            return Err(CoreError::other(
                "PickField0 and PickField1 cannot be used together",
            ));
        }

        if (pick_field0 || pick_field1) && !deserialize {
            return Err(CoreError::other(
                "PickField0 or PickField1 requires DeserializeValues",
            ));
        }

        Ok(())
    }

    /// Writes a raw storage item and charges dynamic storage fees when applicable.
    pub fn put_storage_item(
        &mut self,
        context: &StorageContext,
        key: &[u8],
        value: &[u8],
    ) -> CoreResult<()> {
        let storage_key = StorageKey::new(context.id, key.to_vec());
        let existing = self.snapshot_cache.get(&storage_key);
        let value_len = value.len();
        let new_data_size = if let Some(existing_item) = &existing {
            // Use raw value byte length to match C# item.Value.Length.
            // StorageItem.size() includes a var-length prefix which is wrong here.
            let old_len = existing_item.value_bytes().len();
            if value_len == 0 {
                0
            } else if value_len <= old_len && value_len > 0 {
                ((value_len.saturating_sub(1)) / 4) + 1
            } else if old_len == 0 {
                // Matches C# ApplicationEngine.Storage.cs:193 — `newDataSize = value.Length`
                value_len
            } else {
                ((old_len.saturating_sub(1)) / 4) + 1 + value_len.saturating_sub(old_len)
            }
        } else {
            // New storage item: C# ApplicationEngine.Storage.cs:183 — `newDataSize = key.Length + value.Length`
            key.len() + value_len
        };

        // Native contracts (negative contract IDs) are charged through native
        // method metadata / explicit runtime fees, not via Storage.Put dynamic
        // byte-cost accounting.
        if new_data_size > 0 && context.id >= 0 {
            let fee_units = new_data_size as u64;
            let storage_price = self.get_storage_price() as u64;
            let fee_delta = fee_units
                .checked_mul(storage_price)
                .ok_or_else(|| CoreError::invalid_operation("Storage fee overflow"))?;
            let trace_fees = crate::interop::application_engine_storage::storage_trace_enabled(
                self,
                "NEO_TRACE_STORAGE_FEES",
            );
            let fee_before = self.fee_consumed;
            let result = self.add_runtime_fee(fee_delta);
            if trace_fees {
                let key_preview = key
                    .iter()
                    .take(32)
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>();
                let old_len = existing.as_ref().map(|item| item.value_bytes().len());
                eprintln!(
                    "trace storage.fee: ctx_id={} key_len={} value_len={} old_len={:?} new_data_size={} storage_price={} fee_delta={} fee_before_pico={} fee_after_pico={} key_prefix={} result={:?}",
                    context.id,
                    key.len(),
                    value_len,
                    old_len,
                    new_data_size,
                    storage_price,
                    fee_delta,
                    fee_before,
                    self.fee_consumed,
                    key_preview,
                    result.as_ref().map(|_| ()),
                );
            }
            result?;
        }

        let item = StorageItem::from_bytes(value.to_vec());
        if existing.is_some() {
            self.snapshot_cache.update(storage_key, item);
        } else {
            self.snapshot_cache.add(storage_key, item);
        }
        Ok(())
    }

    /// Deletes a raw storage item from the snapshot.
    pub fn delete_storage_item(&mut self, context: &StorageContext, key: &[u8]) -> CoreResult<()> {
        let storage_key = StorageKey::new(context.id, key.to_vec());
        self.snapshot_cache.delete(&storage_key);
        Ok(())
    }

    /// Pushes an interop container placeholder onto the VM stack.
    pub fn push_interop_container(
        &mut self,
        _container: Arc<VerifiableContainer>,
    ) -> CoreResult<()> {
        // Iterator/interop handles are carried as integer stack items; the
        // concrete object lives in the engine-side `storage_iterators` table.
        self.push(StackItem::from_i64(0))
    }

    /// Pops and validates a storage iterator identifier from the VM stack.
    pub fn pop_iterator_id(&mut self) -> CoreResult<u32> {
        let item = self.pop()?;
        // C# represents a storage iterator as an `InteropInterface` wrapping a
        // `StorageIterator` (System.Storage.Find / native methods that return an
        // iterator both push that form — see `storage_find` and
        // `decode_native_result`). Our engine keeps the concrete iterator in the
        // `storage_iterators` table keyed by a u32 id and carries that id in an
        // `IteratorInterop` interop handle, so the id must be read back from the
        // interop. Falling through to `into_int` keeps any bare-integer handle
        // path working.
        if let Ok(interface) = item.as_interface() {
            if let Some(iterator_id) = interface.iterator_id() {
                return Ok(iterator_id);
            }
        }
        let identifier = item
            .into_int()
            .map_err(|e| CoreError::other(e.to_string()))?
            .to_u32()
            .ok_or_else(|| CoreError::other("Iterator identifier out of range"))?;
        Ok(identifier)
    }

    /// Advances a registered storage iterator.
    pub fn iterator_next_internal(&mut self, iterator_id: u32) -> CoreResult<bool> {
        let iterator = self
            .storage_iterators
            .get_mut(&iterator_id)
            .ok_or_else(|| CoreError::other(format!("Iterator {} not found", iterator_id)))?;
        Ok(iterator.next())
    }

    /// Returns the current value of a registered storage iterator.
    pub fn iterator_value_internal(&self, iterator_id: u32) -> CoreResult<StackItem> {
        let iterator = self
            .storage_iterators
            .get(&iterator_id)
            .ok_or_else(|| CoreError::other(format!("Iterator {} not found", iterator_id)))?;
        iterator.value()
    }

    pub(crate) fn load_script_with_state<F>(
        &mut self,
        script_bytes: Vec<u8>,
        rvcount: i32,
        initial_position: usize,
        configure: F,
    ) -> CoreResult<ExecutionContext<B>>
    where
        F: FnOnce(&mut ExecutionContextState<B>),
    {
        let script = Script::from(script_bytes)
            .map_err(|e| CoreError::invalid_operation(format!("Invalid script: {e}")))?;

        let (context, call_flags, invocation_counter_hash) = {
            let engine = self.vm_engine.engine_mut();
            let context = engine.create_context(script, rvcount, initial_position);

            let script_hash = UInt160::from_bytes(&context.script_hash())
                .map_err(|e| CoreError::invalid_operation(format!("Invalid script hash: {e}")))?;
            let state_arc = context.state();
            let (call_flags, invocation_counter_hash) = {
                let mut state = state_arc.lock();
                // Match Neo C#: each loaded context receives an isolated clone of
                // the current snapshot cache and commits into its parent on unload.
                state.snapshot_cache = Some(Arc::new(self.snapshot_cache.clone_cache()));
                configure(&mut state);
                if state.script_hash.is_none() {
                    state.script_hash = Some(script_hash);
                }
                let invocation_counter_hash = state.script_hash.ok_or_else(|| {
                    CoreError::invalid_operation("Execution context script hash was not set")
                })?;
                (state.call_flags, invocation_counter_hash)
            };
            (context, call_flags, invocation_counter_hash)
        };

        // C# ApplicationEngine.LoadContext initializes the counter with
        // ExecutionContextState.ScriptHash, which may be a logical contract or
        // witness hash rather than the raw script hash.
        self.invocation_counter
            .entry(invocation_counter_hash)
            .or_insert(1);

        let attached_here = self.attach_host();
        let load_result = self.vm_engine.engine_mut().load_context(context);
        self.detach_host(attached_here);
        load_result.map_err(|e| CoreError::invalid_operation(e.to_string()))?;
        self.vm_engine.engine_mut().set_call_flags(call_flags);

        let new_context = self
            .vm_engine
            .engine()
            .current_context()
            .cloned()
            .ok_or_else(|| CoreError::invalid_operation("Failed to load execution context"))?;

        self.refresh_context_tracking()?;

        Ok(new_context)
    }
}
