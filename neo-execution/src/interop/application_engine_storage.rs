//! ApplicationEngine.Storage - matches C# Neo.SmartContract.ApplicationEngine.Storage.cs exactly

use crate::ApplicationExecutionEngine as ExecutionEngine;
use crate::application_engine::ApplicationEngine;
use crate::iterators::{IteratorInterop, StorageIterator};
use crate::native_contract_provider::NativeContractProvider;
use crate::storage_context::StorageContext;
use neo_config::hardfork::Hardfork;
use neo_error::{CoreError, CoreResult};
use neo_primitives::CallFlags;
use neo_primitives::FindOptions;
use neo_vm::error::VmError;
use neo_vm::{StackItem, VmResult};

pub(crate) fn storage_trace_enabled<P, D, B>(
    app: &ApplicationEngine<P, D, B>,
    legacy_env: &str,
) -> bool
where
    P: NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    if std::env::var_os(legacy_env).is_some() {
        return true;
    }
    let Ok(raw) = std::env::var("NEO_TRACE_STORAGE_TX") else {
        return false;
    };
    let Some(container) = app.get_script_container() else {
        return false;
    };
    let Some(transaction) = container.as_transaction() else {
        return false;
    };
    let Ok(hash) = transaction.try_hash() else {
        return false;
    };
    let hash = hash.to_string();
    raw.split(',').any(|entry| {
        let entry = entry.trim();
        entry == "*" || entry.eq_ignore_ascii_case("all") || entry.eq_ignore_ascii_case(&hash)
    })
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn parse_trace_key_hex(raw: &str) -> Option<Vec<u8>> {
    let trimmed = raw.trim().strip_prefix("0x").unwrap_or(raw.trim());
    if trimmed.is_empty() || !trimmed.len().is_multiple_of(2) {
        return None;
    }
    let mut bytes = Vec::with_capacity(trimmed.len() / 2);
    for pair in trimmed.as_bytes().chunks_exact(2) {
        bytes.push((hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?);
    }
    Some(bytes)
}

fn storage_trace_key_enabled(key: &[u8]) -> bool {
    let Ok(raw) = std::env::var("NEO_TRACE_STORAGE_KEY_HEX") else {
        return false;
    };
    raw.split(',').any(|entry| {
        parse_trace_key_hex(entry).is_some_and(|needle| key == needle || key.starts_with(&needle))
    })
}

fn storage_trace_scope<P, D, B>(app: &ApplicationEngine<P, D, B>) -> String
where
    P: NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    let block = app
        .persisting_block()
        .map(|block| block.index().to_string())
        .unwrap_or_else(|| "none".to_string());
    let tx = app
        .get_script_container()
        .and_then(|container| container.as_transaction())
        .and_then(|tx| tx.try_hash().ok())
        .map(|hash| hash.to_string())
        .unwrap_or_else(|| "none".to_string());
    format!("block={block} tx={tx}")
}

impl<P, D, B> ApplicationEngine<P, D, B>
where
    P: NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
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

        self.find_storage_entries(&context, &prefix, options)
    }

    /// Reads storage from the current contract's read-only context.
    pub fn storage_get_local(&mut self, key: Vec<u8>) -> CoreResult<Option<Vec<u8>>> {
        let context = self.storage_get_read_only_context()?;
        self.storage_get(context, key)
    }

    /// Writes storage in the current contract's context.
    pub fn storage_put_local(&mut self, key: Vec<u8>, value: Vec<u8>) -> CoreResult<()> {
        let context = self.storage_get_context()?;
        self.storage_put(context, key, value)
    }

    /// Deletes storage in the current contract's context.
    pub fn storage_delete_local(&mut self, key: Vec<u8>) -> CoreResult<()> {
        let context = self.storage_get_context()?;
        self.storage_delete(context, key)
    }

    /// Finds storage entries in the current contract's read-only context.
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

fn pop_storage_bytes<P, D, B>(
    app: &mut ApplicationEngine<P, D, B>,
    service: &str,
    label: &str,
) -> VmResult<Vec<u8>>
where
    P: NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
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

