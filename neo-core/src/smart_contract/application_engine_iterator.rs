//! ApplicationEngine.Iterator - matches C# Neo.SmartContract.ApplicationEngine.Iterator.cs

use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::call_flags::CallFlags;
use neo_vm::{ExecutionEngine, VmError, VmResult};

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

    let value = app
        .iterator_value_internal(iterator_id)
        .map_err(|e| map_iterator_error("System.Iterator.Value", e))?;

    app.push(value)
        .map_err(|e| map_iterator_error("System.Iterator.Value", e))?;
    Ok(())
}

pub(crate) fn register_iterator_interops(engine: &mut ApplicationEngine) -> VmResult<()> {
    engine.register_host_service(
        "System.Iterator.Next",
        1 << 15,
        CallFlags::NONE,
        iterator_next_handler,
    )?;
    engine.register_host_service(
        "System.Iterator.Value",
        1 << 4,
        CallFlags::NONE,
        iterator_value_handler,
    )?;
    Ok(())
}
