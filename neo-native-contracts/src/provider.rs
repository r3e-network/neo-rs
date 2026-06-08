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

use neo_execution::native_contract_provider::{install_provider, NativeContractProvider};
use neo_execution::NativeContract;
use neo_primitives::UInt160;

use crate::CryptoLib;

/// Provider over the implemented standard native contracts, in canonical
/// (ascending-id-magnitude) registration order.
pub struct StandardNativeProvider {
    contracts: Vec<Arc<dyn NativeContract>>,
}

impl StandardNativeProvider {
    /// Builds the provider with every native contract that currently
    /// implements the [`NativeContract`] trait.
    pub fn new() -> Self {
        let contracts: Vec<Arc<dyn NativeContract>> = vec![Arc::new(CryptoLib::new())];
        Self { contracts }
    }
}

impl Default for StandardNativeProvider {
    fn default() -> Self {
        Self::new()
    }
}

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
        assert_eq!(provider.all_native_contract_hashes(), vec![*CRYPTO_LIB_HASH]);
    }

    #[test]
    fn install_wires_global_provider() {
        install();
        let resolved = neo_execution::native_contract_provider::get_native_contract(&CRYPTO_LIB_HASH);
        assert!(resolved.is_some(), "global provider resolves CryptoLib after install()");
        assert_eq!(resolved.unwrap().name(), "CryptoLib");
    }
}
