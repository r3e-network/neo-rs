//! Native-contract read capabilities for extensible-payload verification.
//!
//! Extensible payload admission needs a narrow whitelist view over NEO and
//! RoleManagement state. Keeping those reads behind a local provider seam makes
//! the handler depend on capabilities instead of constructing native contracts
//! directly in the verification flow.

use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_crypto::ECPoint;
use neo_error::CoreResult;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_primitives::UInt160;
use neo_storage::{CacheRead, DataCache};

/// Native-contract capabilities required to build the extensible witness whitelist.
pub(in crate::pipeline::handlers) trait ExtensibleNativeProvider {
    /// Returns the cached committee multisig address.
    fn committee_address<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
    ) -> CoreResult<Option<UInt160>>;

    /// Returns the validators for the next block, in C# whitelist order.
    fn next_block_validators<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
    ) -> CoreResult<Vec<ECPoint>>;

    /// Returns StateValidator designated nodes effective at `height`.
    fn state_validators<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        height: u32,
    ) -> CoreResult<Vec<ECPoint>>;
}

/// Adapter from the node-composed native-contract provider to the extensible
/// verifier's narrow whitelist read capability.
#[derive(Clone)]
pub(in crate::pipeline::handlers) struct NativeExtensibleProvider<P> {
    native_contract_provider: Arc<P>,
}

impl<P> NativeExtensibleProvider<P>
where
    P: NativeContractProvider,
{
    /// Creates an adapter over the composition-root native-contract provider.
    #[must_use]
    pub(in crate::pipeline::handlers) fn new(native_contract_provider: Arc<P>) -> Self {
        Self {
            native_contract_provider,
        }
    }

    fn provider(&self) -> &P {
        self.native_contract_provider.as_ref()
    }
}

impl<P> std::fmt::Debug for NativeExtensibleProvider<P>
where
    P: NativeContractProvider,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeExtensibleProvider")
            .field("native_contract_provider", &"NativeContractProvider")
            .finish()
    }
}

impl<P> ExtensibleNativeProvider for NativeExtensibleProvider<P>
where
    P: NativeContractProvider,
{
    fn committee_address<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
    ) -> CoreResult<Option<UInt160>> {
        self.provider().committee_address(snapshot)
    }

    fn next_block_validators<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
    ) -> CoreResult<Vec<ECPoint>> {
        self.provider().next_block_validators(snapshot, settings)
    }

    fn state_validators<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        height: u32,
    ) -> CoreResult<Vec<ECPoint>> {
        self.provider().state_validators(snapshot, height)
    }
}
