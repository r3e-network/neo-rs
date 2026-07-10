//! Native contract read capabilities used during mempool admission.
//!
//! Admission verification reads policy limits, GAS/Notary balances, committee
//! state, and Oracle request state. This provider keeps those reads behind a
//! crate-local capability seam so the verifier depends on the facts it needs,
//! not repeated concrete native-contract handle construction.

use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_crypto::ECPoint;
use neo_error::CoreResult;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_primitives::UInt160;
use neo_storage::{CacheRead, DataCache};
use num_bigint::BigInt;

/// Native-contract capabilities required by mempool admission.
pub(super) trait AdmissionNativeProvider {
    /// Returns whether `account` is blocked by Policy.
    fn policy_is_blocked<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        account: &UInt160,
    ) -> CoreResult<bool>;

    /// Returns the effective max valid-until-block increment.
    fn max_valid_until_block_increment<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32>;

    /// Returns the effective fee per byte.
    fn fee_per_byte<B: CacheRead>(&self, snapshot: &DataCache<B>) -> CoreResult<u32>;

    /// Returns the effective execution fee factor.
    fn exec_fee_factor<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
        height: u32,
    ) -> CoreResult<u32>;

    /// Returns the GAS balance for `account`, or zero on absent/invalid state.
    fn gas_balance<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        account: &UInt160,
    ) -> CoreResult<BigInt>;

    /// Returns the Notary deposit balance for `account`, or zero on absent state.
    fn notary_balance<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        account: &UInt160,
    ) -> CoreResult<BigInt>;

    /// Returns the Notary native contract hash.
    fn notary_hash(&self) -> CoreResult<UInt160>;

    /// Returns the cached NEO committee address.
    fn committee_address<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
    ) -> CoreResult<Option<UInt160>>;

    /// Returns the GAS reserved for a pending Oracle response request.
    fn oracle_response_gas<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        request_id: u64,
    ) -> CoreResult<Option<i64>>;

    /// Returns designated oracle nodes effective at `height`.
    fn designated_oracles<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        height: u32,
    ) -> CoreResult<Vec<ECPoint>>;
}

/// Adapter from the node-composed native-contract provider to the admission
/// verifier's narrow native read capability.
#[derive(Clone)]
pub(super) struct NativeAdmissionProvider<P> {
    native_contract_provider: Arc<P>,
}

impl<P> NativeAdmissionProvider<P>
where
    P: NativeContractProvider,
{
    /// Creates an adapter over the composition-root native-contract provider.
    #[must_use]
    pub(super) fn new(native_contract_provider: Arc<P>) -> Self {
        Self {
            native_contract_provider,
        }
    }

    fn provider(&self) -> &P {
        &self.native_contract_provider
    }
}

impl<P> std::fmt::Debug for NativeAdmissionProvider<P>
where
    P: NativeContractProvider,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeAdmissionProvider")
            .field("native_contract_provider", &"NativeContractProvider")
            .finish()
    }
}

impl<P> AdmissionNativeProvider for NativeAdmissionProvider<P>
where
    P: NativeContractProvider,
{
    fn policy_is_blocked<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        account: &UInt160,
    ) -> CoreResult<bool> {
        self.provider().policy_is_blocked(snapshot, account)
    }

    fn max_valid_until_block_increment<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32> {
        self.provider()
            .max_valid_until_block_increment(snapshot, settings)
    }

    fn fee_per_byte<B: CacheRead>(&self, snapshot: &DataCache<B>) -> CoreResult<u32> {
        self.provider().fee_per_byte(snapshot)
    }

    fn exec_fee_factor<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
        height: u32,
    ) -> CoreResult<u32> {
        self.provider().exec_fee_factor(snapshot, settings, height)
    }

    fn gas_balance<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        account: &UInt160,
    ) -> CoreResult<BigInt> {
        self.provider().gas_balance(snapshot, account)
    }

    fn notary_balance<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        account: &UInt160,
    ) -> CoreResult<BigInt> {
        self.provider().notary_balance(snapshot, account)
    }

    fn notary_hash(&self) -> CoreResult<UInt160> {
        self.provider().notary_hash()
    }

    fn committee_address<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
    ) -> CoreResult<Option<UInt160>> {
        self.provider().committee_address(snapshot)
    }

    fn oracle_response_gas<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        request_id: u64,
    ) -> CoreResult<Option<i64>> {
        self.provider().oracle_response_gas(snapshot, request_id)
    }

    fn designated_oracles<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        height: u32,
    ) -> CoreResult<Vec<ECPoint>> {
        self.provider().designated_oracles(snapshot, height)
    }
}
