use crate::{NativeContract, NativeMethod};
use neo_config::ProtocolSettings;
use neo_error::CoreError;
use neo_error::CoreResult;
use std::collections::HashMap;
use std::sync::Arc;

/// Cache of native contract method metadata, mirroring the C# NativeContractsCache behaviour.
#[derive(Default)]
pub struct NativeContractsCache {
    entries: HashMap<i32, NativeContractsCacheEntry>,
}

impl NativeContractsCache {
    /// Gets the cached entry for the given native contract, building it on demand.
    pub fn get_or_build<'a>(
        &'a mut self,
        contract: &dyn NativeContract,
    ) -> &'a NativeContractsCacheEntry {
        let contract_id = contract.id();
        self.entries
            .entry(contract_id)
            .or_insert_with(|| NativeContractsCacheEntry::from_contract(contract))
    }
}

/// Cached metadata for a single native contract.
pub struct NativeContractsCacheEntry {
    methods_by_name: HashMap<String, Vec<CachedNativeMethod>>,
}

struct CachedNativeMethod {
    method_index: usize,
    method: Arc<NativeMethod>,
}

/// Native method metadata selected by the engine for an invocation.
///
/// `method_index` is the index in `NativeContract::methods()`. For standard
/// native contracts that method slice is cloned from the concrete binding table
/// in the same order, so the index can be reused to call the already-resolved
/// Rust handler without repeating name/arity/hardfork selection. The method
/// metadata is shared from the cache, so resolving a native call only clones one
/// `Arc` handle instead of deep-cloning the ABI descriptor.
#[derive(Clone)]
pub struct ResolvedNativeMethod {
    method_index: usize,
    method: Arc<NativeMethod>,
}

impl ResolvedNativeMethod {
    /// Index of the selected method in `NativeContract::methods()`.
    pub fn method_index(&self) -> usize {
        self.method_index
    }

    /// ABI metadata selected for this invocation.
    pub fn method(&self) -> &NativeMethod {
        self.method.as_ref()
    }
}

impl NativeContractsCacheEntry {
    fn from_contract(contract: &dyn NativeContract) -> Self {
        let mut methods_by_name: HashMap<String, Vec<CachedNativeMethod>> = HashMap::new();
        for (method_index, method) in contract.methods().iter().enumerate() {
            let method = Arc::new(method.clone());
            methods_by_name
                .entry(method.name.clone())
                .or_default()
                .push(CachedNativeMethod {
                    method_index,
                    method,
                });
        }

        Self { methods_by_name }
    }

    /// Gets the method metadata entry matching `name`, `parameter_count`, and activation state.
    pub fn get_method(
        &self,
        name: &str,
        parameter_count: usize,
        settings: &ProtocolSettings,
        block_height: u32,
    ) -> CoreResult<Option<ResolvedNativeMethod>> {
        let Some(candidates) = self.methods_by_name.get(name) else {
            return Ok(None);
        };

        let mut active = candidates.iter().filter(|entry| {
            entry.method.parameters.len() == parameter_count
                && entry.method.is_active(settings, block_height)
        });

        let Some(selected) = active.next() else {
            return Ok(None);
        };

        if active.next().is_some() {
            return Err(CoreError::invalid_operation(format!(
                "Ambiguous native method '{}({})' at height {}",
                name, parameter_count, block_height
            )));
        }

        Ok(Some(ResolvedNativeMethod {
            method_index: selected.method_index,
            method: selected.method.clone(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ApplicationEngine;
    use crate::native_contract::NativeContract;
    use neo_config::{Hardfork, ProtocolSettings};
    use neo_error::CoreResult;
    use neo_primitives::{ContractParameterType, UInt160};

    struct IndexedContract {
        methods: Vec<NativeMethod>,
    }

    impl NativeContract for IndexedContract {
        fn id(&self) -> i32 {
            42
        }

        fn hash(&self) -> UInt160 {
            UInt160::zero()
        }

        fn name(&self) -> &str {
            "IndexedContract"
        }

        fn methods(&self) -> &[NativeMethod] {
            &self.methods
        }

        fn invoke(
            &self,
            _engine: &mut ApplicationEngine,
            _method: &str,
            _args: &[Vec<u8>],
        ) -> CoreResult<Vec<u8>> {
            Ok(Vec::new())
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[test]
    fn resolved_method_preserves_contract_method_index() {
        let contract = IndexedContract {
            methods: vec![
                NativeMethod::new(
                    "same",
                    1,
                    true,
                    0,
                    vec![ContractParameterType::Integer],
                    ContractParameterType::Void,
                )
                .with_deprecated_in(Hardfork::HfEchidna),
                NativeMethod::new("other", 1, true, 0, vec![], ContractParameterType::Void),
                NativeMethod::new(
                    "same",
                    2,
                    true,
                    0,
                    vec![ContractParameterType::Integer],
                    ContractParameterType::Void,
                )
                .with_active_in(Hardfork::HfEchidna),
            ],
        };
        let entry = NativeContractsCacheEntry::from_contract(&contract);
        let mut settings = ProtocolSettings::default();
        settings.hardforks.insert(Hardfork::HfEchidna, 10);

        let before = entry
            .get_method("same", 1, &settings, 9)
            .expect("method resolution succeeds")
            .expect("pre-hardfork method exists");
        assert_eq!(before.method_index(), 0);
        assert_eq!(before.method().cpu_fee, 1);

        let after = entry
            .get_method("same", 1, &settings, 10)
            .expect("method resolution succeeds")
            .expect("post-hardfork method exists");
        assert_eq!(after.method_index(), 2);
        assert_eq!(after.method().cpu_fee, 2);
    }
}
