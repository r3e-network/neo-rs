use super::*;
pub(super) fn storage_find_local_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    // FindLocal(prefix, options) follows the same declaration-order popping
    // convention as Find(context, prefix, options).
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
    app.push(StackItem::from_interface(IteratorInterop::new(iterator_id)))
        .map_err(|e| map_storage_error("System.Storage.Local.Find", e))?;
    Ok(())
}
pub(super) fn storage_find_handler(
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
pub(super) fn storage_find_with_start_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    // FindWithStart(context, prefix, start, options) pops in declaration order.
    let context_item = app
        .pop()
        .map_err(|e| map_storage_error("System.Storage.FindWithStart", e))?;
    let context = StorageContext::from_stack_item(&context_item)
        .map_err(|e| map_storage_error("System.Storage.FindWithStart", e))?;
    let prefix = app
        .pop_bytes()
        .map_err(|e| map_storage_error("System.Storage.FindWithStart", e))?;
    let start = app
        .pop_bytes()
        .map_err(|e| map_storage_error("System.Storage.FindWithStart", e))?;
    let options_bits = app
        .pop_integer()
        .map_err(|e| map_storage_error("System.Storage.FindWithStart", e))?;
    let options = FindOptions::from_bits(options_bits as u8).ok_or_else(|| {
        map_storage_error(
            "System.Storage.FindWithStart",
            format!("Invalid FindOptions value: {options_bits}"),
        )
    })?;
    options
        .validate()
        .map_err(|e| map_storage_error("System.Storage.FindWithStart", e))?;
    let iterator = app
        .storage_find_with_start(context, prefix, start, options)
        .map_err(|e| map_storage_error("System.Storage.FindWithStart", e))?;
    let iterator_id = app
        .store_storage_iterator(iterator)
        .map_err(|e| map_storage_error("System.Storage.FindWithStart", e))?;
    app.push(StackItem::from_interface(IteratorInterop::new(iterator_id)))
        .map_err(|e| map_storage_error("System.Storage.FindWithStart", e))?;
    Ok(())
}
pub(super) fn storage_find_local_with_start_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    // FindLocalWithStart(prefix, start, options) follows the same
    // declaration-order popping convention as FindWithStart.
    let prefix = app
        .pop_bytes()
        .map_err(|e| map_storage_error("System.Storage.Local.FindWithStart", e))?;
    let start = app
        .pop_bytes()
        .map_err(|e| map_storage_error("System.Storage.Local.FindWithStart", e))?;
    let options_bits = app
        .pop_integer()
        .map_err(|e| map_storage_error("System.Storage.Local.FindWithStart", e))?;
    let options = FindOptions::from_bits(options_bits as u8).ok_or_else(|| {
        map_storage_error(
            "System.Storage.Local.FindWithStart",
            format!("Invalid FindOptions value: {options_bits}"),
        )
    })?;
    options
        .validate()
        .map_err(|e| map_storage_error("System.Storage.Local.FindWithStart", e))?;
    let iterator = app
        .storage_find_local_with_start(prefix, start, options)
        .map_err(|e| map_storage_error("System.Storage.Local.FindWithStart", e))?;
    let iterator_id = app
        .store_storage_iterator(iterator)
        .map_err(|e| map_storage_error("System.Storage.Local.FindWithStart", e))?;
    app.push(StackItem::from_interface(IteratorInterop::new(iterator_id)))
        .map_err(|e| map_storage_error("System.Storage.Local.FindWithStart", e))?;
    Ok(())
}
