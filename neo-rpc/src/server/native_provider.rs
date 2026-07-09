//! Shared native-contract provider adapter for RPC handlers.
//!
//! RPC handlers expose narrow, feature-local provider traits, but each of those
//! traits adapts the same composition-root native-contract registry. This helper
//! centralizes registry lookup, type downcasting, and redacted debug output so
//! individual RPC modules only describe the capabilities they need.

use neo_error::{CoreError, CoreResult};
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_native_contracts::PolicyContract;
use std::sync::Arc;

/// Adapter over the node-composed native-contract provider.
#[derive(Clone)]
pub(crate) struct NativeProviderAdapter {
    native_contract_provider: Arc<dyn NativeContractProvider>,
}

impl NativeProviderAdapter {
    /// Creates an adapter over the composition-root native-contract provider.
    #[must_use]
    pub(crate) fn new(native_contract_provider: Arc<dyn NativeContractProvider>) -> Self {
        Self {
            native_contract_provider,
        }
    }

    /// Resolves `name`, downcasts it to `T`, and invokes `f`.
    pub(crate) fn with_contract<T, R>(
        &self,
        name: &'static str,
        f: impl FnOnce(&T) -> CoreResult<R>,
    ) -> CoreResult<R>
    where
        T: 'static,
    {
        let contract = self
            .native_contract_provider
            .get_native_contract_by_name(name)
            .ok_or_else(|| {
                CoreError::invalid_operation(format!("native provider missing {name}"))
            })?;
        let typed = contract.as_any().downcast_ref::<T>().ok_or_else(|| {
            CoreError::invalid_operation(format!("native provider returned non-{name}"))
        })?;
        f(typed)
    }

    /// Resolves the canonical Policy native contract and invokes `f`.
    pub(crate) fn with_policy<R>(
        &self,
        f: impl FnOnce(&PolicyContract) -> CoreResult<R>,
    ) -> CoreResult<R> {
        self.with_contract("PolicyContract", f)
    }
}

impl std::fmt::Debug for NativeProviderAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeProviderAdapter")
            .field("native_contract_provider", &"NativeContractProvider")
            .finish()
    }
}
