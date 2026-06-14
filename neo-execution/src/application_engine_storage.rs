//! ApplicationEngine.Storage - matches C# Neo.SmartContract.ApplicationEngine.Storage.cs exactly

use crate::application_engine::ApplicationEngine;
use crate::iterators::{IteratorInterop, StorageIterator};
use crate::storage_context::StorageContext;
use neo_config::hardfork::Hardfork;
use neo_error::{CoreError, CoreResult};
use neo_manifest::CallFlags;
use neo_primitives::FindOptions;
use neo_vm::error::VmError;
use neo_vm::{ExecutionEngine, StackItem, VmResult};

impl ApplicationEngine {
    /// Gets storage context for reading
    pub fn storage_get_context(&mut self) -> CoreResult<StorageContext> {
        self.get_storage_context()
            .map_err(|err| CoreError::other(err.to_string()))
    }

    /// Gets storage context for reading (readonly)
    pub fn storage_get_read_only_context(&mut self) -> CoreResult<StorageContext> {
        self.get_read_only_storage_context()
            .map_err(|err| CoreError::other(err.to_string()))
    }

    /// Converts context to read-write
    pub fn storage_as_read_write(&mut self, context: StorageContext) -> CoreResult<StorageContext> {
        if !self.has_call_flags(CallFlags::WRITE_STATES) {
            return Err(CoreError::other("Write states not allowed"));
        }

        Ok(StorageContext::read_write(context.id))
    }

    /// Gets a storage value
    pub fn storage_get(
        &mut self,
        context: StorageContext,
        key: Vec<u8>,
    ) -> CoreResult<Option<Vec<u8>>> {
        if !self.has_call_flags(CallFlags::READ_STATES) {
            return Err(CoreError::other("Read states not allowed"));
        }

        // Check key size
        if key.len() > neo_primitives::constants::MAX_STORAGE_KEY_SIZE {
            return Err(CoreError::other("Key too large"));
        }

        Ok(self.get_storage_item(&context, &key))
    }

    /// Sets a storage value
    pub fn storage_put(
        &mut self,
        context: StorageContext,
        key: Vec<u8>,
        value: Vec<u8>,
    ) -> CoreResult<()> {
        if context.is_read_only {
            return Err(CoreError::other("Context is read-only"));
        }

        if !self.has_call_flags(CallFlags::WRITE_STATES) {
            return Err(CoreError::other("Write states not allowed"));
        }

        // Check sizes
        if key.len() > neo_primitives::constants::MAX_STORAGE_KEY_SIZE {
            return Err(CoreError::other("Key too large"));
        }

        if value.len() > neo_primitives::constants::MAX_STORAGE_VALUE_SIZE {
            return Err(CoreError::other("Value too large"));
        }

        self.put_storage_item(&context, &key, &value)
            .map_err(|err| CoreError::other(err.to_string()))?;

        Ok(())
    }

    /// Deletes a storage value
    pub fn storage_delete(&mut self, context: StorageContext, key: Vec<u8>) -> CoreResult<()> {
        if context.is_read_only {
            return Err(CoreError::other("Context is read-only"));
        }

        if !self.has_call_flags(CallFlags::WRITE_STATES) {
            return Err(CoreError::other("Write states not allowed"));
        }

        // Check key size
        if key.len() > neo_primitives::constants::MAX_STORAGE_KEY_SIZE {
            return Err(CoreError::other("Key too large"));
        }

        self.delete_storage_item(&context, &key)
            .map_err(|err| CoreError::other(err.to_string()))?;

        Ok(())
    }

    /// Finds storage values
    pub fn storage_find(
        &mut self,
        context: StorageContext,
        prefix: Vec<u8>,
        options: FindOptions,
    ) -> CoreResult<StorageIterator> {
        if !self.has_call_flags(CallFlags::READ_STATES) {
            return Err(CoreError::other("Read states not allowed"));
        }

        // Check prefix size
        if prefix.len() > neo_primitives::constants::MAX_STORAGE_KEY_SIZE {
            return Err(CoreError::other("Prefix too large"));
        }

        self.find_storage_entries(&context, &prefix, options)
    }

    pub fn storage_get_local(&mut self, key: Vec<u8>) -> CoreResult<Option<Vec<u8>>> {
        let context = self.storage_get_read_only_context()?;
        self.storage_get(context, key)
    }

    pub fn storage_put_local(&mut self, key: Vec<u8>, value: Vec<u8>) -> CoreResult<()> {
        let context = self.storage_get_context()?;
        self.storage_put(context, key, value)
    }

