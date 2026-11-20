//! Native-contract hydration at node startup.
//!
//! Starting a node on an existing database must restore the in-memory caches
//! that native contracts keep for persisted metadata. Which contracts need that
//! restoration, and which trigger/limit the restoration runs under, is
//! native-contract lifecycle knowledge — so it lives here rather than in the
//! node host.
//!
//! The host keeps only the wiring: it owns the store, the protocol settings and
//! the native services, and calls [`hydrate_native_contracts`] during startup.

use crate::native::ContractManagement;
use neo_primitives::TriggerType;
use neo_protocol::ProtocolSettings;
use neo_smart_contract::application_engine::{
    ApplicationEngine, EngineNativeServices, TEST_MODE_GAS,
};
use neo_storage::persistence::PersistenceDataCache;
use std::fmt;
use std::sync::Arc;

/// Error returned when native-contract hydration fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeHydrationError {
    /// A contract that must be hydrated is not registered.
    ContractMissing {
        /// Script hash of the missing contract.
        hash: neo_primitives::UInt160,
    },
    /// The hydration engine could not be created.
    Engine(String),
    /// A contract's persistence-restoring initialization hook failed.
    Initialize(String),
}

impl fmt::Display for NativeHydrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ContractMissing { hash } => {
                write!(
                    f,
                    "native contract {hash} is missing from the native registry"
                )
            }
            Self::Engine(message) => {
                write!(f, "failed to create native hydration engine: {message}")
            }
            Self::Initialize(message) => write!(f, "failed to hydrate native contract: {message}"),
        }
    }
}

impl std::error::Error for NativeHydrationError {}

/// Standard native contracts whose persisted metadata must be hydrated at
/// startup.
///
/// `ContractManagement` is the only standard native contract with a persisted
/// metadata cache. The other standard contracts are stateless handles or build
/// their method metadata on demand, so running their genesis initialize hooks
/// here would incorrectly mutate an already-running chain.
#[must_use]
pub fn metadata_hydration_contracts() -> Vec<neo_primitives::UInt160> {
    vec![ContractManagement::contract_hash()]
}

/// Restores native-contract metadata caches from persisted storage.
///
/// Loads the persisted contract state for every contract in
/// [`metadata_hydration_contracts`] and lets it rebuild its in-memory caches,
/// mirroring C# Neo's `NativeContract.Initialize` startup path.
///
/// # Errors
///
/// Returns [`NativeHydrationError::ContractMissing`] when a contract that must
/// be hydrated is not registered, [`NativeHydrationError::Engine`] when the
/// hydration engine cannot be created, and
/// [`NativeHydrationError::Initialize`] when a contract's initialization hook
/// fails.
pub fn hydrate_native_contracts(
    snapshot: &PersistenceDataCache,
    native_services: &EngineNativeServices,
    settings: &ProtocolSettings,
) -> Result<(), NativeHydrationError> {
    for hash in metadata_hydration_contracts() {
        let contract = native_services
            .registry
            .get(&hash)
            .ok_or(NativeHydrationError::ContractMissing { hash })?;
        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            None,
            Arc::new(snapshot.clone()),
            None,
            settings.clone(),
            TEST_MODE_GAS,
            None,
            native_services.clone(),
        )
        .map_err(|error| NativeHydrationError::Engine(error.to_string()))?;
        contract
            .initialize(&mut engine)
            .map_err(|error| NativeHydrationError::Initialize(error.to_string()))?;
    }

    tracing::info!(
        target: "neo::native",
        "Native contract cache hydration completed"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{NativeHydrationError, metadata_hydration_contracts};
    use crate::native::ContractManagement;

    #[test]
    fn only_contract_management_is_hydrated() {
        assert_eq!(
            metadata_hydration_contracts(),
            vec![ContractManagement::contract_hash()],
            "ContractManagement is the only standard native contract with persisted metadata"
        );
    }

    #[test]
    fn missing_contract_error_names_the_hash() {
        let hash = ContractManagement::contract_hash();
        let error = NativeHydrationError::ContractMissing { hash };
        assert!(error.to_string().contains(&hash.to_string()));
    }
}