fn storage_get_context_handler<P, D, B>(
    app: &mut ApplicationEngine<P, D, B>,
    _engine: &mut ExecutionEngine<B>,
) -> VmResult<()>
where
    P: NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    let context = app
        .storage_get_context()
        .map_err(|e| map_storage_error("System.Storage.GetContext", e))?;
    app.push(context.to_stack_item())
        .map_err(|e| map_storage_error("System.Storage.GetContext", e))?;
    Ok(())
}

fn storage_get_read_only_context_handler<P, D, B>(
    app: &mut ApplicationEngine<P, D, B>,
    _engine: &mut ExecutionEngine<B>,
) -> VmResult<()>
where
    P: NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    let context = app
        .storage_get_read_only_context()
        .map_err(|e| map_storage_error("System.Storage.GetReadOnlyContext", e))?;
    app.push(context.to_stack_item())
        .map_err(|e| map_storage_error("System.Storage.GetReadOnlyContext", e))?;
    Ok(())
}

fn storage_as_read_only_handler<P, D, B>(
    app: &mut ApplicationEngine<P, D, B>,
    _engine: &mut ExecutionEngine<B>,
) -> VmResult<()>
where
    P: NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    let item = app
        .pop()
        .map_err(|e| map_storage_error("System.Storage.AsReadOnly", e))?;
    let context = StorageContext::from_stack_item(&item)
        .map_err(|e| map_storage_error("System.Storage.AsReadOnly", e))?;
    app.push(context.as_read_only().to_stack_item())
        .map_err(|e| map_storage_error("System.Storage.AsReadOnly", e))?;
    Ok(())
}

fn storage_get_handler<P, D, B>(
    app: &mut ApplicationEngine<P, D, B>,
    _engine: &mut ExecutionEngine<B>,
) -> VmResult<()>
where
    P: NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    let context_item = app
        .pop()
        .map_err(|e| map_storage_error("System.Storage.Get", e))?;
    let context = StorageContext::from_stack_item(&context_item)
        .map_err(|e| map_storage_error("System.Storage.Get", e))?;

    let key = pop_storage_bytes(app, "System.Storage.Get", "key")?;
    let trace_enabled =
        storage_trace_enabled(app, "NEO_TRACE_STORAGE_GET") || storage_trace_key_enabled(&key);
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
            "trace storage.get: {} ctx_id={} readonly={} key_len={} key_prefix={} hit={} value_len={}",
            storage_trace_scope(app),
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

fn storage_put_handler<P, D, B>(
    app: &mut ApplicationEngine<P, D, B>,
    _engine: &mut ExecutionEngine<B>,
) -> VmResult<()>
where
    P: NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    let context_item = app
        .pop()
        .map_err(|e| map_storage_error("System.Storage.Put", e))?;
    let context = StorageContext::from_stack_item(&context_item)
        .map_err(|e| map_storage_error("System.Storage.Put", e))?;

    let key = pop_storage_bytes(app, "System.Storage.Put", "key")?;

    let value = pop_storage_bytes(app, "System.Storage.Put", "value")?;

    if storage_trace_enabled(app, "NEO_TRACE_STORAGE_PUT") || storage_trace_key_enabled(&key) {
        let key_preview = key
            .iter()
            .take(32)
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let value_preview = value
            .iter()
            .take(32)
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        eprintln!(
            "trace storage.put: {} ctx_id={} readonly={} key_len={} value_len={} key_prefix={} value_prefix={}",
            storage_trace_scope(app),
            context.id,
            context.is_read_only,
            key.len(),
            value.len(),
            key_preview,
            value_preview,
        );
    }

    app.storage_put(context, key, value)
        .map_err(|e| map_storage_error("System.Storage.Put", e))?;
    Ok(())
}