    pub fn storage_delete_local(&mut self, key: Vec<u8>) -> CoreResult<()> {
        let context = self.storage_get_context()?;
        self.storage_delete(context, key)
    }

    pub fn storage_find_local(
        &mut self,
        prefix: Vec<u8>,
        options: FindOptions,
    ) -> CoreResult<StorageIterator> {
        let context = self.storage_get_read_only_context()?;
        self.storage_find(context, prefix, options)
    }
}

fn map_storage_error(service: &str, error: impl std::fmt::Display) -> VmError {
    VmError::InteropService {
        service: service.to_string(),
        error: error.to_string(),
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
    let context_item = app
        .pop()
        .map_err(|e| map_storage_error("System.Storage.Get", e))?;
    let context = StorageContext::from_stack_item(&context_item)
        .map_err(|e| map_storage_error("System.Storage.Get", e))?;

    let key = pop_storage_bytes(app, "System.Storage.Get", "key")?;
    let trace_enabled = std::env::var_os("NEO_TRACE_STORAGE_GET").is_some();
    let trace_context_id = context.id;
    let trace_context_read_only = context.is_read_only;
    let trace_key_len = key.len();
    let trace_key_preview = if trace_enabled {
        Some(
            key.iter()
                .take(24)
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
        )
    } else {
        None
    };

    let value = app
        .storage_get(context, key.clone())
        .map_err(|e| map_storage_error("System.Storage.Get", e))?;

    if trace_enabled {
        eprintln!(
            "trace storage.get: ctx_id={} readonly={} key_len={} key_prefix={} hit={} value_len={}",
            trace_context_id,
            trace_context_read_only,
            trace_key_len,
            trace_key_preview.as_deref().unwrap_or(""),
            value.is_some(),
            value.as_ref().map(|v| v.len()).unwrap_or(0)
        );
    }

    match value {
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
    let context_item = app
        .pop()
        .map_err(|e| map_storage_error("System.Storage.Put", e))?;
    let context = StorageContext::from_stack_item(&context_item)
        .map_err(|e| map_storage_error("System.Storage.Put", e))?;

    let key = pop_storage_bytes(app, "System.Storage.Put", "key")?;

    let value = pop_storage_bytes(app, "System.Storage.Put", "value")?;

    if std::env::var_os("NEO_TRACE_STORAGE_PUT").is_some() {
        let key_preview = key
            .iter()
            .take(16)
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        eprintln!(
            "trace storage.put: ctx_id={} readonly={} key_len={} value_len={} key_prefix={}",
            context.id,
            context.is_read_only,
            key.len(),
            value.len(),
            key_preview,
        );
    }

    app.storage_put(context, key, value)
        .map_err(|e| map_storage_error("System.Storage.Put", e))?;
    Ok(())
}

fn storage_delete_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    let context_item = app
        .pop()
        .map_err(|e| map_storage_error("System.Storage.Delete", e))?;
    let context = StorageContext::from_stack_item(&context_item)
        .map_err(|e| map_storage_error("System.Storage.Delete", e))?;

    let key = pop_storage_bytes(app, "System.Storage.Delete", "key")?;

    app.storage_delete(context, key)
        .map_err(|e| map_storage_error("System.Storage.Delete", e))?;
    Ok(())
}

fn storage_get_local_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    let key = app
        .pop_bytes()
        .map_err(|e| map_storage_error("System.Storage.Local.Get", e))?;

    match app
        .storage_get_local(key)
        .map_err(|e| map_storage_error("System.Storage.Local.Get", e))?
    {
        Some(value) => app
            .push_bytes(value)
            .map_err(|e| map_storage_error("System.Storage.Local.Get", e))?,
        None => app
            .push_null()
            .map_err(|e| map_storage_error("System.Storage.Local.Get", e))?,
    }
    Ok(())
}

fn storage_put_local_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    let value = pop_storage_bytes(app, "System.Storage.Local.Put", "value")?;

    let key = pop_storage_bytes(app, "System.Storage.Local.Put", "key")?;

    app.storage_put_local(key, value)
        .map_err(|e| map_storage_error("System.Storage.Local.Put", e))?;
    Ok(())
}

fn storage_delete_local_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    let key = app
        .pop_bytes()
        .map_err(|e| map_storage_error("System.Storage.Local.Delete", e))?;
    app.storage_delete_local(key)
        .map_err(|e| map_storage_error("System.Storage.Local.Delete", e))?;
    Ok(())
}

