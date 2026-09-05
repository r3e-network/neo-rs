use super::*;
use crate::smart_contract::iterators::IteratorInterop;

impl ApplicationEngine {
    /// Gets the raw value stored under the given key in the context's storage.
    pub fn get_storage_item(&self, context: &StorageContext, key: &[u8]) -> Option<Vec<u8>> {
        let storage_key = StorageKey::new(context.id, key.to_vec());
        self.snapshot_cache
            .get(&storage_key)
            .map(|item| item.value_bytes().into_owned())
    }

    pub(super) fn validate_find_options(&self, options: FindOptions) -> Result<(), String> {
        if options.bits() & !FindOptions::All.bits() != 0 {
            return Err(format!("Invalid FindOptions value: {options:?}"));
        }

        let keys_only = options.contains(FindOptions::KeysOnly);
        let values_only = options.contains(FindOptions::ValuesOnly);
        let deserialize = options.contains(FindOptions::DeserializeValues);
        let pick_field0 = options.contains(FindOptions::PickField0);
        let pick_field1 = options.contains(FindOptions::PickField1);

        if keys_only && (values_only || deserialize || pick_field0 || pick_field1) {
            return Err("KeysOnly cannot be used with ValuesOnly, DeserializeValues, PickField0, or PickField1".to_string());
        }

        if values_only && (keys_only || options.contains(FindOptions::RemovePrefix)) {
            return Err("ValuesOnly cannot be used with KeysOnly or RemovePrefix".to_string());
        }

        if pick_field0 && pick_field1 {
            return Err("PickField0 and PickField1 cannot be used together".to_string());
        }

        if (pick_field0 || pick_field1) && !deserialize {
            return Err("PickField0 or PickField1 requires DeserializeValues".to_string());
        }

        Ok(())
    }

    /// Puts a value, charging dynamic storage fees for the added bytes
    /// (matching the C# `ApplicationEngine` storage accounting).
    pub fn put_storage_item(
        &mut self,
        context: &StorageContext,
        key: &[u8],
        value: &[u8],
    ) -> Result<()> {
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
            self.add_runtime_fee(fee_units.saturating_mul(storage_price))?;
        }

        let item = StorageItem::from_bytes(value.to_vec());
        if existing.is_some() {
            self.snapshot_cache.update(storage_key, item);
        } else {
            self.snapshot_cache.add(storage_key, item);
        }
        Ok(())
    }

    /// Deletes the storage entry for the given key from the context's storage.
    pub fn delete_storage_item(&mut self, context: &StorageContext, key: &[u8]) -> Result<()> {
        let storage_key = StorageKey::new(context.id, key.to_vec());
        self.snapshot_cache.delete(&storage_key);
        Ok(())
    }

    /// Pushes a verifiable script container onto the VM stack as an interop
    /// interface (`System.Runtime.GetScriptContainer`).
    pub fn push_interop_container(&mut self, container: Arc<dyn Verifiable>) -> Result<(), String> {
        // The native `StackItem` model has no integer interop handle
        // (`StackItem::Interop(u64)` was a neo-vm-rs concept). Interop objects are
        // carried inline as `StackItem::InteropInterface`. For a verifiable
        // container we project it to a structured stack item, matching
        // `System.Runtime.GetScriptContainer`. The historical opaque `Interop(0)`
        // placeholder is intentionally not reproduced; no in-tree caller needs it.
        use crate::smart_contract::Interoperable;
        if let Some(transaction) = container
            .as_any()
            .downcast_ref::<crate::network::p2p::payloads::Transaction>()
        {
            self.push(transaction.to_stack_item().map_err(|e| e.to_string())?)
        } else {
            Err("Script container does not implement Interoperable".to_string())
        }
    }

    /// Pops an iterator reference from the stack and returns its engine-side id.
    pub fn pop_iterator_id(&mut self) -> Result<u32, String> {
        let item = self.pop()?;
        // Native iterators are `StackItem::InteropInterface(IteratorInterop { id })`.
        if let Ok(interop) = item.as_interface::<IteratorInterop>() {
            return Ok(interop.id());
        }
        // Fall back to a raw integer id (used by host-side/test paths).
        let identifier = item
            .into_int()
            .map_err(|e| e.to_string())?
            .to_u32()
            .ok_or_else(|| "Iterator identifier out of range".to_string())?;
        Ok(identifier)
    }

    /// Advances the iterator with the given id, returning whether a value is available.
    pub fn iterator_next_internal(&mut self, iterator_id: u32) -> Result<bool, String> {
        let iterator = self
            .storage_iterators
            .get_mut(&iterator_id)
            .ok_or_else(|| format!("Iterator {} not found", iterator_id))?;
        Ok(iterator.next())
    }

    /// Returns the current value of the iterator with the given id.
    pub fn iterator_value_internal(&self, iterator_id: u32) -> Result<StackItem, String> {
        let iterator = self
            .storage_iterators
            .get(&iterator_id)
            .ok_or_else(|| format!("Iterator {} not found", iterator_id))?;
        Ok(iterator.value())
    }

    pub(crate) fn load_script_with_state<F>(
        &mut self,
        script_bytes: Vec<u8>,
        rvcount: i32,
        initial_position: usize,
        configure: F,
    ) -> Result<ExecutionContext>
    where
        F: FnOnce(&mut ExecutionContextState),
    {
        // Ensure the VM has a valid host pointer in case the engine has moved since creation.
        self.attach_host();

        let script = Script::from(script_bytes)
            .map_err(|e| Error::invalid_operation(format!("Invalid script: {e}")))?;

        {
            let engine = self.vm_engine.engine_mut();
            let context = engine.create_context(script, rvcount, initial_position);

            let script_hash = UInt160::from_bytes(&context.script_hash())
                .map_err(|e| Error::invalid_operation(format!("Invalid script hash: {e}")))?;
            let state_arc = context
                .get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new);
            let call_flags = {
                let mut state = state_arc.lock();
                // Match Neo C#: each loaded context receives an isolated clone of
                // the current snapshot cache and commits into its parent on unload.
                state.snapshot_cache = Some(Arc::new(self.snapshot_cache.clone_cache()));
                configure(&mut state);
                if state.script_hash.is_none() {
                    state.script_hash = Some(script_hash);
                }
                state.call_flags
            };

            engine
                .load_context(context)
                .map_err(|e| Error::invalid_operation(e.to_string()))?;

            engine.set_call_flags(call_flags);
        }

        let new_context = self
            .vm_engine
            .engine()
            .current_context()
            .cloned()
            .ok_or_else(|| Error::invalid_operation("Failed to load execution context"))?;

        self.refresh_context_tracking()?;

        Ok(new_context)
    }
}
