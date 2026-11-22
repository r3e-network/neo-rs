//! ApplicationEngine.Iterator - matches C# Neo.SmartContract.ApplicationEngine.Iterator.cs
use super::*;
fn map_iterator_error(service: &str, error: String) -> VmError {
    VmError::InteropService {
        service: service.to_string(),
        error,
    }
}
fn iterator_next_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    let iterator_id = app
        .pop_iterator_id()
        .map_err(|e| map_iterator_error("System.Iterator.Next", e))?;
    let has_next = app
        .iterator_next_internal(iterator_id)
        .map_err(|e| map_iterator_error("System.Iterator.Next", e))?;
    app.push_boolean(has_next)
        .map_err(|e| map_iterator_error("System.Iterator.Next", e))?;
    Ok(())
}
fn iterator_value_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    let iterator_id = app
        .pop_iterator_id()
        .map_err(|e| map_iterator_error("System.Iterator.Value", e))?;
    let (value, bytes_deserialized) = app
        .iterator_value_with_bytes(iterator_id)
        .map_err(|e| map_iterator_error("System.Iterator.Value", e))?;
    // C# v3.11.2 `IteratorValue`: the reference count is captured after the
    // value is produced and before it is pushed.
    let refs_before = app.vm_reference_count();
    app.push(value)
        .map_err(|e| map_iterator_error("System.Iterator.Value", e))?;
    if app.is_hardfork_enabled(Hardfork::HfHuyao) {
        let refs_delta = app.vm_reference_count() - refs_before;
        let price = op_code_prices_v1::ITERATOR_VALUE_PRICE_PER_REF
            .checked_mul(refs_delta)
            .and_then(|value| {
                value.checked_add(
                    op_code_prices_v1::ITERATOR_VALUE_PRICE_PER_BYTE
                        .checked_mul(bytes_deserialized)?,
                )
            })
            .and_then(|value| value.checked_mul(i64::from(app.exec_fee_factor)))
            .ok_or_else(|| {
                map_iterator_error("System.Iterator.Value", "Iterator fee overflow".to_string())
            })?;
        app.add_femto_gas(price, true)
            .map_err(|e| map_iterator_error("System.Iterator.Value", e.to_string()))?;
    }
    Ok(())
}
pub(crate) fn register_iterator_interops(
    engine: &mut dyn super::state::InteropRegistration,
) -> VmResult<()> {
    engine.register_host_service_huyao(
        "System.Iterator.Next",
        1 << 15,
        op_code_prices_v1::ITERATOR_NEXT_PRICE,
        CallFlags::NONE,
        iterator_next_handler,
    )?;
    engine.register_host_service_huyao(
        "System.Iterator.Value",
        1 << 4,
        op_code_prices_v1::ITERATOR_VALUE_BASE_PRICE,
        CallFlags::NONE,
        iterator_value_handler,
    )?;
    Ok(())
}
