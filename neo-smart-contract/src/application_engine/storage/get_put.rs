use super::*;
use std::fmt::Write as _;
pub(super) fn storage_get_context_handler(
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
pub(super) fn storage_get_read_only_context_handler(
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
pub(super) fn storage_as_read_only_handler(
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
pub(super) fn storage_get_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    let context_item = app
        .pop()
        .map_err(|e| map_storage_error("System.Storage.Get", e))?;
    let context = StorageContext::from_stack_item(&context_item)
        .map_err(|e| map_storage_error("System.Storage.Get", e))?;
    let key = pop_storage_bytes(app, "System.Storage.Get", "key")?;
    let trace_enabled = trace_storage_get();
    let trace_context_id = context.id;
    let trace_context_read_only = context.is_read_only;
    let trace_key_len = key.len();
    let trace_key_preview = if trace_enabled {
        Some(key.iter().take(24).fold(String::new(), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        }))
    } else {
        None
    };
    let value = app
        .storage_get(context, key)
        .map_err(|e| map_storage_error("System.Storage.Get", e))?;
    if trace_enabled {
        eprintln!(
            "trace storage.get: ctx_id={trace_context_id} readonly={trace_context_read_only} key_len={trace_key_len} key_prefix={} hit={} value_len={}",
            trace_key_preview.as_deref().unwrap_or(""),
            value.is_some(),
            value.as_ref().map_or(0, std::vec::Vec::len)
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
pub(super) fn storage_put_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    let context_item = app
        .pop()
        .map_err(|e| map_storage_error("System.Storage.Put", e))?;
    let context = StorageContext::from_stack_item(&context_item)
        .map_err(|e| map_storage_error("System.Storage.Put", e))?;
    let key = pop_storage_bytes(app, "System.Storage.Put", "key")?;
    let value = pop_storage_bytes(app, "System.Storage.Put", "value")?;
    if trace_storage_put() {
        let key_preview = key.iter().take(16).fold(String::new(), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        });
        eprintln!(
            "trace storage.put: ctx_id={} readonly={} key_len={} value_len={} key_prefix={key_preview} key={} value={}",
            context.id,
            context.is_read_only,
            key.len(),
            value.len(),
            hex::encode(&key),
            hex::encode(&value),
        );
    }
    app.storage_put(context, key, value)
        .map_err(|e| map_storage_error("System.Storage.Put", e))?;
    Ok(())
}
pub(super) fn storage_delete_handler(
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
pub(super) fn storage_get_local_handler(
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
pub(super) fn storage_put_local_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    // Interop arguments are popped in declaration order: PutLocal(key, value).
    let key = pop_storage_bytes(app, "System.Storage.Local.Put", "key")?;
    let value = pop_storage_bytes(app, "System.Storage.Local.Put", "value")?;
    if trace_storage_put() {
        eprintln!(
            "trace storage.local.put: current={:?} key={} value={}",
            app.current_script_hash(),
            hex::encode(&key),
            hex::encode(&value),
        );
    }
    app.storage_put_local(key, value)
        .map_err(|e| map_storage_error("System.Storage.Local.Put", e))?;
    Ok(())
}
pub(super) fn storage_delete_local_handler(
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