fn storage_find_local_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    let options_bits = app
        .pop_integer()
        .map_err(|e| map_storage_error("System.Storage.Local.Find", e))?;
    let options = FindOptions::from_bits(options_bits as u8).ok_or_else(|| {
        map_storage_error(
            "System.Storage.Local.Find",
            format!("Invalid FindOptions value: {options_bits}"),
        )
    })?;
    options
        .validate()
        .map_err(|e| map_storage_error("System.Storage.Local.Find", e))?;

    let prefix = app
        .pop_bytes()
        .map_err(|e| map_storage_error("System.Storage.Local.Find", e))?;

    let iterator = app
        .storage_find_local(prefix, options)
        .map_err(|e| map_storage_error("System.Storage.Local.Find", e))?;

    let iterator_id = app
        .store_storage_iterator(iterator)
        .map_err(|e| map_storage_error("System.Storage.Local.Find", e))?;
    app.push(StackItem::from_interface(IteratorInterop::new(iterator_id)))
        .map_err(|e| map_storage_error("System.Storage.Local.Find", e))?;
    Ok(())
}

fn storage_find_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    let context_item = app
        .pop()
        .map_err(|e| map_storage_error("System.Storage.Find", e))?;
    let context = StorageContext::from_stack_item(&context_item)
        .map_err(|e| map_storage_error("System.Storage.Find", e))?;

    let prefix = app
        .pop_bytes()
        .map_err(|e| map_storage_error("System.Storage.Find", e))?;

    let options_bits = app
        .pop_integer()
        .map_err(|e| map_storage_error("System.Storage.Find", e))?;
    let options = FindOptions::from_bits(options_bits as u8).ok_or_else(|| {
        map_storage_error(
            "System.Storage.Find",
            format!("Invalid FindOptions value: {options_bits}"),
        )
    })?;
    options
        .validate()
        .map_err(|e| map_storage_error("System.Storage.Find", e))?;

    let iterator = app
        .storage_find(context, prefix.clone(), options)
        .map_err(|e| map_storage_error("System.Storage.Find", e))?;

    let iterator_id = app
        .store_storage_iterator(iterator)
        .map_err(|e| map_storage_error("System.Storage.Find", e))?;
    app.push(StackItem::from_interface(IteratorInterop::new(iterator_id)))
        .map_err(|e| map_storage_error("System.Storage.Find", e))?;
    Ok(())
}

impl ApplicationEngine {
    pub(crate) fn register_storage_interops(&mut self) -> VmResult<()> {
        self.register_host_service(
            "System.Storage.GetContext",
            1 << 4,
            CallFlags::READ_STATES,
            storage_get_context_handler,
        )?;
        self.register_host_service(
            "System.Storage.GetReadOnlyContext",
            1 << 4,
            CallFlags::READ_STATES,
            storage_get_read_only_context_handler,
        )?;
        self.register_host_service(
            "System.Storage.AsReadOnly",
            1 << 4,
            CallFlags::READ_STATES,
            storage_as_read_only_handler,
        )?;
        self.register_host_service(
            "System.Storage.Get",
            1 << 15,
            CallFlags::READ_STATES,
            storage_get_handler,
        )?;
        self.register_host_service(
            "System.Storage.Put",
            1 << 15,
            CallFlags::WRITE_STATES,
            storage_put_handler,
        )?;
        self.register_host_service(
            "System.Storage.Delete",
            1 << 15,
            CallFlags::WRITE_STATES,
            storage_delete_handler,
        )?;
        self.register_host_service(
            "System.Storage.Find",
            1 << 15,
            CallFlags::READ_STATES,
            storage_find_handler,
        )?;
        if self.is_hardfork_enabled(Hardfork::HfFaun) {
            self.register_host_service(
                "System.Storage.Local.Get",
                1 << 15,
                CallFlags::READ_STATES,
                storage_get_local_handler,
            )?;
            self.register_host_service(
                "System.Storage.Local.Put",
                1 << 15,
                CallFlags::WRITE_STATES,
                storage_put_local_handler,
            )?;
            self.register_host_service(
                "System.Storage.Local.Delete",
                1 << 15,
                CallFlags::WRITE_STATES,
                storage_delete_local_handler,
            )?;
            self.register_host_service(
                "System.Storage.Local.Find",
                1 << 15,
                CallFlags::READ_STATES,
                storage_find_local_handler,
            )?;
        }
        Ok(())
    }
}
