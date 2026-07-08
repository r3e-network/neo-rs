//! Native-contract capabilities for empty-block fast-forward staging.
//!
//! The fast-forward stage belongs to `neo-blockchain`, but the state transition
//! math belongs to `neo-native-contracts`. This provider seam keeps staging
//! dependent on a narrow native capability instead of constructing native
//! contracts directly in the pipeline code.

use neo_config::ProtocolSettings;
use neo_error::CoreResult;
use neo_native_contracts::NeoToken;
use neo_storage::DataCache;

/// Native-contract capabilities required by empty-block fast-forward staging.
pub(super) trait EmptyBlockFastForwardNativeProvider {
    /// Applies byte-equivalent native storage effects for a contiguous empty run.
    fn fast_forward_empty_block_rewards(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
        start: u32,
        end: u32,
    ) -> CoreResult<()>;
}

/// Factory for empty-block fast-forward native providers.
pub(super) trait EmptyBlockFastForwardNativeProviderFactory {
    /// Provider returned by this factory.
    type Provider: EmptyBlockFastForwardNativeProvider;

    /// Creates a provider instance.
    fn provider(&self) -> Self::Provider;
}

/// Production provider backed by canonical native-contract handles.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeEmptyBlockFastForwardProvider {
    neo: NeoToken,
}

impl NativeEmptyBlockFastForwardProvider {
    /// Creates a provider backed by canonical native-contract handles.
    #[must_use]
    pub(super) const fn new() -> Self {
        Self {
            neo: NeoToken::new(),
        }
    }
}

impl EmptyBlockFastForwardNativeProvider for NativeEmptyBlockFastForwardProvider {
    fn fast_forward_empty_block_rewards(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
        start: u32,
        end: u32,
    ) -> CoreResult<()> {
        self.neo
            .fast_forward_empty_block_rewards(snapshot, settings, start, end)
    }
}

/// Factory for production empty-block fast-forward native providers.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeEmptyBlockFastForwardProviderFactory;

impl EmptyBlockFastForwardNativeProviderFactory for NativeEmptyBlockFastForwardProviderFactory {
    type Provider = NativeEmptyBlockFastForwardProvider;

    fn provider(&self) -> Self::Provider {
        NativeEmptyBlockFastForwardProvider::new()
    }
}
