//! ApplicationEngine.Iterator - matches C# Neo.SmartContract.ApplicationEngine.Iterator.cs

use crate::ApplicationExecutionEngine as ExecutionEngine;
use crate::application_engine::ApplicationEngine;
use crate::native_contract_provider::NativeContractProvider;
use neo_manifest::CallFlags;
use neo_vm::{VmError, VmResult};

fn map_iterator_error(service: &str, error: impl std::fmt::Display) -> VmError {
    VmError::InteropService {
        service: service.to_string(),
        error: error.to_string(),
    }
}

fn iterator_next_handler<P, D, B>(
    app: &mut ApplicationEngine<P, D, B>,
    _engine: &mut ExecutionEngine<B>,
) -> VmResult<()>
where
    P: NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
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

fn iterator_value_handler<P, D, B>(
    app: &mut ApplicationEngine<P, D, B>,
    _engine: &mut ExecutionEngine<B>,
) -> VmResult<()>
where
    P: NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
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

impl<P, D, B> ApplicationEngine<P, D, B>
where
    P: NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    pub(crate) fn register_iterator_interops(&mut self) -> VmResult<()> {
        self.register_host_service(
            "System.Iterator.Next",
            1 << 15,
            CallFlags::NONE,
            iterator_next_handler,
        )?;
        self.register_host_service(
            "System.Iterator.Value",
            1 << 4,
            CallFlags::NONE,
            iterator_value_handler,
        )?;
        Ok(())
    }
}
