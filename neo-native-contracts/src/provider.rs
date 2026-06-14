//! Standard native-contract provider.
//!
//! Implements neo-execution's [`NativeContractProvider`] seam over the concrete
//! native contracts in this crate and installs it into the engine's global
//! provider slot. This is the link that lets `ApplicationEngine` dispatch
//! `System.Contract.Call` to a native contract without `neo-execution`
//! depending on `neo-native-contracts` (which would be a crate cycle).
//!
//! Only contracts that implement the [`NativeContract`] trait are registered;
//! contracts still being ported are simply absent from the provider until their
//! `invoke`/`methods` are implemented.

use std::sync::Arc;

use neo_execution::NativeContract;
use neo_execution::native_contract_provider::{NativeContractProvider, install_provider};
use neo_primitives::UInt160;

use crate::LedgerContract;
use crate::catalog::standard_native_contracts;

/// Provider over the implemented standard native contracts, in canonical
/// (ascending-id-magnitude) registration order.
pub struct StandardNativeProvider {
    contracts: Vec<Arc<dyn NativeContract>>,
}

impl StandardNativeProvider {
    /// Builds the provider with every native contract that currently
    /// implements the [`NativeContract`] trait.
    pub fn new() -> Self {
        Self {
            contracts: standard_native_contracts(),
        }
    }
}

neo_io::impl_default_via_new!(StandardNativeProvider);

impl NativeContractProvider for StandardNativeProvider {
    fn get_native_contract(&self, hash: &UInt160) -> Option<Arc<dyn NativeContract>> {
        self.contracts.iter().find(|c| &c.hash() == hash).cloned()
    }

    fn get_native_contract_by_name(&self, name: &str) -> Option<Arc<dyn NativeContract>> {
        self.contracts
            .iter()
            .find(|c| c.name().eq_ignore_ascii_case(name))
            .cloned()
    }

    fn all_native_contracts(&self) -> Vec<Arc<dyn NativeContract>> {
        self.contracts.clone()
    }

    fn all_native_contract_hashes(&self) -> Vec<UInt160> {
        self.contracts.iter().map(|c| c.hash()).collect()
    }

    fn current_block_index(&self, snapshot: &neo_storage::DataCache) -> neo_error::CoreResult<u32> {
        LedgerContract::new().current_index(snapshot)
    }
}

/// Installs the standard native-contract provider into neo-execution's global
/// seam. Call once at process startup (and freely from tests); installing again
/// replaces the previous provider.
pub fn install() {
    install_provider(Arc::new(StandardNativeProvider::new()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hashes::CRYPTO_LIB_HASH;

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

        // Both implemented contracts are registered (StdLib + CryptoLib).
        let hashes = provider.all_native_contract_hashes();
        assert!(hashes.contains(&*CRYPTO_LIB_HASH));
        assert!(hashes.contains(&*crate::hashes::STDLIB_HASH));
        assert!(provider.get_native_contract_by_name("StdLib").is_some());
    }

    #[test]
    fn install_wires_global_provider() {
        install();
        let resolved =
            neo_execution::native_contract_provider::get_native_contract(&CRYPTO_LIB_HASH);
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
        use neo_storage::persistence::DataCache;
        use neo_storage::{StorageItem, StorageKey};
        use std::sync::Arc;

        install();
        let cache = Arc::new(DataCache::new(false));
        let current_hash = UInt256::from_bytes(&[0x34; 32]).unwrap();
        cache.add(
            StorageKey::new(LedgerContract::ID, vec![12]),
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
