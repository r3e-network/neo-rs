//! Native contract read capabilities used during mempool admission.
//!
//! Admission verification reads policy limits, GAS/Notary balances, committee
//! state, and Oracle request state. This provider keeps those reads behind a
//! crate-local capability seam so the verifier depends on the facts it needs,
//! not repeated concrete native-contract handle construction.

use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_crypto::ECPoint;
use neo_error::{CoreError, CoreResult};
use neo_execution::NativeContract;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_native_contracts::{
    GasToken, NeoToken, Notary, OracleContract, OracleRequest, PolicyContract, Role, RoleManagement,
};
use neo_primitives::UInt160;
use neo_storage::DataCache;
use num_bigint::BigInt;

/// Native-contract capabilities required by mempool admission.
pub(super) trait AdmissionNativeProvider {
    /// Returns whether `account` is blocked by Policy.
    fn policy_is_blocked(&self, snapshot: &DataCache, account: &UInt160) -> CoreResult<bool>;

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
    fn gas_balance(&self, snapshot: &DataCache, account: &UInt160) -> CoreResult<BigInt>;

    /// Returns the Notary deposit balance for `account`, or zero on absent state.
    fn notary_balance(&self, snapshot: &DataCache, account: &UInt160) -> CoreResult<BigInt>;

    /// Returns the Notary native contract hash.
    fn notary_hash(&self) -> CoreResult<UInt160>;

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

/// Adapter from the node-composed native-contract provider to the admission
/// verifier's narrow native read capability.
#[derive(Clone)]
pub(super) struct NativeAdmissionProvider<P: ?Sized> {
    native_contract_provider: Arc<P>,
}

impl<P> NativeAdmissionProvider<P>
where
    P: NativeContractProvider + ?Sized,
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

    fn native_contract(&self, name: &'static str) -> CoreResult<Arc<dyn NativeContract>> {
        self.provider()
            .get_native_contract_by_name(name)
            .ok_or_else(|| CoreError::invalid_operation(format!("native provider missing {name}")))
    }

    fn with_contract<T, R>(
        &self,
        name: &'static str,
        f: impl FnOnce(&T) -> CoreResult<R>,
    ) -> CoreResult<R>
    where
        T: 'static,
    {
        let contract = self.native_contract(name)?;
        let concrete = contract.as_any().downcast_ref::<T>().ok_or_else(|| {
            CoreError::invalid_operation(format!("native provider returned non-{name}"))
        })?;
        f(concrete)
    }
}

impl<P> std::fmt::Debug for NativeAdmissionProvider<P>
where
    P: NativeContractProvider + ?Sized,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeAdmissionProvider")
            .field("native_contract_provider", &"NativeContractProvider")
            .finish()
    }
}

impl<P> AdmissionNativeProvider for NativeAdmissionProvider<P>
where
    P: NativeContractProvider + ?Sized,
{
    fn policy_is_blocked(&self, snapshot: &DataCache, account: &UInt160) -> CoreResult<bool> {
        self.with_contract::<PolicyContract, _>("PolicyContract", |_| {
            Ok(PolicyContract::is_blocked_snapshot(snapshot, account))
        })
    }

    fn max_valid_until_block_increment(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32> {
        self.with_contract::<PolicyContract, _>("PolicyContract", |policy| {
            policy.get_max_valid_until_block_increment_snapshot(snapshot, settings)
        })
    }

    fn fee_per_byte(&self, snapshot: &DataCache) -> CoreResult<u32> {
        self.with_contract::<PolicyContract, _>("PolicyContract", |policy| {
            policy.get_fee_per_byte_snapshot(snapshot)
        })
    }

    fn exec_fee_factor(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
        height: u32,
    ) -> CoreResult<u32> {
        self.with_contract::<PolicyContract, _>("PolicyContract", |policy| {
            policy.get_exec_fee_factor_snapshot(snapshot, settings, height)
        })
    }

    fn gas_balance(&self, snapshot: &DataCache, account: &UInt160) -> CoreResult<BigInt> {
        self.with_contract::<GasToken, _>("GasToken", |_| {
            Ok(GasToken::balance_of(snapshot, account).unwrap_or_else(|_| BigInt::from(0)))
        })
    }

    fn notary_balance(&self, snapshot: &DataCache, account: &UInt160) -> CoreResult<BigInt> {
        self.with_contract::<Notary, _>("Notary", |_| {
            Ok(Notary::balance_of(snapshot, account).unwrap_or_else(|_| BigInt::from(0)))
        })
    }

    fn notary_hash(&self) -> CoreResult<UInt160> {
        Ok(self.native_contract("Notary")?.hash())
    }

    fn committee_address(&self, snapshot: &DataCache) -> CoreResult<Option<UInt160>> {
        self.with_contract::<NeoToken, _>("NeoToken", |neo| neo.committee_address(snapshot))
    }

    fn oracle_request(
        &self,
        snapshot: &DataCache,
        request_id: u64,
    ) -> CoreResult<Option<OracleRequest>> {
        self.with_contract::<OracleContract, _>("OracleContract", |oracle| {
            oracle.get_request(snapshot, request_id)
        })
    }

    fn designated_oracles(&self, snapshot: &DataCache, height: u32) -> CoreResult<Vec<ECPoint>> {
        self.with_contract::<RoleManagement, _>("RoleManagement", |roles| {
            roles.get_designated_by_role_at(snapshot, Role::Oracle, height)
        })
    }
}
