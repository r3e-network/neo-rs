//! ApplicationEngine.Storage - matches C# Neo.SmartContract.ApplicationEngine.Storage.cs exactly

use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::call_flags::CallFlags;
use crate::smart_contract::find_options::FindOptions;
use crate::smart_contract::iterators::StorageIterator;
use crate::smart_contract::storage_context::StorageContext;
use neo_vm::error::VmError;
use neo_vm::{ExecutionEngine, StackItem, VmResult};

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
        if key.len() > crate::constants::MAX_STORAGE_KEY_SIZE {
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
        if key.len() > crate::constants::MAX_STORAGE_KEY_SIZE {
            return Err("Key too large".to_string());
        }

        if value.len() > crate::constants::MAX_STORAGE_VALUE_SIZE {
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
        if key.len() > crate::constants::MAX_STORAGE_KEY_SIZE {
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
        if prefix.len() > crate::constants::MAX_STORAGE_KEY_SIZE {
            return Err("Prefix too large".to_string());
        }

        Ok(self.find_storage_entries(&context, &prefix, options))
    }
}

fn map_storage_error(service: &str, error: String) -> VmError {
    VmError::InteropService {
        service: service.to_string(),
        error,
    }
}

fn storage_get_context_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    let context = app
        .storage_get_context()
        .map_err(|e| map_storage_error("System.Storage.GetContext", e))?;
    app.push(context.to_stack_item())
        .map_err(|e| map_storage_error("System.Storage.GetContext", e))?;
    Ok(())
}

fn storage_get_read_only_context_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    let context = app
        .storage_get_read_only_context()
        .map_err(|e| map_storage_error("System.Storage.GetReadOnlyContext", e))?;
    app.push(context.to_stack_item())
        .map_err(|e| map_storage_error("System.Storage.GetReadOnlyContext", e))?;
    Ok(())
}

fn storage_as_read_only_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    let item = app
        .pop()
        .map_err(|e| map_storage_error("System.Storage.AsReadOnly", e))?;
    let context = StorageContext::from_stack_item(&item)
        .map_err(|e| map_storage_error("System.Storage.AsReadOnly", e))?;
    app.push(context.as_read_only().to_stack_item())
        .map_err(|e| map_storage_error("System.Storage.AsReadOnly", e))?;
    Ok(())
}

fn storage_get_handler(app: &mut ApplicationEngine, _engine: &mut ExecutionEngine) -> VmResult<()> {
    let key_item = app
        .pop()
        .map_err(|e| map_storage_error("System.Storage.Get", e))?;
    let key = match key_item {
        StackItem::ByteString(bytes) => bytes,
        StackItem::Buffer(buffer) => buffer.data().to_vec(),
        _ => {
            return Err(map_storage_error(
                "System.Storage.Get",
                "Storage key must be a byte array".to_string(),
            ))
        }
    };

    let context_item = app
        .pop()
        .map_err(|e| map_storage_error("System.Storage.Get", e))?;
    let context = StorageContext::from_stack_item(&context_item)
        .map_err(|e| map_storage_error("System.Storage.Get", e))?;

    match app
        .storage_get(context, key)
        .map_err(|e| map_storage_error("System.Storage.Get", e))?
    {
        Some(value) => app
            .push_bytes(value)
            .map_err(|e| map_storage_error("System.Storage.Get", e))?,
        None => {
            app.push_null()
                .map_err(|e| map_storage_error("System.Storage.Get", e))?;
        }
    }
    Ok(())
}

fn storage_put_handler(app: &mut ApplicationEngine, _engine: &mut ExecutionEngine) -> VmResult<()> {
    let value_item = app
        .pop()
        .map_err(|e| map_storage_error("System.Storage.Put", e))?;
    let value = match value_item {
        StackItem::ByteString(bytes) => bytes,
        StackItem::Buffer(buffer) => buffer.data().to_vec(),
        _ => {
            return Err(map_storage_error(
                "System.Storage.Put",
                "Storage value must be a byte array".to_string(),
            ))
        }
    };

    let key_item = app
        .pop()
        .map_err(|e| map_storage_error("System.Storage.Put", e))?;
    let key = match key_item {
        StackItem::ByteString(bytes) => bytes,
        StackItem::Buffer(buffer) => buffer.data().to_vec(),
        _ => {
            return Err(map_storage_error(
                "System.Storage.Put",
                "Storage key must be a byte array".to_string(),
            ))
        }
    };

    let context_item = app
        .pop()
        .map_err(|e| map_storage_error("System.Storage.Put", e))?;
    let context = StorageContext::from_stack_item(&context_item)
        .map_err(|e| map_storage_error("System.Storage.Put", e))?;

    app.storage_put(context, key, value)
        .map_err(|e| map_storage_error("System.Storage.Put", e))?;
    Ok(())
}

fn storage_delete_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    let key_item = app
        .pop()
        .map_err(|e| map_storage_error("System.Storage.Delete", e))?;
    let key = match key_item {
        StackItem::ByteString(bytes) => bytes,
        StackItem::Buffer(buffer) => buffer.data().to_vec(),
        _ => {
            return Err(map_storage_error(
                "System.Storage.Delete",
                "Storage key must be a byte array".to_string(),
            ))
        }
    };

    let context_item = app
        .pop()
        .map_err(|e| map_storage_error("System.Storage.Delete", e))?;
    let context = StorageContext::from_stack_item(&context_item)
        .map_err(|e| map_storage_error("System.Storage.Delete", e))?;

    app.storage_delete(context, key)
        .map_err(|e| map_storage_error("System.Storage.Delete", e))?;
    Ok(())
}

fn storage_find_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    let options_bits = app
        .pop_integer()
        .map_err(|e| map_storage_error("System.Storage.Find", e))?;
    let options = FindOptions::from_bits(options_bits as u8).ok_or_else(|| {
        map_storage_error(
            "System.Storage.Find",
            format!("Invalid FindOptions value: {options_bits}"),
        )
    })?;

    let prefix = app
        .pop_bytes()
        .map_err(|e| map_storage_error("System.Storage.Find", e))?;

    let context_item = app
        .pop()
        .map_err(|e| map_storage_error("System.Storage.Find", e))?;
    let context = StorageContext::from_stack_item(&context_item)
        .map_err(|e| map_storage_error("System.Storage.Find", e))?;

    let iterator = app
        .storage_find(context, prefix.clone(), options)
        .map_err(|e| map_storage_error("System.Storage.Find", e))?;

    let iterator_id = app
        .store_storage_iterator(iterator)
        .map_err(|e| map_storage_error("System.Storage.Find", e))?;
    app.push_bytes(iterator_id.to_le_bytes().to_vec())
        .map_err(|e| map_storage_error("System.Storage.Find", e))?;
    Ok(())
}

pub(crate) fn register_storage_interops(engine: &mut ApplicationEngine) -> VmResult<()> {
    engine.register_host_service(
        "System.Storage.GetContext",
        1 << 4,
        CallFlags::READ_STATES,
        storage_get_context_handler,
    )?;
    engine.register_host_service(
        "System.Storage.GetReadOnlyContext",
        1 << 4,
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
    engine.register_host_service(
        "System.Storage.Find",
        1 << 15,
        CallFlags::READ_STATES,
        storage_find_handler,
    )?;
    Ok(())
}
