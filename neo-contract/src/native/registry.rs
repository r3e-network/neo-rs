use alloc::sync::Arc;

use dashmap::DashMap;

use crate::{
    error::ContractError,
    manifest::ContractMethod,
    runtime::{ExecutionContext, Value},
};

use super::NativeContract;

/// In-memory registry that keeps track of all registered native contracts.
#[derive(Default)]
pub struct NativeRegistry {
    contracts: DashMap<&'static str, Arc<dyn NativeContract>>,
}

impl NativeRegistry {
    pub fn new() -> Self {
        Self {
            contracts: DashMap::new(),
        }
    }

    pub fn register<C>(&self, contract: C)
    where
        C: NativeContract + 'static,
    {
        self.contracts.insert(contract.name(), Arc::new(contract));
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn NativeContract>> {
        self.contracts.get(name).map(|c| Arc::clone(&c))
    }

    pub fn invoke(
        &self,
        name: &str,
        method: &str,
        ctx: &mut ExecutionContext<'_>,
        params: &[Value],
    ) -> Result<crate::runtime::InvocationResult, ContractError> {
        let contract = self
            .get(name)
            .ok_or_else(|| ContractError::NativeNotFound(name.to_owned()))?;
        let manifest = contract.manifest();
        let method_def =
            manifest
                .find_method(method)
                .ok_or_else(|| ContractError::MethodNotFound {
                    method: method.to_owned(),
                })?;

        Self::validate_parameters(method_def, params)?;

        ctx.gas_mut().charge(1)?; // base call cost
        contract.invoke(ctx, method_def, params)
    }

    fn validate_parameters(
        method: &ContractMethod,
        supplied: &[Value],
    ) -> Result<(), ContractError> {
        if method.parameters.len() != supplied.len() {
            return Err(ContractError::InvalidParameters);
        }
        for (param, value) in method.parameters.iter().zip(supplied.iter()) {
            if param.kind != value.kind() {
                return Err(ContractError::InvalidParameters);
            }
        }
        Ok(())
    }
}
