//! Native contract read capabilities used during mempool admission.
//!
//! Admission verification reads policy limits, GAS/Notary balances, committee
//! state, and Oracle request state. This provider keeps those reads behind a
//! crate-local capability seam so the verifier depends on the facts it needs,
//! not repeated concrete native-contract handle construction.

use neo_config::ProtocolSettings;
use neo_crypto::ECPoint;
use neo_error::CoreResult;
use neo_native_contracts::{
    GasToken, NeoToken, Notary, OracleContract, OracleRequest, PolicyContract, Role, RoleManagement,
};
use neo_primitives::UInt160;
use neo_storage::DataCache;
use num_bigint::BigInt;

/// Native-contract capabilities required by mempool admission.
pub(super) trait AdmissionNativeProvider {
    /// Returns whether `account` is blocked by Policy.
    fn policy_is_blocked(&self, snapshot: &DataCache, account: &UInt160) -> bool;

    /// Returns the effective max valid-until-block increment.
    fn max_valid_until_block_increment(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32>;

    /// Returns the effective fee per byte.
    fn fee_per_byte(&self, snapshot: &DataCache) -> CoreResult<u32>;

    /// Returns the effective execution fee factor.
    fn exec_fee_factor(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
        height: u32,
    ) -> CoreResult<u32>;

    /// Returns the GAS balance for `account`, or zero on absent/invalid state.
    fn gas_balance(&self, snapshot: &DataCache, account: &UInt160) -> BigInt;

    /// Returns the Notary deposit balance for `account`, or zero on absent state.
    fn notary_balance(&self, snapshot: &DataCache, account: &UInt160) -> BigInt;

    /// Returns the Notary native contract hash.
    fn notary_hash(&self) -> UInt160;

    /// Returns the cached NEO committee address.
    fn committee_address(&self, snapshot: &DataCache) -> CoreResult<Option<UInt160>>;

    /// Returns the pending Oracle request by id.
    fn oracle_request(
        &self,
        snapshot: &DataCache,
        request_id: u64,
    ) -> CoreResult<Option<OracleRequest>>;

    /// Returns designated oracle nodes effective at `height`.
    fn designated_oracles(&self, snapshot: &DataCache, height: u32) -> CoreResult<Vec<ECPoint>>;
}

/// Factory for admission native-contract providers.
pub(super) trait AdmissionNativeProviderFactory {
    /// Provider returned by this factory.
    type Provider: AdmissionNativeProvider;

    /// Creates a provider instance.
    fn provider(&self) -> Self::Provider;
}

/// Production provider backed by canonical native-contract handles.
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct NativeAdmissionProvider {
    neo: NeoToken,
    oracle: OracleContract,
    policy: PolicyContract,
    roles: RoleManagement,
}

impl NativeAdmissionProvider {
    /// Creates a provider backed by canonical native contract handles.
    #[must_use]
    pub(super) const fn new() -> Self {
        Self {
            neo: NeoToken::new(),
            oracle: OracleContract::new(),
            policy: PolicyContract::new(),
            roles: RoleManagement::new(),
        }
    }
}

impl AdmissionNativeProvider for NativeAdmissionProvider {
    fn policy_is_blocked(&self, snapshot: &DataCache, account: &UInt160) -> bool {
        PolicyContract::is_blocked_snapshot(snapshot, account)
    }

    fn max_valid_until_block_increment(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32> {
        self.policy
            .get_max_valid_until_block_increment_snapshot(snapshot, settings)
    }

    fn fee_per_byte(&self, snapshot: &DataCache) -> CoreResult<u32> {
        self.policy.get_fee_per_byte_snapshot(snapshot)
    }

    fn exec_fee_factor(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
        height: u32,
    ) -> CoreResult<u32> {
        self.policy
            .get_exec_fee_factor_snapshot(snapshot, settings, height)
    }

    fn gas_balance(&self, snapshot: &DataCache, account: &UInt160) -> BigInt {
        GasToken::balance_of(snapshot, account).unwrap_or_else(|_| BigInt::from(0))
    }

    fn notary_balance(&self, snapshot: &DataCache, account: &UInt160) -> BigInt {
        Notary::balance_of(snapshot, account).unwrap_or_else(|_| BigInt::from(0))
    }

    fn notary_hash(&self) -> UInt160 {
        Notary::script_hash()
    }

    fn committee_address(&self, snapshot: &DataCache) -> CoreResult<Option<UInt160>> {
        neo_execution::NativeContract::committee_address(&self.neo, snapshot)
    }

    fn oracle_request(
        &self,
        snapshot: &DataCache,
        request_id: u64,
    ) -> CoreResult<Option<OracleRequest>> {
        self.oracle.get_request(snapshot, request_id)
    }

    fn designated_oracles(&self, snapshot: &DataCache, height: u32) -> CoreResult<Vec<ECPoint>> {
        self.roles
            .get_designated_by_role_at(snapshot, Role::Oracle, height)
    }
}

/// Factory for production admission native-contract read providers.
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct NativeAdmissionProviderFactory;

impl AdmissionNativeProviderFactory for NativeAdmissionProviderFactory {
    type Provider = NativeAdmissionProvider;

    fn provider(&self) -> Self::Provider {
        NativeAdmissionProvider::new()
    }
}
