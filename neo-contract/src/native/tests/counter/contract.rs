use alloc::vec::Vec;

use neo_base::Bytes;
use neo_core::tx::SignerScopes;
use neo_store::Store;

use crate::{
    error::ContractError,
    manifest::{ContractManifest, ContractMethod},
    runtime::{execution::ExecutionContext, InvocationResult, Value},
};

use super::fixtures::Column;

pub(super) struct CounterContract {
    pub manifest: ContractManifest,
}

impl CounterContract {
    fn increment(
        &self,
        ctx: &mut ExecutionContext<'_>,
        key: &[u8],
    ) -> Result<InvocationResult, ContractError> {
        let current = ctx
            .load(Column::ID, key)?
            .map(|v| i64::from_le_bytes(v.try_into().unwrap_or([0u8; 8])))
            .unwrap_or(0);
        let next = current + 1;
        ctx.gas_mut().charge(10)?;
        ctx.put(Column::ID, key, next.to_le_bytes().to_vec())?;
        Ok(InvocationResult::new(
            Value::Int(next),
            ctx.gas().consumed(),
        ))
    }

    fn reset(
        &self,
        ctx: &mut ExecutionContext<'_>,
        key: &[u8],
    ) -> Result<InvocationResult, ContractError> {
        ctx.gas_mut().charge(5)?;
        ctx.delete(Column::ID, key)?;
        Ok(InvocationResult::new(
            Value::Bytes(Bytes::default()),
            ctx.gas().consumed(),
        ))
    }
}

impl crate::native::NativeContract for CounterContract {
    fn name(&self) -> &'static str {
        "Counter"
    }

    fn manifest(&self) -> &ContractManifest {
        &self.manifest
    }

    fn invoke(
        &self,
        ctx: &mut ExecutionContext<'_>,
        method: &ContractMethod,
        params: &[Value],
    ) -> Result<InvocationResult, ContractError> {
        let key = match &params[0] {
            Value::Bytes(bytes) => bytes.clone().into_vec(),
            _ => return Err(ContractError::InvalidParameters),
        };
        match method.name.as_str() {
            "increment" => self.increment(ctx, key.as_slice()),
            "reset" => self.reset(ctx, key.as_slice()),
            _ => Err(ContractError::MethodNotFound {
                method: method.name.clone(),
            }),
        }
    }
}