fn storage_delete_handler<P, D, B>(
    app: &mut ApplicationEngine<P, D, B>,
    _engine: &mut ExecutionEngine<B>,
) -> VmResult<()>
where
    P: NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    let context_item = app
        .pop()
        .map_err(|e| map_storage_error("System.Storage.Delete", e))?;
    let context = StorageContext::from_stack_item(&context_item)
        .map_err(|e| map_storage_error("System.Storage.Delete", e))?;

    let key = pop_storage_bytes(app, "System.Storage.Delete", "key")?;

    if storage_trace_enabled(app, "NEO_TRACE_STORAGE_DELETE") || storage_trace_key_enabled(&key) {
        let key_preview = key
            .iter()
            .take(32)
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        eprintln!(
            "trace storage.delete: {} ctx_id={} readonly={} key_len={} key_prefix={}",
            storage_trace_scope(app),
            context.id,
            context.is_read_only,
            key.len(),
            key_preview,
        );
    }

    app.storage_delete(context, key)
        .map_err(|e| map_storage_error("System.Storage.Delete", e))?;
    Ok(())
}

fn storage_get_local_handler<P, D, B>(
    app: &mut ApplicationEngine<P, D, B>,
    _engine: &mut ExecutionEngine<B>,
) -> VmResult<()>
where
    P: NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
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

fn storage_put_local_handler<P, D, B>(
    app: &mut ApplicationEngine<P, D, B>,
    _engine: &mut ExecutionEngine<B>,
) -> VmResult<()>
where
    P: NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    let key = pop_storage_bytes(app, "System.Storage.Local.Put", "key")?;
    let value = pop_storage_bytes(app, "System.Storage.Local.Put", "value")?;

    app.storage_put_local(key, value)
        .map_err(|e| map_storage_error("System.Storage.Local.Put", e))?;
    Ok(())
}

fn storage_delete_local_handler<P, D, B>(
    app: &mut ApplicationEngine<P, D, B>,
    _engine: &mut ExecutionEngine<B>,
) -> VmResult<()>
where
    P: NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    let key = app
        .pop_bytes()
        .map_err(|e| map_storage_error("System.Storage.Local.Delete", e))?;
    app.storage_delete_local(key)
        .map_err(|e| map_storage_error("System.Storage.Local.Delete", e))?;
    Ok(())
}

fn storage_find_local_handler<P, D, B>(
    app: &mut ApplicationEngine<P, D, B>,
    _engine: &mut ExecutionEngine<B>,
) -> VmResult<()>
where
    P: NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    let prefix = app
        .pop_bytes()
        .map_err(|e| map_storage_error("System.Storage.Local.Find", e))?;

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

    let iterator = app
        .storage_find_local(prefix, options)
        .map_err(|e| map_storage_error("System.Storage.Local.Find", e))?;

    let iterator_id = app
        .store_storage_iterator(iterator)
        .map_err(|e| map_storage_error("System.Storage.Local.Find", e))?;
    app.push(StackItem::from_interface(IteratorInterop::iterator(
        iterator_id,
    )))
    .map_err(|e| map_storage_error("System.Storage.Local.Find", e))?;
    Ok(())
}

fn storage_find_handler<P, D, B>(
    app: &mut ApplicationEngine<P, D, B>,
    _engine: &mut ExecutionEngine<B>,
) -> VmResult<()>
where
    P: NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
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

    if storage_trace_enabled(app, "NEO_TRACE_STORAGE_FIND") || storage_trace_key_enabled(&prefix) {
        let prefix_preview = prefix
            .iter()
            .take(32)
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        eprintln!(
            "trace storage.find: {} ctx_id={} readonly={} prefix_len={} prefix={} options={:?}",
            storage_trace_scope(app),
            context.id,
            context.is_read_only,
            prefix.len(),
            prefix_preview,
            options,
        );
    }

    let iterator = app
        .storage_find(context, prefix.clone(), options)
        .map_err(|e| map_storage_error("System.Storage.Find", e))?;

    let iterator_id = app
        .store_storage_iterator(iterator)
        .map_err(|e| map_storage_error("System.Storage.Find", e))?;
    app.push(StackItem::from_interface(IteratorInterop::iterator(
        iterator_id,
    )))
    .map_err(|e| map_storage_error("System.Storage.Find", e))?;
    Ok(())
}

impl<P, D, B> ApplicationEngine<P, D, B>
where
    P: NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
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

#[cfg(test)]
#[path = "../tests/interop/application_engine_storage.rs"]
mod tests;
