//! Standard native-contract provider.
//!
//! Implements neo-execution's [`NativeContractProvider`] seam over the concrete
//! native contracts in this crate and installs it into the engine's global
//! provider slot. This is the link that lets `ApplicationEngine` dispatch
//! `System.Contract.Call` to a native contract without `neo-execution`
//! depending on `neo-native-contracts` (which would be a crate cycle).
//!
//! The canonical catalog in [`crate::catalog`] is the single source of truth for
//! standard-contract order, id, name, hash, and construction.

use std::sync::Arc;

use neo_execution::NativeContract;
use neo_execution::native_contract_provider::{NativeContractLookup, NativeContractProvider};
use neo_primitives::UInt160;

use crate::LedgerContract;
use crate::catalog::{
    StandardNativeContractSpec, standard_native_contract_hashes,
    standard_native_contract_spec_by_hash, standard_native_contract_spec_by_name,
    standard_native_contracts,
};

/// Provider over every standard native contract, in canonical C# id order.
pub struct StandardNativeProvider {
    contracts: Vec<Arc<dyn NativeContract>>,
}

impl StandardNativeProvider {
    /// Builds the provider from the canonical standard native-contract catalog.
    pub fn new() -> Self {
        Self {
            contracts: standard_native_contracts(),
        }
    }

    fn contract_for_spec(
        &self,
        spec: StandardNativeContractSpec,
    ) -> Option<Arc<dyn NativeContract>> {
        self.contracts
            .iter()
            .find(|contract| contract.id() == spec.id)
            .cloned()
    }
}

neo_io::impl_default_via_new!(StandardNativeProvider);

impl NativeContractProvider for StandardNativeProvider {
    fn get_native_contract(&self, hash: &UInt160) -> Option<Arc<dyn NativeContract>> {
        standard_native_contract_spec_by_hash(hash).and_then(|spec| self.contract_for_spec(spec))
    }

    fn get_native_contract_by_name(&self, name: &str) -> Option<Arc<dyn NativeContract>> {
        standard_native_contract_spec_by_name(name).and_then(|spec| self.contract_for_spec(spec))
    }

    fn all_native_contracts(&self) -> Vec<Arc<dyn NativeContract>> {
        self.contracts.clone()
    }

    fn all_native_contract_hashes(&self) -> Vec<UInt160> {
        standard_native_contract_hashes().into_iter().collect()
    }

    fn current_block_index(&self, snapshot: &neo_storage::DataCache) -> neo_error::CoreResult<u32> {
        LedgerContract::new().current_index(snapshot)
    }
}

/// Installs the standard native-contract provider into neo-execution's global
/// seam. Call once at process startup (and freely from tests); installing again
/// replaces the previous provider.
pub fn install() {
    NativeContractLookup::install_provider(Arc::new(StandardNativeProvider::new()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hashes::CRYPTO_LIB_HASH;

    #[test]
    fn provider_registers_exact_standard_catalog() {
        let provider = StandardNativeProvider::new();
        let specs = crate::standard_native_contract_specs();
        let contracts = provider.all_native_contracts();

        assert_eq!(contracts.len(), crate::STANDARD_NATIVE_CONTRACT_COUNT);
        assert_eq!(contracts.len(), specs.len());

        for (contract, spec) in contracts.iter().zip(specs) {
            assert_eq!(contract.id(), spec.id, "{} id", spec.name);
            assert_eq!(contract.name(), spec.name, "{} name", spec.name);
            assert_eq!(contract.hash(), spec.hash, "{} hash", spec.name);
            assert_eq!(
                provider
                    .get_native_contract(&spec.hash)
                    .expect("hash lookup")
                    .name(),
                spec.name,
                "{} hash lookup",
                spec.name
            );
            assert_eq!(
                provider
                    .get_native_contract_by_name(spec.name)
                    .expect("name lookup")
                    .hash(),
                spec.hash,
                "{} name lookup",
                spec.name
            );
        }

        let expected_hashes = specs.iter().map(|spec| spec.hash).collect::<Vec<_>>();
        assert_eq!(provider.all_native_contract_hashes(), expected_hashes);
    }

    #[test]
    fn provider_resolves_cryptolib_by_name_and_hash() {
        let provider = StandardNativeProvider::new();

        let by_name = provider
            .get_native_contract_by_name("CryptoLib")
            .expect("CryptoLib registered");
        assert_eq!(by_name.hash(), *CRYPTO_LIB_HASH);
        assert_eq!(by_name.id(), -3);

        let by_hash = provider
            .get_native_contract(&CRYPTO_LIB_HASH)
            .expect("CryptoLib resolvable by hash");
        assert_eq!(by_hash.name(), "CryptoLib");

        assert!(provider.get_native_contract_by_name("crypTOlib").is_some());
    }

    #[test]
    fn install_wires_global_provider() {
        install();
        let resolved =
            neo_execution::native_contract_provider::NativeContractLookup::get_native_contract(
                &CRYPTO_LIB_HASH,
            );
        assert!(
            resolved.is_some(),
            "global provider resolves CryptoLib after install()"
        );
        assert_eq!(resolved.unwrap().name(), "CryptoLib");
    }

    #[test]
    fn provider_current_block_index_feeds_engine_without_persisting_block() {
        use crate::LedgerContract;
        use neo_config::ProtocolSettings;
        use neo_execution::ApplicationEngine;
        use neo_payloads::Block;
        use neo_primitives::{TriggerType, UInt256};
        use neo_storage::StorageItem;
        use neo_storage::persistence::DataCache;
        use std::sync::Arc;

        install();
        let cache = Arc::new(DataCache::new(false));
        let current_hash = UInt256::from_bytes(&[0x34; 32]).unwrap();
        cache.add(
            LedgerContract::current_block_storage_key(),
            StorageItem::from_bytes(
                LedgerContract::new()
                    .serialize_hash_index_state(&current_hash, 1234)
                    .unwrap(),
            ),
        );

        let engine = ApplicationEngine::new(
            TriggerType::Application,
            None,
            Arc::clone(&cache),
            None::<Block>,
            ProtocolSettings::default(),
            1_000_000,
            None,
        )
        .expect("engine builds");

        assert_eq!(engine.current_block_index(), 1234);
    }
}
