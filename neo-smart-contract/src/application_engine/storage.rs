//! ApplicationEngine.Storage - matches C# Neo.SmartContract.ApplicationEngine.Storage.cs exactly
use super::*;
use crate::iterators::IteratorInterop;
use neo_primitives::constants::{MAX_STORAGE_KEY_SIZE, MAX_STORAGE_VALUE_SIZE};
use std::sync::OnceLock;

fn trace_storage_get() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("NEO_TRACE_STORAGE_GET").is_some())
}

fn trace_storage_put() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("NEO_TRACE_STORAGE_PUT").is_some())
}
impl ApplicationEngine {
    /// Gets storage context for reading
    pub fn storage_get_context(&mut self) -> Result<StorageContext, String> {
        self.get_storage_context().map_err(|err| err.to_string())
    }
    /// Gets storage context for reading (readonly)
    pub fn storage_get_read_only_context(&mut self) -> Result<StorageContext, String> {
        self.get_read_only_storage_context()
            .map_err(|err| err.to_string())
    }
    /// Converts context to read-write
    pub fn storage_as_read_write(
        &mut self,
        context: StorageContext,
    ) -> Result<StorageContext, String> {
        if !self.has_call_flags(CallFlags::WRITE_STATES) {
            return Err("Write states not allowed".to_string());
        }
        Ok(StorageContext::read_write(context.id))
    }
    /// Gets a storage value
    pub fn storage_get(
        &mut self,
        context: StorageContext,
        key: Vec<u8>,
    ) -> Result<Option<Vec<u8>>, String> {
        if !self.has_call_flags(CallFlags::READ_STATES) {
            return Err("Read states not allowed".to_string());
        }
        // Check key size
        if key.len() > MAX_STORAGE_KEY_SIZE {
            return Err("Key too large".to_string());
        }
        Ok(self.get_storage_item(&context, &key))
    }
    /// Sets a storage value
    pub fn storage_put(
        &mut self,
        context: StorageContext,
        key: Vec<u8>,
        value: Vec<u8>,
    ) -> Result<(), String> {
        if context.is_read_only {
            return Err("Context is read-only".to_string());
        }
        if !self.has_call_flags(CallFlags::WRITE_STATES) {
            return Err("Write states not allowed".to_string());
        }
        // Check sizes
        if key.len() > MAX_STORAGE_KEY_SIZE {
            return Err("Key too large".to_string());
        }
        if value.len() > MAX_STORAGE_VALUE_SIZE {
            return Err("Value too large".to_string());
        }
        self.put_storage_item(&context, &key, &value)
            .map_err(|err| err.to_string())?;
        Ok(())
    }
    /// Deletes a storage value
    pub fn storage_delete(&mut self, context: StorageContext, key: Vec<u8>) -> Result<(), String> {
        if context.is_read_only {
            return Err("Context is read-only".to_string());
        }
        if !self.has_call_flags(CallFlags::WRITE_STATES) {
            return Err("Write states not allowed".to_string());
        }
        // Check key size
        if key.len() > MAX_STORAGE_KEY_SIZE {
            return Err("Key too large".to_string());
        }
        self.delete_storage_item(&context, &key)
            .map_err(|err| err.to_string())?;
        Ok(())
    }
    /// Finds storage values
    pub fn storage_find(
        &mut self,
        context: StorageContext,
        prefix: Vec<u8>,
        options: FindOptions,
    ) -> Result<StorageIterator, String> {
        if !self.has_call_flags(CallFlags::READ_STATES) {
            return Err("Read states not allowed".to_string());
        }
        // Check prefix size
        if prefix.len() > MAX_STORAGE_KEY_SIZE {
            return Err("Prefix too large".to_string());
        }
        self.find_storage_entries(&context, &prefix, options)
    }
    /// Gets a value from the current contract's storage context.
    pub fn storage_get_local(&mut self, key: Vec<u8>) -> Result<Option<Vec<u8>>, String> {
        let context = self.storage_get_read_only_context()?;
        self.storage_get(context, key)
    }
    /// Puts a key/value pair into the current contract's storage context.
    pub fn storage_put_local(&mut self, key: Vec<u8>, value: Vec<u8>) -> Result<(), String> {
        let context = self.storage_get_context()?;
        self.storage_put(context, key, value)
    }
    /// Deletes an entry from the current contract's storage context.
    pub fn storage_delete_local(&mut self, key: Vec<u8>) -> Result<(), String> {
        let context = self.storage_get_context()?;
        self.storage_delete(context, key)
    }
    /// Finds entries with the given prefix in the current contract's storage context.
    pub fn storage_find_local(
        &mut self,
        prefix: Vec<u8>,
        options: FindOptions,
    ) -> Result<StorageIterator, String> {
        let context = self.storage_get_read_only_context()?;
        self.storage_find(context, prefix, options)
    }
    /// Finds entries with the given prefix, starting at `prefix + start`
    /// inclusive (C# v3.11.2 `FindWithStart`, active since `HF_Huyao`).
    pub fn storage_find_with_start(
        &mut self,
        context: StorageContext,
        prefix: Vec<u8>,
        start: Vec<u8>,
        options: FindOptions,
    ) -> Result<StorageIterator, String> {
        if !self.has_call_flags(CallFlags::READ_STATES) {
            return Err("Read states not allowed".to_string());
        }
        // Check prefix size
        if prefix.len() > MAX_STORAGE_KEY_SIZE {
            return Err("Prefix too large".to_string());
        }
        self.find_storage_entries_with_start(&context, &prefix, &start, options)
    }
    /// Finds entries with the given prefix in the current contract's storage
    /// context, starting at `prefix + start` inclusive.
    pub fn storage_find_local_with_start(
        &mut self,
        prefix: Vec<u8>,
        start: Vec<u8>,
        options: FindOptions,
    ) -> Result<StorageIterator, String> {
        let context = self.storage_get_read_only_context()?;
        self.storage_find_with_start(context, prefix, start, options)
    }
}
fn map_storage_error(service: &str, error: String) -> VmError {
    VmError::InteropService {
        service: service.to_string(),
        error,
    }
}
fn pop_storage_bytes(app: &mut ApplicationEngine, service: &str, label: &str) -> VmResult<Vec<u8>> {
    let item = app.pop().map_err(|e| map_storage_error(service, e))?;
    item.as_bytes().map_err(|error| {
        map_storage_error(
            service,
            format!(
                "Storage {label} conversion failed for {:?}: {error}",
                item.stack_item_type()
            ),
        )
    })
}
pub(crate) fn register_storage_interops(
    engine: &mut dyn super::state::InteropRegistration,
) -> VmResult<()> {
    engine.register_host_service_huyao(
        "System.Storage.GetContext",
        1 << 4,
        op_code_prices_v1::GET_STORAGE_CONTEXT_PRICE,
        CallFlags::READ_STATES,
        storage_get_context_handler,
    )?;
    engine.register_host_service_huyao(
        "System.Storage.GetReadOnlyContext",
        1 << 4,
        op_code_prices_v1::GET_READ_ONLY_CONTEXT_PRICE,
        CallFlags::READ_STATES,
        storage_get_read_only_context_handler,
    )?;
    engine.register_host_service(
        "System.Storage.AsReadOnly",
        1 << 4,
        CallFlags::READ_STATES,
        storage_as_read_only_handler,
    )?;
    engine.register_host_service(
        "System.Storage.Get",
        1 << 15,
        CallFlags::READ_STATES,
        storage_get_handler,
    )?;
    engine.register_host_service(
        "System.Storage.Put",
        1 << 15,
        CallFlags::WRITE_STATES,
        storage_put_handler,
    )?;
    engine.register_host_service(
        "System.Storage.Delete",
        1 << 15,
        CallFlags::WRITE_STATES,
        storage_delete_handler,
    )?;
    engine.register_host_service_huyao(
        "System.Storage.Find",
        1 << 15,
        op_code_prices_v1::STORAGE_FIND_PRICE,
        CallFlags::READ_STATES,
        storage_find_handler,
    )?;
    if engine.is_hardfork_enabled(Hardfork::HfFaun) {
        engine.register_host_service(
            "System.Storage.Local.Get",
            1 << 15,
            CallFlags::READ_STATES,
            storage_get_local_handler,
        )?;
        engine.register_host_service(
            "System.Storage.Local.Put",
            1 << 15,
            CallFlags::WRITE_STATES,
            storage_put_local_handler,
        )?;
        engine.register_host_service(
            "System.Storage.Local.Delete",
            1 << 15,
            CallFlags::WRITE_STATES,
            storage_delete_local_handler,
        )?;
        engine.register_host_service_huyao(
            "System.Storage.Local.Find",
            1 << 15,
            op_code_prices_v1::STORAGE_FIND_PRICE,
            CallFlags::READ_STATES,
            storage_find_local_handler,
        )?;
    }
    // C# v3.11.2 registers both `FindWithStart` descriptors with
    // `Hardfork.HF_Huyao` and an `[InteropPrice(HF_Huyao, ReadFromDiskPrice)]`
    // on the implementation, so they only exist once the hardfork is active
    // and always bill the Huyao seek price.
    if engine.is_hardfork_enabled(Hardfork::HfHuyao) {
        engine.register_host_service_huyao(
            "System.Storage.FindWithStart",
            1 << 15,
            op_code_prices_v1::STORAGE_FIND_PRICE,
            CallFlags::READ_STATES,
            storage_find_with_start_handler,
        )?;
        engine.register_host_service_huyao(
            "System.Storage.Local.FindWithStart",
            1 << 15,
            op_code_prices_v1::STORAGE_FIND_PRICE,
            CallFlags::READ_STATES,
            storage_find_local_with_start_handler,
        )?;
    }
    Ok(())
}

mod find;
mod get_put;

use find::{
    storage_find_handler, storage_find_local_handler, storage_find_local_with_start_handler,
    storage_find_with_start_handler,
};
use get_put::{
    storage_as_read_only_handler, storage_delete_handler, storage_delete_local_handler,
    storage_get_context_handler, storage_get_handler, storage_get_local_handler,
    storage_get_read_only_context_handler, storage_put_handler, storage_put_local_handler,
};

#[cfg(test)]
mod tests;
