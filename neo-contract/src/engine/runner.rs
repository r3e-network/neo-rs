//! Minimal ApplicationEngine scaffold for the root `neo-*` crates.
//!
//! It currently hydrates an `ExecutionContext`, ensuring transaction signers and
//! script hashes are loaded. Actual VM execution will be wired in once the new
//! runtime is ready.

use neo_core::{script::Script, tx::Tx};
use neo_store::Store;
use neo_vm::{NativeInvoker, VirtualMachine, VmError, VmValue};

use crate::{
    runtime::{ExecutionContext, Value},
    script_decoder::decode_script,
    InvocationResult,
};

use super::EngineConfig;

/// Lightweight ApplicationEngine placeholder.
pub struct ApplicationEngine<'a> {
    ctx: ExecutionContext<'a>,
    _config: EngineConfig<'a>,
}

impl<'a> ApplicationEngine<'a> {
    pub fn new(store: &'a mut dyn Store, config: EngineConfig<'a>, container: Option<&Tx>) -> Self {
        let mut ctx =
            ExecutionContext::with_timestamp(store, config.gas_limit, None, config.timestamp);
        match container {
            Some(tx) => ctx.load_transaction_context(tx),
            None => ctx.set_signers_from(config.signers),
        }
        if let Some(hash) = config.calling_script_hash {
            ctx.set_calling_script_hash(Some(hash));
        }
        ctx.set_trigger(config.trigger);
        ctx.set_platform(config.platform);
        if !config.current_contract_groups.is_empty() {
            ctx.set_current_contract_groups(config.current_contract_groups.to_vec());
        }
        if !config.calling_contract_groups.is_empty() {
            ctx.set_calling_contract_groups(config.calling_contract_groups.to_vec());
        }
        Self {
            ctx,
            _config: config,
        }
    }

    /// Executes the provided script and returns its result.
    pub fn execute_script(&mut self, script: &Script) -> Result<InvocationResult, VmError> {
        self.ctx.set_script_from_core(script);
        let program = decode_script(script).map_err(|_| VmError::Fault)?;
        let mut invoker = NoopInvoker;
        let value =
            VirtualMachine::with_context(&program, &mut invoker, &mut self.ctx).execute()?;
        let gas_used = self.ctx.gas().consumed();
        let logs = self.ctx.drain_logs();
        let notifications = self.ctx.drain_notifications();
        Ok(InvocationResult::new(Value::from(value), gas_used).with_events(logs, notifications))
    }
}

struct NoopInvoker;

impl NativeInvoker for NoopInvoker {
    fn invoke(
        &mut self,
        _contract: &str,
        _method: &str,
        _args: &[VmValue],
    ) -> Result<VmValue, VmError> {
        Ok(VmValue::Null)
    }
}
