//! Native contract read capabilities used by the oracle service.
//!
//! The oracle service needs narrow read access to Oracle, RoleManagement,
//! Policy, and ContractManagement native-contract storage. Keeping those reads
//! behind a crate-local provider seam prevents service loops and transaction
//! builders from constructing concrete native handles at each call site while
//! preserving the native contracts' canonical storage codecs.

use neo_config::ProtocolSettings;
use neo_crypto::ECPoint;
use neo_error::CoreResult;
use neo_execution::ContractState;
use neo_native_contracts::{
    ContractManagement, OracleContract, OracleRequest, PolicyContract, Role, RoleManagement,
};
use neo_primitives::UInt160;
use neo_storage::DataCache;

/// Native-contract read capabilities required by oracle processing.
pub(super) trait OracleServiceNativeProvider {
    /// Returns the Oracle native contract hash.
    fn oracle_hash(&self) -> UInt160;

    /// Returns the deployed Oracle native contract state, when present.
    fn oracle_contract_state(&self, snapshot: &DataCache) -> CoreResult<Option<ContractState>>;

    /// Returns a single pending oracle request by id.
    fn oracle_request(
        &self,
        snapshot: &DataCache,
        request_id: u64,
    ) -> CoreResult<Option<OracleRequest>>;

    /// Returns all pending oracle requests.
    fn oracle_requests(&self, snapshot: &DataCache) -> Vec<(u64, OracleRequest)>;

    /// Returns all pending oracle requests for `url`.
    fn oracle_requests_by_url(
        &self,
        snapshot: &DataCache,
        url: &str,
    ) -> CoreResult<Vec<(u64, OracleRequest)>>;

    /// Returns the designated oracle nodes effective at `height`.
    fn designated_oracles(&self, snapshot: &DataCache, height: u32) -> CoreResult<Vec<ECPoint>>;

    /// Returns the effective max valid-until-block increment.
    fn max_valid_until_block_increment(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32>;

    /// Returns the effective execution fee factor.
    fn exec_fee_factor(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
        height: u32,
    ) -> CoreResult<u32>;

    /// Returns the effective fee per byte.
    fn fee_per_byte(&self, snapshot: &DataCache) -> CoreResult<u32>;
}

/// Factory for native-contract read providers.
pub(super) trait OracleServiceNativeProviderFactory {
    /// Provider returned by this factory.
    type Provider: OracleServiceNativeProvider;

    /// Creates a provider instance.
    fn provider(&self) -> Self::Provider;
}

/// Native-contract backed provider for production oracle processing.
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct NativeOracleServiceProvider {
    oracle: OracleContract,
    roles: RoleManagement,
    policy: PolicyContract,
}

impl NativeOracleServiceProvider {
    /// Creates a provider backed by canonical native contract handles.
    #[must_use]
    pub(super) const fn new() -> Self {
        Self {
            oracle: OracleContract::new(),
            roles: RoleManagement::new(),
            policy: PolicyContract::new(),
        }
    }
}

impl OracleServiceNativeProvider for NativeOracleServiceProvider {
    fn oracle_hash(&self) -> UInt160 {
        self.oracle.hash()
    }

    fn oracle_contract_state(&self, snapshot: &DataCache) -> CoreResult<Option<ContractState>> {
        ContractManagement::get_contract_from_snapshot(snapshot, &self.oracle_hash())
    }

    fn oracle_request(
        &self,
        snapshot: &DataCache,
        request_id: u64,
    ) -> CoreResult<Option<OracleRequest>> {
        self.oracle.get_request(snapshot, request_id)
    }

    fn oracle_requests(&self, snapshot: &DataCache) -> Vec<(u64, OracleRequest)> {
        self.oracle.get_requests(snapshot)
    }

    fn oracle_requests_by_url(
        &self,
        snapshot: &DataCache,
        url: &str,
    ) -> CoreResult<Vec<(u64, OracleRequest)>> {
        self.oracle.get_requests_by_url(snapshot, url)
    }

    fn designated_oracles(&self, snapshot: &DataCache, height: u32) -> CoreResult<Vec<ECPoint>> {
        self.roles
            .get_designated_by_role_at(snapshot, Role::Oracle, height)
    }

    fn max_valid_until_block_increment(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32> {
        self.policy
            .get_max_valid_until_block_increment_snapshot(snapshot, settings)
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

    fn fee_per_byte(&self, snapshot: &DataCache) -> CoreResult<u32> {
        self.policy.get_fee_per_byte_snapshot(snapshot)
    }
}

/// Factory for production native-contract read providers.
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct NativeOracleServiceProviderFactory;

impl OracleServiceNativeProviderFactory for NativeOracleServiceProviderFactory {
    type Provider = NativeOracleServiceProvider;

    fn provider(&self) -> Self::Provider {
        NativeOracleServiceProvider::new()
    }
}
