//! Host-side state that supports `ApplicationEngine`.
//!
//! The root module owns the public engine facade. This module keeps the private
//! VM-host wrapper, syscall dispatch metadata, queued native call record, and
//! error projection helpers together so the facade reads as engine composition
//! rather than support mechanics.

use crate::{
    ApplicationExecutionContext as ExecutionContext, ApplicationExecutionEngine as ExecutionEngine,
};
use neo_error::{CoreError, CoreResult};
use neo_primitives::CallFlags;
use neo_primitives::UInt160;
use neo_vm::{StackItem, VmError};

use super::InteropHandler;
use crate::diagnostic::Diagnostic;
use crate::native_contract_provider::NativeContractProvider;

pub(super) type StdResult<T> = CoreResult<T>;

pub(super) struct HostInteropHandler<P, D, B>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
{
    pub(super) price: i64,
    pub(super) required_call_flags: CallFlags,
    pub(super) handler: InteropHandler<P, D, B>,
}

impl<P, D, B> Copy for HostInteropHandler<P, D, B>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
{
}

impl<P, D, B> Clone for HostInteropHandler<P, D, B>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
{
    fn clone(&self) -> Self {
        *self
    }
}

pub(super) fn map_core_error_to_vm_error(error: CoreError) -> VmError {
    match error {
        CoreError::InsufficientGas {
            required,
            available,
        } => VmError::gas_exhausted(required, available),
        other => VmError::invalid_operation_msg(other.to_string()),
    }
}

pub(super) struct VmEngineHost<B> {
    engine: ExecutionEngine<B>,
}

impl<B> VmEngineHost<B> {
    pub(super) fn new(engine: ExecutionEngine<B>) -> Self {
        Self { engine }
    }

    pub(super) fn engine(&self) -> &ExecutionEngine<B> {
        &self.engine
    }

    pub(super) fn engine_mut(&mut self) -> &mut ExecutionEngine<B> {
        &mut self.engine
    }

    pub(super) fn current_context(&self) -> Option<&ExecutionContext<B>> {
        self.engine.current_context()
    }
}

/// Represents a contract call queued by a native contract.
///
/// Native contracts sometimes need to invoke a user contract (e.g., NEP-17
/// `onNEP17Payment`) but cannot safely switch the current VM context while the
/// native syscall is still executing. Instead, we queue the call and let the
/// `System.Contract.CallNative` handler load it after the native method has
/// returned its value.
#[derive(Clone, Debug)]
pub(super) struct PendingNativeCall {
    pub(super) calling_script_hash: UInt160,
    pub(super) contract_hash: UInt160,
    pub(super) method: String,
    pub(super) args: Vec<StackItem>,
}
