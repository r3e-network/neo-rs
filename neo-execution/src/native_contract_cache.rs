use neo_error::CoreError as Error;
use neo_error::CoreResult as Result;
use neo_config::ProtocolSettings;
use crate::{NativeContract, NativeMethod};
use std::collections::HashMap;

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
    methods_by_name: HashMap<String, Vec<NativeMethod>>,
}

impl NativeContractsCacheEntry {
    fn from_contract(contract: &dyn NativeContract) -> Self {
        let mut methods_by_name: HashMap<String, Vec<NativeMethod>> = HashMap::new();
        for method in contract.methods() {
            methods_by_name
                .entry(method.name.clone())
                .or_default()
                .push(method.clone());
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
    ) -> Result<Option<&NativeMethod>> {
        let Some(candidates) = self.methods_by_name.get(name) else {
            return Ok(None);
        };

        let mut active = candidates.iter().filter(|method| {
            method.parameters.len() == parameter_count && method.is_active(settings, block_height)
        });

        let Some(selected) = active.next() else {
            return Ok(None);
        };

        if active.next().is_some() {
            return Err(Error::invalid_operation(format!(
                "Ambiguous native method '{}({})' at height {}",
                name, parameter_count, block_height
            )));
        }

        Ok(Some(selected))
    }
}
