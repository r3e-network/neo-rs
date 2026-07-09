//! Native contract read capabilities used by the oracle service.
//!
//! The oracle service needs narrow read access to Oracle, RoleManagement,
//! Policy, and ContractManagement native-contract storage. Keeping those reads
//! behind a crate-local provider seam prevents service loops and transaction
//! builders from constructing concrete native handles at each call site while
//! preserving the native contracts' canonical storage codecs.

use super::OracleService;
use neo_config::ProtocolSettings;
use neo_crypto::ECPoint;
use neo_error::{CoreError, CoreResult};
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_execution::{ContractState, NativeContract};
use neo_native_contracts::{
    ContractManagement, OracleContract, OracleRequest, PolicyContract, Role, RoleManagement,
};
use neo_primitives::UInt160;
use neo_storage::DataCache;
use std::sync::Arc;

/// Native-contract read capabilities required by oracle processing.
pub(super) trait OracleServiceNativeProvider {
    /// Returns the Oracle native contract hash.
    fn oracle_hash(&self) -> CoreResult<UInt160>;

    /// Returns the deployed Oracle native contract state, when present.
    fn oracle_contract_state(&self, snapshot: &DataCache) -> CoreResult<Option<ContractState>>;

    /// Returns a single pending oracle request by id.
    fn oracle_request(
        &self,
        snapshot: &DataCache,
        request_id: u64,
    ) -> CoreResult<Option<OracleRequest>>;

    /// Returns all pending oracle requests.
    fn oracle_requests(&self, snapshot: &DataCache) -> CoreResult<Vec<(u64, OracleRequest)>>;

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

/// Adapter from the node-composed native-contract provider to the oracle
/// service's narrow native read capability.
#[derive(Clone)]
pub(super) struct NativeOracleServiceProvider {
    native_contract_provider: Arc<dyn NativeContractProvider>,
}

impl NativeOracleServiceProvider {
    /// Creates an adapter over the composition-root native-contract provider.
    #[must_use]
    pub(super) fn new(native_contract_provider: Arc<dyn NativeContractProvider>) -> Self {
        Self {
            native_contract_provider,
        }
    }

    fn native_contract(&self, name: &'static str) -> CoreResult<Arc<dyn NativeContract>> {
        self.native_contract_provider
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

impl std::fmt::Debug for NativeOracleServiceProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeOracleServiceProvider")
            .field("native_contract_provider", &"NativeContractProvider")
            .finish()
    }
}

impl OracleService {
    pub(super) fn native_provider(&self) -> NativeOracleServiceProvider {
        NativeOracleServiceProvider::new(Arc::clone(&self.native_contract_provider))
    }
}

impl OracleServiceNativeProvider for NativeOracleServiceProvider {
    fn oracle_hash(&self) -> CoreResult<UInt160> {
        Ok(self.native_contract("OracleContract")?.hash())
    }

    fn oracle_contract_state(&self, snapshot: &DataCache) -> CoreResult<Option<ContractState>> {
        self.with_contract::<ContractManagement, _>("ContractManagement", |_| {
            let oracle_hash = self.oracle_hash()?;
            ContractManagement::get_contract_from_snapshot(snapshot, &oracle_hash)
        })
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

    fn oracle_requests(&self, snapshot: &DataCache) -> CoreResult<Vec<(u64, OracleRequest)>> {
        self.with_contract::<OracleContract, _>("OracleContract", |oracle| {
            Ok(oracle.get_requests(snapshot))
        })
    }

    fn oracle_requests_by_url(
        &self,
        snapshot: &DataCache,
        url: &str,
    ) -> CoreResult<Vec<(u64, OracleRequest)>> {
        self.with_contract::<OracleContract, _>("OracleContract", |oracle| {
            oracle.get_requests_by_url(snapshot, url)
        })
    }

    fn designated_oracles(&self, snapshot: &DataCache, height: u32) -> CoreResult<Vec<ECPoint>> {
        self.with_contract::<RoleManagement, _>("RoleManagement", |roles| {
            roles.get_designated_by_role_at(snapshot, Role::Oracle, height)
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

    fn fee_per_byte(&self, snapshot: &DataCache) -> CoreResult<u32> {
        self.with_contract::<PolicyContract, _>("PolicyContract", |policy| {
            policy.get_fee_per_byte_snapshot(snapshot)
        })
    }
}
