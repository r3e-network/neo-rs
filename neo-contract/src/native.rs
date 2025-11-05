use alloc::sync::Arc;

use dashmap::DashMap;

use crate::{
    error::ContractError,
    manifest::{ContractManifest, ContractMethod, PermissionKind},
    runtime::{ExecutionContext, InvocationResult, Value},
};

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
    ) -> Result<InvocationResult, ContractError> {
        let contract = self
            .get(name)
            .ok_or_else(|| ContractError::NativeNotFound(name.to_owned()))?;
        let manifest = contract.manifest();
        manifest.ensure_allowed(PermissionKind::Call)?;
        let method_def = manifest
            .find_method(method)
            .ok_or_else(|| ContractError::MethodNotFound {
                method: method.to_owned(),
            })?;

        if method_def.parameters.len() != params.len() {
            return Err(ContractError::InvalidParameters);
        }
        for (param, value) in method_def.parameters.iter().zip(params.iter()) {
            if param.kind != value.kind() {
                return Err(ContractError::InvalidParameters);
            }
        }

        ctx.gas_mut().charge(1)?; // base call cost
        contract.invoke(ctx, method_def, params)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        manifest::{
            ContractManifest,
            ContractMethod,
            ContractParameter,
            ParameterKind,
            Permission,
            PermissionKind,
        },
        runtime::{ExecutionContext, Value},
    };
    use neo_base::Bytes;
    use neo_store::MemoryStore;

    struct CounterContract {
        manifest: ContractManifest,
    }

    impl CounterContract {
        const COLUMN: neo_store::ColumnId = neo_store::ColumnId::new("counter");
    }

    impl NativeContract for CounterContract {
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
            match method.name.as_str() {
                "increment" => {
                    let key = match &params[0] {
                        Value::Bytes(bytes) => bytes.clone().into_vec(),
                        _ => return Err(ContractError::InvalidParameters),
                    };
                    let current = ctx
                        .load(Self::COLUMN, &key)?
                        .map(|v| i64::from_le_bytes(v.try_into().unwrap_or([0u8; 8])))
                        .unwrap_or(0);
                    let next = current + 1;
                    ctx.gas_mut().charge(10)?;
                    ctx.put(Self::COLUMN, key.clone(), next.to_le_bytes().to_vec())?;
                    Ok(InvocationResult {
                        value: Value::Int(next),
                        gas_used: ctx.gas().consumed(),
                    })
                }
                "reset" => {
                    let key = match &params[0] {
                        Value::Bytes(bytes) => bytes.clone().into_vec(),
                        _ => return Err(ContractError::InvalidParameters),
                    };
                    ctx.gas_mut().charge(5)?;
                    ctx.delete(Self::COLUMN, &key)?;
                    Ok(InvocationResult {
                        value: Value::Bytes(Bytes::default()),
                        gas_used: ctx.gas().consumed(),
                    })
                }
                _ => Err(ContractError::MethodNotFound {
                    method: method.name.clone(),
                }),
            }
        }
    }

    #[test]
    fn invoke_native_contract() {
        let manifest = ContractManifest {
            name: "Counter".into(),
            groups: vec![],
            methods: vec![
                ContractMethod {
                    name: "increment".into(),
                    parameters: vec![ContractParameter {
                        name: "key".into(),
                        kind: ParameterKind::ByteArray,
                    }],
                    return_type: ParameterKind::Integer,
                    safe: false,
                },
                ContractMethod {
                    name: "reset".into(),
                    parameters: vec![ContractParameter {
                        name: "key".into(),
                        kind: ParameterKind::ByteArray,
                    }],
                    return_type: ParameterKind::ByteArray,
                    safe: false,
                },
            ],
            permissions: vec![Permission {
                kind: PermissionKind::Call,
                contract: None,
            }],
        };

        let registry = NativeRegistry::new();
        registry.register(CounterContract { manifest });

        let mut store = MemoryStore::new();
        store.create_column(CounterContract::COLUMN);
        let mut ctx = ExecutionContext::new(&mut store, 1_000, None);
        let key = Bytes::from(vec![1, 2, 3, 4]);
        let result = registry
            .invoke(
                "Counter",
                "increment",
                &mut ctx,
                &[Value::Bytes(key.clone())],
            )
            .expect("call succeeds");
        assert_eq!(result.value, Value::Int(1));
        let result_two = registry
            .invoke(
                "Counter",
                "increment",
                &mut ctx,
                &[Value::Bytes(key.clone())],
            )
            .expect("second call succeeds");
        assert_eq!(result_two.value, Value::Int(2));

        let reset = registry
            .invoke(
                "Counter",
                "reset",
                &mut ctx,
                &[Value::Bytes(key.clone())],
            )
            .expect("reset succeeds");
        assert_eq!(reset.value, Value::Bytes(Bytes::default()));
        assert!(ctx
            .load(CounterContract::COLUMN, key.as_ref())
            .expect("load succeeds")
            .is_none());
    }
}
