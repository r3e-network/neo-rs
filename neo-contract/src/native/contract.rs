use crate::{
    error::ContractError,
    manifest::{ContractManifest, ContractMethod},
    runtime::{ExecutionContext, InvocationResult, Value},
};

/// Trait implemented by each native contract so the registry can dispatch calls.
pub trait NativeContract: Send + Sync {
    fn name(&self) -> &'static str;
    fn manifest(&self) -> &ContractManifest;
    fn invoke(
        &self,
        ctx: &mut ExecutionContext<'_>,
        method: &ContractMethod,
        params: &[Value],
    ) -> Result<InvocationResult, ContractError>;
}
