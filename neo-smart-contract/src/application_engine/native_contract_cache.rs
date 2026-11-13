use super::{NativeContract, NativeMethod};
use crate::contract_state::ContractState;
use crate::error::CoreError as Error;
use crate::error::CoreResult as Result;
use neo_primitives::UInt160;
use neo_protocol::ProtocolSettings;
use std::collections::HashMap;
use std::sync::Arc;

/// Cache of native contract method metadata, mirroring the C# NativeContractsCache behaviour.
#[derive(Default)]
pub struct NativeContractsCache {
    entries: HashMap<i32, NativeContractsCacheEntry>,
    /// Committed-state Faun fee decisions shared by persistence engines.
    /// Engines that mutate contract or policy state use a private overlay instead.
    fee_whitelist: HashMap<(UInt160, u32), HashMap<String, Option<i64>>>,
    /// Immutable deployed contract records shared by persistence engines. The
    /// scripts and manifests stay alive across transactions without VM aliasing.
    contract_states: HashMap<UInt160, Arc<ContractState>>,
    /// Immutable ContractManagement.getContract results, including absence.
    contract_query_results: HashMap<UInt160, Option<Vec<u8>>>,
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

    pub(super) fn whitelisted_fee(
        &self,
        hash: UInt160,
        method: &str,
        parameter_count: u32,
    ) -> Option<Option<i64>> {
        self.fee_whitelist
            .get(&(hash, parameter_count))
            .and_then(|methods| methods.get(method))
            .copied()
    }

    pub(super) fn cache_whitelisted_fee(
        &mut self,
        hash: UInt160,
        method: &str,
        parameter_count: u32,
        fee: Option<i64>,
    ) {
        self.fee_whitelist
            .entry((hash, parameter_count))
            .or_default()
            .insert(method.to_owned(), fee);
    }

    pub(super) fn invalidate_fee_whitelist(&mut self) {
        self.fee_whitelist.clear();
    }

    pub(super) fn contract_state(&self, hash: &UInt160) -> Option<Arc<ContractState>> {
        self.contract_states.get(hash).cloned()
    }

    pub(super) fn cache_contract_state(&mut self, hash: UInt160, contract: Arc<ContractState>) {
        self.contract_states.insert(hash, contract);
    }

    pub(super) fn contract_query_result(&self, hash: &UInt160) -> Option<Option<Vec<u8>>> {
        self.contract_query_results.get(hash).cloned()
    }

    pub(super) fn cache_contract_query_result(&mut self, hash: UInt160, result: Option<Vec<u8>>) {
        self.contract_query_results.insert(hash, result);
    }

    pub(super) fn invalidate_contract_states(&mut self) {
        self.contract_states.clear();
        self.contract_query_results.clear();
        self.invalidate_fee_whitelist();
    }
}

/// Cached metadata for a single native contract.
pub struct NativeContractsCacheEntry {
    methods_by_name: HashMap<String, Vec<Arc<NativeMethod>>>,
}

impl NativeContractsCacheEntry {
    fn from_contract(contract: &dyn NativeContract) -> Self {
        let mut methods_by_name: HashMap<String, Vec<Arc<NativeMethod>>> = HashMap::new();
        for method in contract.methods() {
            methods_by_name
                .entry(method.name.clone())
                .or_default()
                .push(Arc::new(method.clone()));
        }

        Self { methods_by_name }
    }

    fn find_method(
        &self,
        name: &str,
        parameter_count: usize,
        settings: &ProtocolSettings,
        block_height: u32,
    ) -> Result<Option<&Arc<NativeMethod>>> {
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
                "Ambiguous native method '{name}({parameter_count})' at height {block_height}"
            )));
        }

        Ok(Some(selected))
    }

    /// Gets the method metadata entry matching `name`, `parameter_count`, and activation state.
    pub fn get_method(
        &self,
        name: &str,
        parameter_count: usize,
        settings: &ProtocolSettings,
        block_height: u32,
    ) -> Result<Option<&NativeMethod>> {
        Ok(self
            .find_method(name, parameter_count, settings, block_height)?
            .map(AsRef::as_ref))
    }

    /// Gets shared immutable metadata so callers can release the cache lock
    /// without deep-cloning method names and parameter vectors on every call.
    pub fn get_method_shared(
        &self,
        name: &str,
        parameter_count: usize,
        settings: &ProtocolSettings,
        block_height: u32,
    ) -> Result<Option<Arc<NativeMethod>>> {
        Ok(self
            .find_method(name, parameter_count, settings, block_height)?
            .cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_primitives::ContractParameterType;
    use neo_protocol::Hardfork;

    fn overload(
        cpu_fee: i64,
        active: Option<Hardfork>,
        deprecated: Option<Hardfork>,
    ) -> Arc<NativeMethod> {
        let mut method = NativeMethod::new(
            "balanceOf".to_string(),
            cpu_fee,
            true,
            0,
            vec![ContractParameterType::Hash160],
            ContractParameterType::Integer,
        )
        .with_parameter_names(vec!["account".to_string()]);
        method.active_in = active;
        method.deprecated_in = deprecated;
        Arc::new(method)
    }

    #[test]
    fn shared_lookup_preserves_overload_activation_and_metadata_identity() {
        let legacy = overload(1, None, Some(Hardfork::HfFaun));
        let faun = overload(2, Some(Hardfork::HfFaun), None);
        let entry = NativeContractsCacheEntry {
            methods_by_name: HashMap::from([(
                "balanceOf".to_string(),
                vec![Arc::clone(&legacy), Arc::clone(&faun)],
            )]),
        };
        let mut settings = ProtocolSettings::default();
        settings.hardforks.insert(Hardfork::HfFaun, 10);

        let before = entry
            .get_method_shared("balanceOf", 1, &settings, 9)
            .expect("legacy lookup")
            .expect("legacy method");
        let after = entry
            .get_method_shared("balanceOf", 1, &settings, 10)
            .expect("Faun lookup")
            .expect("Faun method");
        let after_again = entry
            .get_method_shared("balanceOf", 1, &settings, 11)
            .expect("repeated Faun lookup")
            .expect("Faun method");

        assert!(Arc::ptr_eq(&before, &legacy));
        assert!(Arc::ptr_eq(&after, &faun));
        assert!(Arc::ptr_eq(&after, &after_again));
        assert_eq!(before.cpu_fee, 1);
        assert_eq!(after.cpu_fee, 2);
        assert_eq!(after.parameters, vec![ContractParameterType::Hash160]);
        assert_eq!(after.parameter_names, vec!["account".to_string()]);
        assert!(after.safe);
        assert_eq!(after.return_type, ContractParameterType::Integer);
        assert!(std::ptr::eq(
            entry
                .get_method("balanceOf", 1, &settings, 10)
                .expect("borrowed lookup")
                .expect("borrowed method"),
            after.as_ref()
        ));
        assert!(
            entry
                .get_method_shared("balanceOf", 2, &settings, 10)
                .expect("parameter-count lookup")
                .is_none()
        );
    }

    #[test]
    fn shared_lookup_still_rejects_ambiguous_active_overloads() {
        let method = overload(1, None, None);
        let entry = NativeContractsCacheEntry {
            methods_by_name: HashMap::from([(
                "balanceOf".to_string(),
                vec![Arc::clone(&method), method],
            )]),
        };

        let error = entry
            .get_method_shared("balanceOf", 1, &ProtocolSettings::default(), 0)
            .expect_err("duplicate active overloads must fail");
        assert!(error.to_string().contains("Ambiguous native method"));
    }
}
